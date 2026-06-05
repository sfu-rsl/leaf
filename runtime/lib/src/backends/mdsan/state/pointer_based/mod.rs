mod memory;

use std::{num::NonZero, rc::Rc};

use crate::{
    abs::{PlaceUsage, PointerOffset, RawAddress, TypeId, TypeSize, place::HasMetadata},
    utils::byte_offset_from,
};

use super::backend;

use backend::{
    MdMemoryState, MdSanTypeManager,
    place::{PlaceWithMetadata, Projection},
    state::{MdState, MemoryRegion, PlaceValue, WritablePlace},
};

use super::Value;

use memory::*;

type Place = PlaceWithMetadata;

pub(in super::super) struct RawPointerVariableState {
    memory: memory::MemoryGate<MdState>,
    type_manager: Rc<MdSanTypeManager>,
}

impl RawPointerVariableState {
    pub fn new(type_manager: Rc<MdSanTypeManager>) -> Self {
        Self {
            memory: Default::default(),
            type_manager,
        }
    }

    fn get_type_size(&self, type_id: TypeId) -> TypeSize {
        self.type_manager.get_size(&type_id).unwrap()
    }
}

type MemObject = MdState;

impl MdMemoryState for RawPointerVariableState {
    type PlaceInfo = Place;
    type PlaceValue = PlaceValue;
    type ToInspectPlaceValue = RawAddress;
    type ToTakePlaceValue = PlaceValue;
    type ToSetPlaceValue = PlaceValue;
    type ToUpdatePlaceValue = PlaceValue;
    type ToErasePlaceValue = MemoryRegion;
    type ValueForAddress = MemObject;
    type Value = Value;

    fn ref_place(&self, place: Self::PlaceInfo, usage: PlaceUsage) -> Self::PlaceValue {
        self.get_place(place, usage)
    }

    fn ref_place_by_ptr(
        &self,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        usage: PlaceUsage,
    ) -> Self::PlaceValue {
        self.get_deref_of_ptr(conc_ptr, ptr_type_id, usage)
    }

    fn peek_place(&self, place: &Self::ToInspectPlaceValue) -> Option<&Self::ValueForAddress> {
        self.peek_addr(place)
    }

    fn take_place(&mut self, place: &Self::ToTakePlaceValue) -> Self::Value {
        match place {
            PlaceValue::NonRelevant { .. } => Value::non_rel(),
            PlaceValue::AccessedMdWrapped { .. } => {
                // Calling into_inner: Decommission of the MD wrapper, nothing to carry
                Value::non_rel()
            }
            PlaceValue::ToCarryMdContainer { mem_region } => self.take_region(*mem_region),
            PlaceValue::LazyDestination(..)
            | PlaceValue::LifetimeMarkedMd { .. }
            | PlaceValue::ToDropMaybeMdWrapped { .. } => unreachable!(),
        }
    }

    fn set_place(&mut self, place: &Self::ToSetPlaceValue, value: Self::Value) {
        if !value.is_rel() {
            return;
        }

        match place {
            PlaceValue::LazyDestination(writable_place) => {
                self.set_region(writable_place.memory_region(&self.type_manager), value)
            }
            PlaceValue::NonRelevant {} => {
                panic!("Setting labels to a place that was assessed as non-relevant")
            }
            PlaceValue::AccessedMdWrapped { .. }
            | PlaceValue::ToCarryMdContainer { .. }
            | PlaceValue::LifetimeMarkedMd { .. }
            | PlaceValue::ToDropMaybeMdWrapped { .. } => {
                panic!("Unexpected place type for setting a value")
            }
        }
    }

    fn mark_place_dropped(&mut self, place: &Self::ToUpdatePlaceValue) {
        match place {
            PlaceValue::ToDropMaybeMdWrapped { wrapped_addr } => {
                let _old_value = self.update_addr(*wrapped_addr, MdState::Dropped);
            }
            PlaceValue::NonRelevant {} => {}
            PlaceValue::AccessedMdWrapped { .. }
            | PlaceValue::ToCarryMdContainer { .. }
            | PlaceValue::LazyDestination(..)
            | PlaceValue::LifetimeMarkedMd { .. } => {
                panic!("Unexpected place type for marking as dropped")
            }
        }
    }

