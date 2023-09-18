use std::{
    cell::RefCell,
    collections::{btree_map::Entry, BTreeMap},
    ops::{Bound, RangeBounds},
    rc::Rc,
};

use delegate::delegate;

use crate::{
    abs::{
        self, place::HasMetadata, PointerOffset, RawPointer, TypeId, TypeSize, ValueType,
        USIZE_TYPE,
    },
    backends::basic::{
        expr::{PorterValue, RawConcreteValue},
        place::{LocalWithMetadata, PlaceMetadata},
        VariablesState,
    },
    utils::SelfHierarchical,
};

use super::{
    super::{
        alias::SymValueRefProjector as SymbolicProjector, expr::prelude::*,
        place::PlaceWithMetadata, ValueRef,
    },
    proj::{apply_projs_sym, IndexResolver, ProjectionResolutionExt},
};

type Local = LocalWithMetadata;
type Place = PlaceWithMetadata;
type Projection = crate::abs::Projection<Local>;

type RRef<T> = Rc<RefCell<T>>;

/* NOTE: Memory structure
 * How does this state tries to store (symbolic) objects?
 * We divide symbolic objects into two categories:
 * - Primitives: We assume that symbolic variables can be only from primitive types,
 *   and all expressions built from them are also from primitive types.
 *   These contribute to the majority of values we keep in the memory.
 * - Non-primitives: These are symbolic values that correspond to not
 *   necessarily primitive types such as arrays or ADTs. Since symbolic variables
 *   cannot be of these types, they can only be generated when there is a
 *   symbolic projection.
 *
 * Effectively, non-primitive symbolic values always correspond to multiple
 * objects, and the memory regions they are associated with is non-deterministic.
 * Thus, any read or write to these parts of memory will be also
 * non-deterministic, and we cannot store a value inside their target memory
 * regions directly. Instead, if it is a read, we derive another value that
 * corresponds to multiple objects (in case of read), or update their
 * non-determinism information (in case of write).
 * To understand it better, let's look at an example:
 * ```
 * let x = 10.mark_symbolic();
 * let y = a[x];
 * let z = y.1 + 20;
 * y.0 = z;
 * ```
 * Here the value of `y` is non-deterministic, since it is a result of a
 * symbolic index on `a`. Later the field projection on `y` is also symbolic but
 * corresponds to different objects in the memory (the second field of `a[x]`).
 * We derive another value based on `y` for it. Now in the last line we write to
 * `y.0`. In this case, we need to update the value we have stored in place of
 * `y` to reflect the change that the first field is set to `z`. This happens
 * because `y.0` resides inside a non-deterministic memory region.
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
type Memory = BTreeMap<RawPointer, MemoryObject>;

/* (*)
 * NOTE: Once type system for runtime is built we can use a uniform key for both
 * type categories.
 */
enum TypeKey {
    Id(TypeId),
    Primitive(ValueType),
}

// (*)
const PRIMITIVE_TYPE_ID: TypeId = 0;

/// Provides a mapping for raw pointers to symbolic values.
/// All places that have a valid address are handled by this state, otherwise
/// they will be sent to the `fallback` state to be handled.
pub(in super::super) struct RawPointerVariableState<VS, SP: SymbolicProjector> {
    memory: Memory,
    fallback: VS,
    sym_projector: RRef<SP>,
    return_value_addr: Option<RawPointer>,
}

impl<VS, SP: SymbolicProjector> RawPointerVariableState<VS, SP> {
    pub fn new(fallback: VS, sym_projector: RRef<SP>) -> Self
    where
        VS: VariablesState<Place>,
    {
        Self {
            memory: Default::default(),
            fallback,
            sym_projector,
            return_value_addr: None,
        }
    }

    fn get<'a, 'b>(&'a self, addr: &'b RawPointer, type_id: TypeKey) -> Option<&'a SymValueRef> {
        let (obj_address, (obj_value, obj_type_id)) = self.get_object(*addr)?;

        // FIXME: (*)
        debug_assert_eq!(
            obj_address, addr,
            "Non-deterministic memory regions are not supported yet."
        );

        match type_id {
            /* We assume that a parent host will be queried before its children.
             * So, if the type id is not the same, it means that the object is
             * nested inside the queried object. */
            TypeKey::Id(type_id) if type_id == *obj_type_id => Some(obj_value),
            TypeKey::Primitive(_ty) if *obj_type_id == PRIMITIVE_TYPE_ID => Some(obj_value),
            _ => None,
        }
    }

    fn get_object<'a, 'b>(
        &'a self,
        addr: RawPointer,
    ) -> Option<(&'a RawPointer, &'a MemoryObject)> {
        let cursor = self.memory.upper_bound(Bound::Included(&addr));
        while let Some(start) = cursor.key().copied() {
            // FIXME: (*) no type information is available so we just check for the exact start.
            let size = 1;
            let region = start..(start + size);
            if addr < region.start {
                continue;
            } else if addr >= region.end {
                return None;
            } else {
                return cursor.key_value();
            }
        }

        None
    }

    fn entry_object<'a, 'b>(&'a mut self, addr: RawPointer) -> Entry<'a, RawPointer, MemoryObject> {
        let key = self
            .get_object(addr)
            .map(|(start, _)| *start)
            .unwrap_or(addr);
        self.memory.entry(key)
    }
}

