use std::{ops::DerefMut, rc::Rc};

use crate::{
    abs::{PlaceUsage, PointerOffset, TypeId, TypeSize},
    backends::basic::{
        GenericVariablesState, Implied, Precondition,
        alias::{SymValueRefExprBuilder, TypeDatabase},
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
 *
 * Assumptions:
 * - Accessing a container object happens before a projection over it, or the state may not be able
 *   to retrieve the corresponding value.
 *   For example: if `y.1` is accessed, either we first access `y`, or have the information about it (local + projections).
 *   This is compared to the case in which the field 1 is accessed directly through a pointer.
 * - Similarly, unaligned or any strangely unsafe memory accesses happen very rarely.
 *   The state may work or panic in these cases.
 *   Examples include directly accessing a single byte of an `i32`, or doing the opposite.
 * - We can symbolic objects into two categories:
 *   - Primitives: Symbolic _variables_ (atoms) in this backend can be only from primitive types,
 *     and all expressions built based on them are also from primitive types.
 *     These contribute to the majority of values we keep in the memory.
 *   - Non-primitives: These are the rest of symbolic values that may correspond to
 *     non-primitive types such as arrays or ADTs. Since symbolic variables
 *     cannot be of these types, they can only appear as a form of expression,
 *     and the only expressions that can be of these types are the ones corresponding
 *     to symbolic places (`Expr::Multi`).
 *
 * How do these assumptions affect our implementation?
 *
 * While memory is basically a map from addresses to values, there are cases that
 * need special consideration.
 * (Let's forget about symbolic places for now)
 * - Places corresponding to non-primitives. Examples: `y` with a symbolic values stored at `y.1`.
 *   In case of a read, this value is going to be stored somewhere else. So we create a
 *   (temporary) special value (`Expr::Porter`) which holds the information about
 *   where the symbolic values are stored in it.
 *   Note that we are talking about the first copy in the following:
 *   ```
 *   let a = y;
 *   let b = a.0;
 *   ```
 *   The latter is accessed directly as we have the information for both `a` and `a.0`.
 *   In case of a write, we simply replace the whole region (nothing special).
 * - Places sharing the same address: Examples: `y` and `y.0` or `a` and `a.0`.
 *   In this case, address alone is not enough to know what is being retrieved.
 *   Thus we keep the type as well to distinguish what is being accessed.
 *
 *
 * How are symbolic places handled?
 *
 * (This explanation only concerns the underlying map and not the symbolic place logic).
 * Symbolic reads are handled through a special type of expression (`Expr::Multi`).
 * Symbolic writes are not supported.
 * No special concern is noticed for those from primitive types.
 * For non-primitives, with respect to the first assumption, we know that the
 * object will be accessed first, so we are able to retrieve the symbolic expression
 * first and then can apply the projections on it.
 *
 * The difficulty with symbolic writes are the non-determinism nature of it.
 * For example:
 * ```
 * let x = 10.mark_symbolic();
 * let y = a[x];
 * let z = y.1 + 20;
 * y.0 = z;
 * ```
 * Here, the value of `y` will be a `Multi` expression. When writing to `y.0`,
 * we need to update that expression in such way that all field  0s of items in
 * `a` may have the value of `z`.
 */
/* NOTE: Preconditions information (Draft)
 *
 * Granularity
 * We choose to store preconditions at the object level.
 * For example:
 * ```
 * let y = foo();
 * y.0 = bar();
 * y.1 = baz();
 * ```
 * Here, preconditions of all fields will be merged into `y`.
 * And it's two-way, i.e., when accessing fields of `y` later, the return value will inherit
 * the preconditions of `y`. In other words, we return the preconditions of the memory region
 * of the object that an address falls within.
 */

/// Provides a mapping for raw pointers to symbolic values.
/// All places that have a valid address are handled by this state, otherwise
/// they will be sent to the `fallback` state to be handled.
pub(crate) struct RawPointerVariableState<EB> {
    memory: memory::MemoryGate,
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

    #[inline]
    fn get_type(&self, type_id: TypeId) -> &'static TypeInfo {
        self.type_manager.get_type(&type_id)
    }

    fn get_type_size(&self, place_val: &DeterministicPlaceValue) -> TypeSize {
        place_val
            .type_info()
            .get_size(self.type_manager.as_ref())
            .expect("Copying/Moving of unsized types")
    }
}

impl<EB: SymValueRefExprBuilder> GenericVariablesState for RawPointerVariableState<EB> {
    type PlaceInfo = Place;
    type PlaceValue = PlaceValueRef;
    type Value = Implied<ValueRef>;

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
        ptr: Self::Value,
        ptr_type_id: TypeId,
        usage: PlaceUsage,
    ) -> PlaceValueRef {
        self.get_deref_of_ptr(ptr.value, ptr_type_id, match usage {
            PlaceUsage::Read => self.sym_read_handler.borrow_mut(),
            PlaceUsage::Write => self.sym_write_handler.borrow_mut(),
        })
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn copy_place(&self, place: &PlaceValueRef) -> Self::Value {
        match place.as_ref() {
            PlaceValue::Deterministic(ref place) => self.copy_deterministic_place(place),
            PlaceValue::Symbolic(ref sym_place) => self.copy_symbolic_place(sym_place),
        }
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn take_place(&mut self, place: &PlaceValueRef) -> Self::Value {
        match place.as_ref() {
            PlaceValue::Deterministic(ref place) => self.take_deterministic_place(place),
            PlaceValue::Symbolic(ref sym_place) => self.take_symbolic_place(sym_place),
        }
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn set_place(&mut self, place: &PlaceValueRef, value: Self::Value) {
        match place.as_ref() {
            PlaceValue::Deterministic(ref place) => {
                self.set_deterministic_place(place, value);
            }
            PlaceValue::Symbolic(..) => todo!("#238"),
        }
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn drop_place(&mut self, place: &Self::PlaceValue) {
        match place.as_ref() {
            PlaceValue::Deterministic(ref place) => self.drop_deterministic_place(place),
            PlaceValue::Symbolic(..) => {
                panic!("Erasing a symbolic place is not expected")
            }
        }
    }
}

// Deterministic Place
impl<EB: SymValueRefExprBuilder> RawPointerVariableState<EB> {
    fn copy_deterministic_place(&self, place_val: &DeterministicPlaceValue) -> Implied<ValueRef> {
        let addr = place_val.address();
        let size = self.get_type_size(place_val);

        let values = self.memory.read_values(addr, size);

        let value = match values[..] {
            // Single
            [(_, sym_val, type_id)] if place_val.type_id().eq(type_id) => self
                .retrieve_sym_value(sym_val.clone(), place_val.type_id())
                .into(),
            // None
            [] => place_val.to_raw_value().to_value_ref(),
            // Multiple/Different Id
            _ => self
                .create_porter_for_copy(place_val, size, values)
                .to_value_ref()
                .into(),
        };

        Implied {
            by: Precondition::merge(self.memory.read_preconditions(addr, size)),
            value,
        }
    }

    fn take_deterministic_place(
        &mut self,
        place_val: &DeterministicPlaceValue,
    ) -> Implied<ValueRef> {
        let addr = place_val.address();
        let size = self.get_type_size(place_val);

        let values = self.memory.read_values(addr, size);

        let value = match values[..] {
            // Single
            [(_, sym_val, type_id)] if place_val.type_id().eq(type_id) => self
                .retrieve_sym_value(sym_val.clone(), place_val.type_id())
                .into(),
            // None
            [] => place_val.to_raw_value().to_value_ref(),
            // Multiple/Different Id
            _ => self
                .create_porter_for_move(place_val, size, values)
                .to_value_ref()
                .into(),
        };

        Implied {
            by: Precondition::merge(self.memory.read_preconditions(addr, size)),
            value,
        }
    }

    fn set_deterministic_place(
        &mut self,
        place_val: &DeterministicPlaceValue,
        value: Implied<ValueRef>,
    ) {
        let size = self.get_type_size(place_val);
        self.set_addr(place_val.address(), size, place_val.type_id(), value)
    }

    fn drop_deterministic_place(&mut self, place_val: &DeterministicPlaceValue) {
        let size = self.get_type_size(place_val);
        self.memory.erase_values(place_val.address(), size);
        self.memory
            .erase_preconditions_in(place_val.address(), size);
    }
}

// Porters
impl<EB: SymValueRefExprBuilder> RawPointerVariableState<EB> {
    fn create_porter_for_copy(
        &self,
        place_val: &DeterministicPlaceValue,
        size: TypeSize,
        values: Vec<(Address, &SymValueRef, &TypeId)>,
    ) -> PorterValue {
        let result = Self::create_porter(place_val, values);
        self.inspect_porter_sym_values(&result, size);
        self.retrieve_porter_value(&result)
    }

    fn create_porter_for_move(
        /* &mut */ &self,
        place_val: &DeterministicPlaceValue,
        size: TypeSize,
        values: Vec<(Address, &SymValueRef, &TypeId)>,
    ) -> PorterValue {
        // Disabling removal because of possible use after move.
        // https://github.com/rust-lang/unsafe-code-guidelines/issues/188
        // self.memory.erase(range)
        let result = Self::create_porter(place_val, values);
        self.inspect_porter_sym_values(&result, size);
        self.retrieve_porter_value(&result)
    }

    /// Looks in the region indicated by `addr` and `size` and picks all
    /// symbolic values that are residing in that region. If there is no
    /// symbolic value in that region, returns `None`.
    #[tracing::instrument(level = "debug", skip_all)]
    fn create_porter(
        place_val: &DeterministicPlaceValue,
        sym_values: Vec<(Address, &SymValueRef, &TypeId)>,
    ) -> PorterValue {
        let obj_addr = place_val.address();
        let sym_values = sym_values
            .into_iter()
            .map(|(addr, sym_value, sym_type_id)| {
                let offset: PointerOffset = unsafe { addr.byte_offset_from(obj_addr) }
                    .try_into()
                    .unwrap();
                (offset, *sym_type_id, sym_value.clone())
            })
            .collect();

        PorterValue {
            as_concrete: place_val.to_raw_value(),
            sym_values,
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
    fn copy_symbolic_place(&self, place_val: &SymbolicPlaceValue) -> Implied<ValueRef> {
        self.resolve_and_retrieve_symbolic_place(place_val)
            .map_value(Into::into)
    }

    #[inline]
    fn take_symbolic_place(&self, place_val: &SymbolicPlaceValue) -> Implied<ValueRef> {
        // Currently, no different behavior from copying unless needed.
        self.copy_symbolic_place(place_val)
    }
}

// Setting (storing)
impl<EB: SymValueRefExprBuilder> RawPointerVariableState<EB> {
    fn set_addr(
        &mut self,
        addr: Address,
        size: TypeSize,
        type_id: TypeId,
        value: Implied<ValueRef>,
    ) {
        let mut sym_values = Vec::new();
        self.to_sym_values(&mut sym_values, 0, size, value.value, type_id);
        self.memory.replace_values(addr, size, sym_values);
        self.memory.append_merge_precondition(addr, size, value.by);
    }

    fn to_sym_values(
        &self,
        values: &mut Vec<(PointerOffset, TypeSize, SymValueRef, TypeId)>,
        base_offset: PointerOffset,
        size: TypeSize,
        value: ValueRef,
        type_id: TypeId,
    ) {
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
                self.to_sym_values(
                    values,
                    base_offset,
                    size,
                    self.retrieve_sym_value(SymValueRef::new(value), type_id)
                        .into(),
                    type_id,
                );
            }
            Value::Symbolic(SymValue::Expression(Expr::Partial(porter))) => {
                self.to_sym_values_porter(values, base_offset, size, porter, type_id)
            }
            Value::Symbolic(_) => {
                values.push((base_offset, size, SymValueRef::new(value), type_id));
            }
            Value::Concrete(ConcreteValue::Adt(adt)) => {
                self.to_sym_values_adt(values, base_offset, adt, type_id);
            }
            Value::Concrete(ConcreteValue::Array(array)) => {
                self.to_sym_values_array(values, base_offset, array, type_id)
            }
            Value::Concrete(_) => {}
        }
    }

    #[tracing::instrument(level = "debug", skip(self, adt), fields(value = %adt))]
    fn to_sym_values_adt(
        &self,
        values: &mut Vec<(PointerOffset, TypeSize, SymValueRef, TypeId)>,
        base_offset: PointerOffset,
        adt: &AdtValue,
        type_id: TypeId,
    ) {
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
                        self.to_sym_values(
                            values,
                            base_offset + info.offset,
                            self.get_type(info.ty).size().unwrap(),
                            value.clone(),
                            info.ty,
                        );
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
                self.to_sym_values(
                    values,
                    base_offset + offset,
                    self.get_type(fields[i].ty).size().unwrap(),
                    value.clone(),
                    fields[i].ty,
                );
            }
            _ => panic!(
                "Unexpected shape for fields of an ADT: {:?}",
                variant.fields
            ),
        };
    }

    #[tracing::instrument(level = "debug", skip(self, array), fields(value = %array))]
    fn to_sym_values_array(
        &self,
        values: &mut Vec<(PointerOffset, TypeSize, SymValueRef, TypeId)>,
        base_offset: PointerOffset,
        array: &ArrayValue,
        type_id: TypeId,
    ) {
        let item_ty = {
            let item_ty_id = self.get_type(type_id).expect_array().item_ty;
            self.get_type(item_ty_id)
        };
        let item_size = item_ty.size().unwrap();

        for (i, element) in array.elements.iter().enumerate() {
            self.to_sym_values(
                values,
                base_offset + (item_ty.size * i as PointerOffset),
                item_size,
                element.clone(),
                item_ty.id,
            );
        }
    }

    #[tracing::instrument(level = "debug", skip(self, porter), fields(value = %porter))]
    fn to_sym_values_porter(
        &self,
        values: &mut Vec<(PointerOffset, TypeSize, SymValueRef, TypeId)>,
        base_offset: PointerOffset,
        size: TypeSize,
        porter: &PorterValue,
        type_id: TypeId,
    ) {
        let opt_masked_value = porter.try_to_masked_value(
            self.type_manager.as_ref(),
            self.expr_builder.borrow_mut().deref_mut(),
        );
        if let Ok(value) = opt_masked_value {
            values.push((0, size, value.into(), type_id))
        } else {
            for (offset, type_id, sym_value) in porter.sym_values.iter() {
                self.to_sym_values(
                    values,
                    base_offset + *offset,
                    self.get_type(*type_id).size().unwrap(),
                    sym_value.clone_to(),
                    *type_id,
                );
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
            .value
    }
}