    fn set_place_alive(&mut self, place: &Self::ToUpdatePlaceValue) {
        match place {
            PlaceValue::LazyDestination(writable_place) => {
                let mem_region = writable_place.memory_region(&self.type_manager);
                self.set_region(mem_region, Value::fresh(mem_region.size))
            }
            PlaceValue::NonRelevant {} => {
                panic!("Setting labels to a place that was assessed as non-relevant")
            }
            PlaceValue::AccessedMdWrapped { .. }
            | PlaceValue::ToCarryMdContainer { .. }
            | PlaceValue::LifetimeMarkedMd { .. }
            | PlaceValue::ToDropMaybeMdWrapped { .. } => {
                panic!("Unexpected place type for setting a value")
            }
        }
    }

    fn erase_place(&mut self, place: &Self::ToErasePlaceValue) -> bool {
        self.erase_region(*place)
    }

    fn copy_raw_memory(
        &mut self,
        conc_src_ptr: RawAddress,
        conc_dst_ptr: RawAddress,
        ptr_type_id: TypeId,
        conc_count: usize,
    ) {
        self.copy_region_raw(
            conc_src_ptr,
            conc_dst_ptr,
            self.type_manager.get_pointee_size(&ptr_type_id).unwrap() * conc_count as TypeSize,
        )
    }
}