impl<VS: VariablesState<Place>, SP: SymbolicProjector> VariablesState<Place>
    for RawPointerVariableState<VS, SP>
where
    Self: IndexResolver<Local>,
{
    delegate! {
        to self.fallback {
            fn id(&self) -> usize;
        }
    }

    fn copy_place(&self, place: &Place) -> ValueRef {
        let Some(addr) = place.address() else {
            return self.fallback.copy_place(place);
        };

        if let Some((sym_val, sym_projs)) = self.first_symbolic_value(place) {
            return self.handle_sym_value(sym_val, sym_projs).into();
        }

        if let Some(size) = place.metadata().size() {
            if let Some(porter) = Self::try_create_porter(
                addr,
                size,
                |start| self.memory.upper_bound(start),
                |c| c.key_value(),
                |c| c.move_next(),
            ) {
                return porter.to_value_ref();
            }
        }

        Self::create_lazy(addr, place.metadata().ty()).to_value_ref()
    }

    fn try_take_place(&mut self, place: &Place) -> Option<ValueRef> {
        let addr = place.address().or_else(|| {
            if matches!(place.local().as_ref(), abs::Local::ReturnValue) {
                self.return_value_addr.take()
            } else {
                None
            }
        });
        let Some(addr) = addr else {
            return self.fallback.try_take_place(place);
        };

        if let Some((sym_val, sym_projs)) = self.first_symbolic_value_iter(
            place.local().metadata(),
            place.projections(),
            place.projs_metadata(),
        ) {
            return Some(if sym_projs.is_empty() {
                let value = sym_val.clone_to();
                // FIXME: (*)
                self.memory.remove(&addr);
                value
            } else {
                self.handle_sym_value(sym_val, sym_projs).into()
            });
        }

        if let Some(size) = place.metadata().size() {
            if let Some(porter) = Self::try_create_porter(
                addr,
                size,
                |start| self.memory.upper_bound_mut(start),
                |c| c.key_value(),
                |c| {
                    // FIXME: (*)
                    c.remove_current();
                },
            ) {
                return Some(porter.to_value_ref());
            }
        }

        Some(Self::create_lazy(addr, place.metadata().ty()).to_value_ref())
    }

    fn set_place(&mut self, place: &Place, value: ValueRef) {
        let Some(addr) = place.address() else {
            return self.fallback.set_place(place, value);
        };

        if matches!(place.local().as_ref(), abs::Local::ReturnValue) {
            self.return_value_addr = Some(addr);
        }

        if let Some((_sym_val, sym_projs)) = self.first_symbolic_value(place) {
            if !sym_projs.is_empty() {
                todo!("#238");
            }
        }

        self.set_addr(
            addr,
            value,
            /* FIXME: (*) */
            place.metadata().type_id().unwrap_or(PRIMITIVE_TYPE_ID),
        );

        log::debug!("Current memory state: {:?}", self.memory);
    }
}

impl<VS: VariablesState<Place>, SP: SymbolicProjector> RawPointerVariableState<VS, SP> {
    /// Finds the first symbolic value in the chain of projections (hosts) leading to the place.
    /// # Returns
    /// The first symbolic value and the remaining projections to be applied on it.
    fn first_symbolic_value<'a, 'b>(
        &'a self,
        place: &'b Place,
    ) -> Option<(&'a SymValueRef, &'b [Projection])>
    where
        Self: IndexResolver<Local>,
    {
        self.first_symbolic_value_iter(
            place.local().metadata(),
            place.projections(),
            place.projs_metadata(),
        )
    }

    fn first_symbolic_value_iter<'a, 'b>(
        &'a self,
        local_metadata: &PlaceMetadata,
        projs: &'b [Projection],
        projs_metadata: impl Iterator<Item = &'b PlaceMetadata>,
    ) -> Option<(&'a SymValueRef, &'b [Projection])>
    where
        Self: IndexResolver<Local>,
    {
        if let Some(sym_val) =
            self.get(local_metadata.address().as_ref()?, type_key(local_metadata))
        {
            Some((sym_val, projs))
        } else {
            // Checking for the value after each projection.
            projs
                .iter()
                .zip(projs_metadata)
                .enumerate()
                // The first symbolic value in the projection chain.
                .find_map(|(i, (proj, metadata))| {
                    // Checking for symbolic index.
                    if let Projection::Index(index) = proj {
                        if let Some(index) = IndexResolver::get(self, index) {
                            if index.is_symbolic() {
                                let value = todo!("Symbolic index");
                                return Some((i, value));
                            }
                        }
                    }

                    // Or any symbolic value residing in a location in the chain.
                    metadata
                        .address()
                        .and_then(|addr| self.get(&addr, type_key(metadata)))
                        .map(|sym_val| (i, sym_val))
                })
                // Returning the remaining projections.
                .map(|(i, sym_val)| (sym_val, &projs[(Bound::Excluded(i), Bound::Unbounded)]))
        }
    }

