use std::{
    cell::RefCell,
    collections::btree_map::Entry,
    ops::{DerefMut, Range},
    rc::Rc,
};

use crate::{
    abs::{PlaceUsage, PointerOffset, TypeId, TypeSize},
    backends::basic::{
        GenericVariablesState,
        alias::{SymValueRefExprBuilder, TypeDatabase},
        config::{SymbolicPlaceConfig, SymbolicPlaceStrategy},
        expr::{
            lazy::RawPointerRetriever,
            place::{DeterministicPlaceValue, SymbolicPlaceValue},
        },
    },
    type_info::TypeInfoExt,
    utils::{InPlaceSelfHierarchical, alias::RRef},
};
use common::type_info::{FieldsShapeInfo, StructShape, TypeInfo, UnionShape};

use super::super::{
    ValueRef,
    expr::prelude::*,
    place::{LocalWithMetadata, PlaceWithMetadata, Projection},
};

mod memory;
pub(super) mod sym_place;
use common::log_debug;
use memory::*;

type Local = LocalWithMetadata;
type Place = PlaceWithMetadata;

type SymPlaceHandlerDyn = dyn super::SymPlaceHandler<
        SymEntity = SymValueRef,
        ConcEntity = ConcreteValueRef,
        Entity = ValueRef,
    >;

type SymPlaceHandlerObject = RRef<SymPlaceHandlerDyn>;

/* NOTE: Memory structure
 * How does this state tries to store (symbolic) objects?
 * We divide symbolic objects into two categories:
 * - Primitives: We assume that symbolic _variables_ can be only from primitive types,
 *   and all expressions built based on them are also from primitive types.
 *   These contribute to the majority of values we keep in the memory.
 * - Non-primitives: These are the rest of symbolic values that may correspond to
 *   non-primitive types such as arrays or ADTs. Since symbolic variables
 *   cannot be of these types, they can only appear when a symbolic projection
 *   occurs.
 *
 * Effectively, non-primitive symbolic values always correspond to multiple
 * objects, and the memory regions they are associated with is non-deterministic.
 * This means any read or write to these parts of memory will be also
 * non-deterministic, and we cannot store a value inside their target memory
 * regions directly. Instead, we derive another symbolic value that corresponds
 * to multiple objects (in case of read), or update the non-determinism
 * information of the current symbolic value (in case of write).
 * To understand it better, let's look at an example:
 * ```
 * let x = 10.mark_symbolic();
 * let y = a[x];
 * let z = y.1 + 20;
 * y.0 = z;
 * ```
 * Here, the value of `y` is non-deterministic (it is the result of a symbolic
 * index on `a`). Later, the field projection on `y` is also symbolic but
 * corresponds to different objects in the memory (the second field of `a[x]`).
 * We derive another value based on `y` for it. In the last line, we write to
 * `y.0`, a non-deterministic location, so we need to update the value we have
 * stored in place of `y` to reflect the change that the first field is set to
 * `z`.
 *
 * Therefore, any write to any part of a non-deterministic memory region will
 * update the information for the whole region. This brings the important
 * guarantee that there will be no overlapping objects in the memory.
 *
 * Getting back to the implementation, to keep track of these regions, we need
 * to know the memory layout of each object during the runtime. This can be
 * achieved by getting type of each place (and having the information about
 * each type). In the base case, the size of the type is enough to determine
 * the region. This also assists us when different places have the same address.
 * (e.g., `y` and `y.0` in the example above).
 *
 * Current state: (*)
 * We don't have the exact type information and just have an id. So we can partly
 * distinguish between primitive and non-primitive types and non-primitive types
 * themselves.
 * Also, we don't have the layout information so we skip exact reasoning about
 * regions.
 */

type MemoryObject = (SymValueRef, TypeId);

/// Provides a mapping for raw pointers to symbolic values.
/// All places that have a valid address are handled by this state, otherwise
/// they will be sent to the `fallback` state to be handled.
pub(in super::super) struct RawPointerVariableState<EB> {
    memory: memory::Memory,
    type_manager: Rc<dyn TypeDatabase>,
    sym_read_handler: SymPlaceHandlerObject,
    sym_write_handler: SymPlaceHandlerObject,
    expr_builder: RRef<EB>,
}