impl RawPointerVariableState {
    #[tracing::instrument(level = "debug", skip(self))]
    fn peek_addr(&self, addr: &RawAddress) -> Option<&MemObject> {
        self.memory.get_containing(*addr)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn update_addr(&mut self, addr: RawAddress, value: MemObject) -> Option<MemObject> {
        self.memory.update_containing(addr, value)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn erase_region(&mut self, region: MemoryRegion) -> bool {
        let addr = region.addr;
        let size = region.size;
        self.memory.erase_objects(addr, size) > 0
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn copy_region_raw(&mut self, src_addr: RawAddress, dst_addr: RawAddress, size: TypeSize) {
        let values = self.memory.read_objects(src_addr, size);
        let values = self.convert_to_offsets(src_addr, values);
        self.memory.replace_objects(dst_addr, size, values);
    }
}

impl RawPointerVariableState {
    #[tracing::instrument(level = "debug", skip(self))]
    fn take_region(&mut self, region: MemoryRegion) -> Value {
        let addr = region.addr;
        let size = region.size;

        let values = self.memory.read_objects(addr, size);
        let values = self.convert_to_offsets(addr, values);
        self.memory.erase_objects(addr, size);
        Value::new(values)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn set_region(&mut self, region: MemoryRegion, value: Value) {
        let addr = region.addr;
        let size = region.size;
        self.memory.replace_objects(addr, size, value.labels);
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn drop_region(&mut self, region: MemoryRegion) {
        let addr = region.addr;
        let size = region.size;
        self.memory.erase_objects(addr, size);
    }
}

// Porters
impl RawPointerVariableState {
    #[tracing::instrument(level = "debug", skip_all)]
    fn convert_to_offsets(
        &self,
        start: RawAddress,
        values: Vec<((Address, NonZero<TypeSize>), &MemObject)>,
    ) -> Vec<((PointerOffset, NonZero<TypeSize>), MemObject)> {
        values
            .into_iter()
            .map(|((addr, size), obj)| {
                let offset: PointerOffset = byte_offset_from(addr, start) as PointerOffset;
                ((offset, size), *obj)
            })
            .collect()
    }
}

enum PlaceMdRelevance {
    Container(bool),
    Wrapped,
    WrappedOwned,
}

impl RawPointerVariableState {
    pub(super) fn get_place<'a, 'b>(&'a self, place: Place, usage: PlaceUsage) -> PlaceValue {
        match usage {
            PlaceUsage::Copy => self
                .opt_copied_md_wrapped_owned(&place)
                .map_or(PlaceValue::NonRelevant {}, |addr| {
                    PlaceValue::AccessedMdWrapped { addr }
                }),
            PlaceUsage::Ref => self
                .opt_referenced_md_wrapped(&place)
                .map_or(PlaceValue::NonRelevant {}, |addr| {
                    PlaceValue::AccessedMdWrapped { addr }
                }),
            PlaceUsage::Move => self
                .opt_moved_md_container(&place)
                .map_or(PlaceValue::NonRelevant {}, |region| {
                    PlaceValue::ToCarryMdContainer { mem_region: region }
                }),
            PlaceUsage::Drop => self
                .opt_dropped_md_wrapped(&place)
                .map_or(PlaceValue::NonRelevant {}, |addr| {
                    PlaceValue::ToDropMaybeMdWrapped { wrapped_addr: addr }
                }),
            PlaceUsage::Mark => self
                .opt_marked_md(&place)
                .map_or(PlaceValue::NonRelevant {}, |region| {
                    PlaceValue::LifetimeMarkedMd { mem_region: region }
                }),
            PlaceUsage::Write => PlaceValue::LazyDestination(WritablePlace {
                addr: place.metadata().address(),
                type_id: place.metadata().type_id(),
                pointer_type_id: None,
            }),
        }
    }

    fn opt_copied_md_wrapped_owned(&self, place: &Place) -> Option<RawAddress> {
        let relevance = self.inspect_place_for_wrapped(place)?;
        match relevance {
            PlaceMdRelevance::WrappedOwned => Some(place.metadata().address()),
            _ => None,
        }
    }

    fn opt_moved_md_container(&self, place: &Place) -> Option<MemoryRegion> {
        place
            .metadata()
            .type_id()
            .filter(|type_id| self.type_manager.is_md_container_type2(*type_id))
            .map(|type_id| MemoryRegion {
                addr: place.metadata().address(),
                size: self.get_type_size(type_id),
            })
    }

    fn opt_referenced_md_wrapped(&self, place: &Place) -> Option<RawAddress> {
        let relevance = self.inspect_place_for_wrapped(place)?;
        match relevance {
            PlaceMdRelevance::Wrapped | PlaceMdRelevance::WrappedOwned => {
                Some(place.metadata().address())
            }
            _ => None,
        }
    }

    fn opt_dropped_md_wrapped(&self, _place: &Place) -> Option<RawAddress> {
        // `ManuallyDrop::drop` directly calls `drop_in_place` so it is going to be a pointer-based access
        None
    }

    fn opt_marked_md(&self, place: &Place) -> Option<MemoryRegion> {
        place
            .metadata()
            .type_id()
            .filter(|type_id| self.type_manager.is_md_container_type2(*type_id))
            .map(|type_id| MemoryRegion {
                addr: place.metadata().address(),
                size: self.get_type_size(type_id),
            })
    }

    fn inspect_place_for_wrapped(&self, place: &Place) -> Option<PlaceMdRelevance> {
        let base_state = |type_id| {
            let Some(type_id) = type_id else {
                return None;
            };
            if self.type_manager.is_md_container_type2(type_id) {
                Some(PlaceMdRelevance::Container(
                    self.type_manager.is_md_type2(type_id),
                ))
            } else {
                None
            }
        };

        let projs = place.projections();
        let mut projs_and_meta = projs.iter().zip(place.projs_metadata());
        let mut current = match projs {
            [] => return None,
            [Projection::Deref, ..] => base_state(projs_and_meta.next().unwrap().1.type_id())?,
            _ => base_state(place.base().metadata().type_id())?,
        };

        for (proj, meta) in projs_and_meta {
            current = match (proj, &current) {
                (_, PlaceMdRelevance::Container(false)) => base_state(meta.type_id())?,
                (Projection::Field(..), PlaceMdRelevance::Container(true)) => {
                    PlaceMdRelevance::Wrapped
                }
                (_, PlaceMdRelevance::Container(true)) => unreachable!(),
                (_, PlaceMdRelevance::Wrapped) => PlaceMdRelevance::WrappedOwned,
                (_, PlaceMdRelevance::WrappedOwned) => break,
            };
        }

        Some(current)
    }

    pub(super) fn get_deref_of_ptr<'a>(
        &self,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        usage: PlaceUsage,
    ) -> PlaceValue {
        match usage {
            PlaceUsage::Copy => PlaceValue::NonRelevant {},
            PlaceUsage::Write => PlaceValue::LazyDestination(WritablePlace {
                addr: conc_ptr,
                type_id: None,
                pointer_type_id: Some(ptr_type_id),
            }),
            PlaceUsage::Drop => PlaceValue::ToDropMaybeMdWrapped {
                wrapped_addr: conc_ptr,
            },
            PlaceUsage::Move | PlaceUsage::Ref | PlaceUsage::Mark => unimplemented!(),
        }
    }
}