    fn handle_sym_value<'a, 'b>(
        &self,
        host: &'a SymValueRef,
        projs: &'b [Projection],
    ) -> SymValueRef
    where
        Self: IndexResolver<Local>,
    {
        apply_projs_sym(
            self.sym_projector.clone(),
            host,
            projs.iter().map(|p| p.resolved_index(self)),
        )
    }

    fn try_create_porter<'a, C: 'a>(
        addr: RawPointer,
        size: TypeSize,
        lower_bound: impl FnOnce(Bound<&RawPointer>) -> C,
        key_value: impl Fn(&C) -> Option<(&RawPointer, &MemoryObject)>,
        move_next: impl Fn(&mut C),
    ) -> Option<PorterValue> {
        let range = addr..addr + size;
        let mut cursor = lower_bound(range.start_bound());
        let mut sym_values = Vec::new();
        while let Some((sym_addr, (sym_value, sym_type_id))) = key_value(&cursor) {
            if *sym_addr < range.start {
                // TODO: Check for symbolic value size.
                continue;
            }

            if !range.contains(sym_addr) {
                break;
            }

            let offset: PointerOffset = sym_addr - addr;
            sym_values.push((offset, *sym_type_id, sym_value.clone()));
            move_next(&mut cursor);
        }

        if !sym_values.is_empty() {
            Some(PorterValue { sym_values })
        } else {
            None
        }
    }

    #[inline]
    fn create_lazy(addr: RawPointer, ty: Option<&ValueType>) -> RawConcreteValue {
        RawConcreteValue(addr, ty.cloned())
    }

    fn set_addr(&mut self, addr: RawPointer, value: ValueRef, type_id: TypeId) {
        fn insert(entry: Entry<RawPointer, MemoryObject>, value: MemoryObject) {
            match entry {
                Entry::Occupied(mut entry) => {
                    entry.insert(value);
                }
                Entry::Vacant(entry) => {
                    entry.insert(value);
                }
            }
        }

        let entry = self.entry_object(addr);

        // FIXME: (*)
        debug_assert_eq!(
            *entry.key(),
            addr,
            "Non-deterministic memory regions are not supported yet."
        );

        match value.as_ref() {
            Value::Symbolic(_) => {
                insert(entry, (SymValueRef::new(value), type_id));
            }
            Value::Concrete(ConcreteValue::Adt(adt)) => {
                for field in adt.fields.iter() {
                    if let Some(value) = &field.value {
                        self.set_addr(
                            addr + field.offset,
                            value.clone(),
                            // FIXME: (*)
                            PRIMITIVE_TYPE_ID,
                        );
                    }
                }
            }
            Value::Concrete(ConcreteValue::Array(array)) => {
                for element in array.elements.iter() {
                    if element.is_symbolic() {
                        todo!("#265: Alignment information is not available yet.");
                    }
                }
            }
            Value::Concrete(ConcreteValue::Unevaluated(UnevalValue::Porter(porter))) => {
                for (offset, type_id, sym_value) in porter.sym_values.iter() {
                    self.set_addr(addr + offset, sym_value.clone_to(), *type_id);
                }
            }
            Value::Concrete(_) => {
                if let Entry::Occupied(entry) = entry {
                    // FIXME: (*)
                    entry.remove();
                }
            }
        }
    }
}

impl<VS, SP: SymbolicProjector> IndexResolver<Local> for RawPointerVariableState<VS, SP>
where
    VS: IndexResolver<Local>,
{
    fn get(&self, local: &Local) -> Option<ValueRef> {
        let Some(addr) = local.address() else {
            return self.fallback.get(local);
        };

        Some(
            if let Some(sym_val) = self.get(&addr, TypeKey::Primitive(USIZE_TYPE.into())) {
                sym_val.clone_to()
            } else {
                UnevalValue::Lazy(RawConcreteValue(addr, Some(USIZE_TYPE.into()))).to_value_ref()
            },
        )
    }
}

impl<VS, SP: SymbolicProjector> SelfHierarchical for RawPointerVariableState<VS, SP>
where
    VS: SelfHierarchical,
{
    fn add_layer(self) -> Self {
        Self {
            fallback: self.fallback.add_layer(),
            ..self
        }
    }

    fn drop_layer(self) -> Option<Self> {
        self.fallback.drop_layer().map(|f| Self {
            fallback: f,
            ..self
        })
    }
}

fn type_key(metadata: &PlaceMetadata) -> TypeKey {
    // If type id is sent, it is a non-primitive type.
    metadata
        .type_id()
        .map(TypeKey::Id)
        .unwrap_or_else(|| TypeKey::Primitive(metadata.ty().cloned().unwrap()))
}