impl<EB: SymValueRefExprBuilder> RawPointerVariableState<EB> {
    pub fn new(
        type_manager: Rc<dyn TypeDatabase>,
        sym_read_handler: SymPlaceHandlerObject,
        sym_write_handler: SymPlaceHandlerObject,
        expr_builder: RRef<EB>,
    ) -> Self {
        Self {
            memory: Default::default(),
            type_manager,
            sym_read_handler,
            sym_write_handler,
            expr_builder,
        }
    }

    fn get<'a, 'b>(&'a self, addr: Address, type_id: TypeId) -> Option<&'a SymValueRef> {
        log_debug!(
            "Querying memory for address: {:p} with type: {:?}",
            addr,
            type_id
        );

        let (obj_address, (obj_value, obj_type_id)) = self.get_object(addr)?;

        log_debug!(
            "Found value {} for address: {:p} with type: {:?}",
            obj_value,
            addr,
            obj_type_id,
        );

        // FIXME: (*)
        debug_assert_eq!(
            *obj_address, addr,
            "Non-deterministic memory regions are not supported yet."
        );

        /* We assume that a parent host will be queried before its children.
         * So, if the type id is not the same, it means that the object is
         * nested inside the queried object. */
        if obj_type_id.eq(&type_id) {
            Some(obj_value)
        } else {
            log_debug!(
                "Faced an (nested) object with different type: {:?}",
                obj_type_id
            );
            None
        }
    }

    /// Returns the object that contains the given address.
    fn get_object<'a, 'b>(&'a self, addr: Address) -> Option<(&'a Address, &'a MemoryObject)> {
        let cursor = self.memory.before_or_at(&addr);
        if let entry @ Some((start, ..)) = cursor.peek_prev() {
            let size = 1;
            let region = *start..(start.wrapping_byte_add(size));
            if region.contains(&addr) {
                return entry;
            }
        }

        None
    }

    fn entry_object<'a, 'b>(&'a mut self, addr: Address) -> Entry<'a, Address, MemoryObject> {
        let key = self
            .get_object(addr)
            .map(|(start, _)| *start)
            .unwrap_or(addr);
        self.memory.entry_at(key)
    }

    #[inline]
    fn get_type(&self, type_id: TypeId) -> &'static TypeInfo {
        self.type_manager.get_type(&type_id)
    }

    fn get_type_size(&self, place_val: &DeterministicPlaceValue) -> Option<TypeSize> {
        let ty = self.type_manager.get_type(&place_val.type_id());
        ty.is_sized().then_some(ty.size)
    }
}

impl<EB: SymValueRefExprBuilder> GenericVariablesState for RawPointerVariableState<EB> {
    type PlaceInfo = Place;
    type PlaceValue = PlaceValueRef;
    type Value = ValueRef;

    fn id(&self) -> usize {
        // FIXME
        0
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn ref_place(&self, place: &Place, usage: PlaceUsage) -> PlaceValueRef {
        self.get_place(place, match usage {
            PlaceUsage::Read => self.sym_read_handler.borrow_mut(),
            PlaceUsage::Write => self.sym_write_handler.borrow_mut(),
        })
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn ref_place_by_ptr(
        &self,
        ptr: ValueRef,
        ptr_type_id: TypeId,
        usage: PlaceUsage,
    ) -> PlaceValueRef {
        self.get_deref_of_ptr(ptr, ptr_type_id, match usage {
            PlaceUsage::Read => self.sym_read_handler.borrow_mut(),
            PlaceUsage::Write => self.sym_write_handler.borrow_mut(),
        })
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn copy_place(&self, place: &PlaceValueRef) -> ValueRef {
        match place.as_ref() {
            PlaceValue::Deterministic(ref place) => self.copy_deterministic_place(place),
            PlaceValue::Symbolic(ref sym_place) => self.copy_symbolic_place(sym_place).into(),
        }
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn take_place(&mut self, place: &PlaceValueRef) -> ValueRef {
        match place.as_ref() {
            PlaceValue::Deterministic(ref place) => self.take_deterministic_place(place),
            PlaceValue::Symbolic(ref sym_place) => self.take_symbolic_place(sym_place).into(),
        }
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn set_place(&mut self, place: &PlaceValueRef, value: ValueRef) {
        match place.as_ref() {
            PlaceValue::Deterministic(ref place) => {
                self.set_deterministic_place(place, value);
            }
            PlaceValue::Symbolic(..) => todo!("#238"),
        }
    }
}

// Deterministic Place
impl<EB: SymValueRefExprBuilder> RawPointerVariableState<EB> {
    fn copy_deterministic_place(&self, place_val: &DeterministicPlaceValue) -> ValueRef {
        let addr = place_val.address();

        // If the place is pointing to a symbolic value.
        if let Some(sym_val) = self.get(addr, place_val.type_id()) {
            return self
                .retrieve_sym_value(sym_val.clone(), place_val.type_id())
                .into();
        }

        // Or it is pointing to an object embracing symbolic values.
        // FIXME: Double querying memory.
        if let Some(porter) = self.try_create_porter_for_copy(place_val) {
            return porter.to_value_ref().into();
        }

        place_val.to_raw_value().to_value_ref()
    }

    fn take_deterministic_place(&mut self, place_val: &DeterministicPlaceValue) -> ValueRef {
        let addr = place_val.address();

        // If the place is pointing to a symbolic value.
        if let Some(sym_val) = self.get(addr, place_val.type_id()) {
            // Disabling removal because of possible use after move.
            // https://github.com/rust-lang/unsafe-code-guidelines/issues/188
            // self.memory.remove_at(&addr);

            return self
                .retrieve_sym_value(sym_val.clone(), place_val.type_id())
                .into();
        }

        let lazy = place_val.to_raw_value();
        // Or it is pointing to an object embracing symbolic values.
        // FIXME: Double querying memory.
        if let Some(porter) = self.try_create_porter_for_move(place_val) {
            return porter.to_value_ref().into();
        }

        lazy.to_value_ref()
    }

    fn set_deterministic_place(&mut self, place_val: &DeterministicPlaceValue, value: ValueRef) {
        let size = self.get_type_size(place_val).unwrap();
        self.memory.drain_range_and_apply(
            place_val.address()..(place_val.address().wrapping_byte_add(size as usize)),
            |_, _| {},
        );
        self.set_addr(place_val.address(), 0, value, place_val.type_id())
    }
}

// Porters
impl<EB: SymValueRefExprBuilder> RawPointerVariableState<EB> {
    fn try_create_porter_for_copy(
        &self,
        place_val: &DeterministicPlaceValue,
    ) -> Option<PorterValue> {
        let size = self
            .get_type_size(place_val)
            .expect("Cannot copy unsized type.");
        Self::try_create_porter(place_val, size, |range, f| {
            self.memory.apply_in_range(range, f)
        })
        .inspect(|porter| self.inspect_porter_sym_values(porter, size))
        .map(|porter| self.retrieve_porter_value(&porter))
    }

    fn try_create_porter_for_move(
        &mut self,
        place_val: &DeterministicPlaceValue,
    ) -> Option<PorterValue> {
        let size = self
            .get_type_size(place_val)
            .expect("Cannot move unsized type.");
        Self::try_create_porter(place_val, size, |range, mut f| {
            // FIXME: (*)
            // Disabling removal because of possible use after move.
            // https://github.com/rust-lang/unsafe-code-guidelines/issues/188
            // self.memory.drain_range(range, |addr, obj| f(&addr, &obj))
            self.memory
                .apply_in_range(range, |addr, obj| f(&addr, &obj))
        })
        .inspect(|porter| self.inspect_porter_sym_values(porter, size))
        .map(|porter| self.retrieve_porter_value(&porter))
    }

    /// Looks in the region indicated by `addr` and `size` and picks all
    /// symbolic values that are residing in that region. If there is no
    /// symbolic value in that region, returns `None`.
    #[tracing::instrument(level = "debug", skip(apply_in_range))]
    fn try_create_porter(
        place_val: &DeterministicPlaceValue,
        size: TypeSize,
        apply_in_range: impl for<'a> FnOnce(
            Range<Address>,
            Box<dyn FnMut(&Address, &MemoryObject) + 'a>,
        ),
    ) -> Option<PorterValue> {
        // TODO: What if the address is at the middle of a symbolic value?

        let obj_addr = place_val.address();
        let range = obj_addr..(obj_addr.wrapping_byte_add(size as usize));
        let mut sym_values = Vec::new();

        apply_in_range(
            range,
            Box::new(|addr, (sym_value, sym_type_id)| {
                let offset: PointerOffset = unsafe { addr.byte_offset_from(obj_addr) }
                    .try_into()
                    .unwrap();
                sym_values.push((offset, *sym_type_id, sym_value.clone()));
            }),
        );

        if !sym_values.is_empty() {
            Some(PorterValue {
                as_concrete: place_val.to_raw_value(),
                sym_values,
            })
        } else {
            None
        }
    }

    fn inspect_porter_sym_values(&self, porter: &PorterValue, porter_size: TypeSize) {
        for (offset, type_id, _) in &porter.sym_values {
            let value_size = self.type_manager.get_type(type_id).size;
            if offset + value_size > porter_size {
                unimplemented!(
                    "Overflowing symbolic values in a porter are not handled yet: {:?}",
                    porter
                );
            }
        }
    }
}

// Symbolic Place
impl<EB: SymValueRefExprBuilder> RawPointerVariableState<EB> {
    #[inline]
    fn copy_symbolic_place(&self, place_val: &SymbolicPlaceValue) -> SymValueRef {
        self.resolve_and_retrieve_symbolic_place(place_val)
    }

    #[inline]
    fn take_symbolic_place(&self, place_val: &SymbolicPlaceValue) -> SymValueRef {
        // Currently, no different behavior from copying unless needed.
        self.copy_symbolic_place(place_val)
    }
}

// Setting (storing)
impl<EB: SymValueRefExprBuilder> RawPointerVariableState<EB> {
    fn set_addr(&mut self, addr: Address, offset: PointerOffset, value: ValueRef, type_id: TypeId) {
        fn insert(entry: Entry<Address, MemoryObject>, value: MemoryObject) {
            log_debug!(
                "Storing value: {} with type {} at address: {:p}",
                value.0,
                value.1,
                *entry.key()
            );
            match entry {
                Entry::Occupied(mut entry) => {
                    entry.insert(value);
                }
                Entry::Vacant(entry) => {
                    entry.insert(value);
                }
            }
        }

        let addr = addr.wrapping_byte_add(offset as usize);
        let entry = self.entry_object(addr);

        // FIXME: (*)
        debug_assert_eq!(
            *entry.key(),
            addr,
            "Non-deterministic memory regions are not supported yet."
        );

        match value.as_ref() {
            /* NOTE: This is a way of keeping symbolic place handling internal to this module.
             * We rely on the fact that these expressions are stored right after creation.
             * Ideally the creator of these expressions should take care of retrieval or even
             * these expressions should not exist. */
            Value::Symbolic(SymValue::Expression(Expr::Len(..) | Expr::PtrMetadata(..))) => {
                /* NOTE: Don't we need to resolve (the symbolic place) before retrieval?
                 * The only case that holds an unresolved symbolic place is Ref expression,
                 * which cannot appear as the target value.
                 * For Len, it is a deref over a slice pointer/ref, which cannot be
                 * a symbolic reference (o.w., it would be possible to have a standalone value
                 * from a slice type which is unsized. Also, ref over deref gets optimized.).
                 * For PtrMetadata, the reference is to an unsized type (o.w., it gets optimized),
                 * and the same as above holds. */
                self.set_addr(
                    addr,
                    offset,
                    self.retrieve_sym_value(SymValueRef::new(value), type_id)
                        .into(),
                    type_id,
                );
            }
            Value::Symbolic(SymValue::Expression(Expr::Partial(porter))) => {
                self.set_addr_porter(addr, porter, type_id)
            }
            Value::Symbolic(_) => {
                insert(entry, (SymValueRef::new(value), type_id));
            }
            Value::Concrete(ConcreteValue::Adt(adt)) => {
                self.set_addr_adt(addr, adt, type_id);
            }
            Value::Concrete(ConcreteValue::Array(array)) => {
                self.set_addr_array(addr, array, type_id)
            }
            Value::Concrete(_) => {
                if let Entry::Occupied(entry) = entry {
                    // FIXME: (*)
                    entry.remove();
                }
            }
        }
    }

    #[tracing::instrument(level = "debug", skip(self, adt), fields(value = %adt))]
    fn set_addr_adt(&mut self, addr: Address, adt: &AdtValue, type_id: TypeId) {
        let ty = self.get_type(type_id);
        let variant = match adt.kind {
            /* NOTE: Don't we need to take care of tag value and possible symbolic value for it?
             * No, because the tag is always concrete at the time of construction.
             * Even if it it is niche, it will be calculated based on a field value which is
             * taken care of. */
            AdtKind::Enum { variant } => ty.get_variant(variant).unwrap(),
            _ => ty.expect_single_variant(),
        };

        match &variant.fields {
            FieldsShapeInfo::Struct(StructShape { fields }) => {
                for (field, info) in adt.fields.iter().zip(fields) {
                    if let Some(value) = &field.value {
                        self.set_addr(addr, info.offset, value.clone(), info.ty);
                    }
                }
            }
            FieldsShapeInfo::Union(UnionShape { fields }) => {
                let field_find = &mut adt
                    .fields
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| f.value.as_ref().map(|v| (i, v.clone())));
                let (i, value) = field_find
                    .next()
                    .expect("Could not find the single field of a union.");
                debug_assert!(
                    field_find.next().is_none(),
                    "Multiple fields found in a union."
                );
                /* NOTE: The documents about union is a bit unreliable.
                 * - https://doc.rust-lang.org/reference/types/union.html
                 * - https://doc.rust-lang.org/nightly/nightly-rustc/rustc_target/abi/enum.FieldsShape.html#variant.Union
                 */
                let offset = 0;
                self.set_addr(addr, offset, value.clone(), fields[i].ty);
            }
            _ => panic!(
                "Unexpected shape for fields of an ADT: {:?}",
                variant.fields
            ),
        };
    }

    #[tracing::instrument(level = "debug", skip(self, array), fields(value = %array))]
    fn set_addr_array(&mut self, addr: Address, array: &ArrayValue, type_id: TypeId) {
        let item_ty = {
            let item_ty_id = self.get_type(type_id).expect_array().item_ty;
            self.get_type(item_ty_id)
        };

        for (i, element) in array.elements.iter().enumerate() {
            self.set_addr(
                addr,
                item_ty.size * i as TypeSize,
                element.clone(),
                item_ty.id,
            );
        }
    }

    #[tracing::instrument(level = "debug", skip(self, porter), fields(value = %porter))]
    fn set_addr_porter(&mut self, addr: Address, porter: &PorterValue, type_id: TypeId) {
        let opt_masked_value = porter.try_to_masked_value(
            self.type_manager.as_ref(),
            self.expr_builder.borrow_mut().deref_mut(),
        );
        if let Ok(value) = opt_masked_value {
            self.set_addr(addr, 0, value.into(), type_id)
        } else {
            for (offset, type_id, sym_value) in porter.sym_values.iter() {
                self.set_addr(addr, *offset, sym_value.clone_to(), *type_id);
            }
        }
    }
}

impl<EB> InPlaceSelfHierarchical for RawPointerVariableState<EB> {
    fn add_layer(&mut self) {
        // Nothing to do.
    }

    fn drop_layer(&mut self) -> Option<Self> {
        None
    }
}

impl<EB: SymValueRefExprBuilder> RawPointerRetriever for RawPointerVariableState<EB> {
    #[tracing::instrument(level = "debug", skip(self))]
    fn retrieve(&self, addr: RawAddress, type_id: TypeId) -> ValueRef {
        self.copy_deterministic_place(&DeterministicPlaceValue::from_addr_type(addr, type_id))
    }
}
