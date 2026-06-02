use core::iter;

use std::{cell::RefCell, num::NonZero, ops::DerefMut, rc::Rc};

use derive_more as dm;

use common::{log_info, log_warn, type_info::TypeInfo};

use crate::{
    abs::{PlaceUsage, PointerOffset, RawAddress, TypeId, TypeSize, place::HasMetadata},
    backends::mdsan::{
        MdMemoryState, MdSanPlaceValue,
        state::{
            EvaluatedPlace, MdState, MemoryRegion, PlaceValue, PlaceValue2, WritablePlace,
            get_mem_region,
        },
    },
    type_info::{TypeInfoExt, TypeLayoutResolver, TypeLayoutResolverExt},
    utils::{InPlaceSelfHierarchical, alias::RRef, byte_offset_from, meta},
};

use super::backend;

use backend::{
    MdSanTypeManager,
    place::{LocalWithMetadata, PlaceWithMetadata, Projection},
};

use super::Value;

mod memory;
use memory::*;

type Local = LocalWithMetadata;
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

    #[inline]
    fn get_type(&self, type_id: TypeId) -> &'static TypeInfo {
        self.type_manager.get_type(&type_id)
    }

    fn get_type_size(&self, place_val: &PlaceValue) -> TypeSize {
        todo!()
    }
}

type MemObject = MdState;

// impl GenericVariablesState for RawPointerVariableState {
//     type PlaceInfo = Place;
//     type PlaceValue = MdSanPlaceValue;
//     type Value = Value;

//     fn id(&self) -> usize {
//         // FIXME
//         0
//     }

//     // #[tracing::instrument(level = "debug", skip(self))]
//     fn ref_place(&self, place: Place, usage: PlaceUsage) -> Self::PlaceValue {
//         self.get_place(place, usage)
//     }

//     // #[tracing::instrument(level = "debug", skip(self))]
//     fn ref_place_by_ptr(
//         &self,
//         ptr: Self::Value,
//         conc_ptr: RawAddress,
//         ptr_type_id: TypeId,
//         usage: PlaceUsage,
//     ) -> Self::PlaceValue {
//         self.get_deref_of_ptr(ptr, conc_ptr, ptr_type_id, usage)
//     }

//     // #[tracing::instrument(level = "debug", skip(self))]
//     fn copy_place(&self, place: &Self::PlaceValue) -> Self::Value {
//         let place = match place {
//             PlaceValue::Evaluated(place) => place,
//             PlaceValue::Unevaluated(info) => &self.evaluate_place(info),
//         };

//         match place {
//             EvaluatedPlace::NonMdRelevant { .. } | EvaluatedPlace::MdWrapped { .. } => {
//                 Value::non_rel()
//             }
//             EvaluatedPlace::MdContainer { mem_region, .. } => self.copy_region(*mem_region),
//         }
//     }

//     // #[tracing::instrument(level = "debug", skip(self))]
//     fn take_place(&mut self, place: &Self::PlaceValue) -> Self::Value {
//         let place = match place {
//             PlaceValue::Evaluated(place) => place,
//             PlaceValue::Unevaluated(info) => &self.evaluate_place(info),
//         };

//         match place {
//             EvaluatedPlace::NonMdRelevant { .. } | EvaluatedPlace::MdWrapped { .. } => {
//                 Value::non_rel()
//             }
//             EvaluatedPlace::MdContainer { mem_region, .. } => self.take_region(*mem_region),
//         }
//     }

//     // #[tracing::instrument(level = "debug", skip(self))]
//     fn set_place(&mut self, place: &Self::PlaceValue, value: Self::Value) {
//         if !value.is_rel() {
//             return;
//         }

//         let place = match place {
//             PlaceValue::Evaluated(place) => place,
//             PlaceValue::Unevaluated(info) => &self.evaluate_place(info),
//         };

//         match place {
//             EvaluatedPlace::NonMdRelevant { .. } | EvaluatedPlace::MdWrapped { .. } => {
//                 unreachable!()
//             }
//             EvaluatedPlace::MdContainer { mem_region, .. } => self.set_region(*mem_region, value),
//         }
//     }

//     // #[tracing::instrument(level = "debug", skip(self))]
//     fn drop_place(&mut self, place: &Self::PlaceValue) {
//         let place = match place {
//             PlaceValue::Evaluated(place) => place,
//             PlaceValue::Unevaluated(info) => &self.evaluate_place(info),
//         };

//         match place {
//             EvaluatedPlace::NonMdRelevant { .. } | EvaluatedPlace::MdWrapped { .. } => {
//                 // TODO
//                 return;
//             }
//             EvaluatedPlace::MdContainer { mem_region, .. } => self.drop_region(*mem_region),
//         }
//     }
// }

impl MdMemoryState for RawPointerVariableState {
    type PlaceInfo = Place;
    type PlaceValue = PlaceValue2;
    type ToInspectPlaceValue = RawAddress;
    type ToTakePlaceValue = MemoryRegion;
    type ToSetPlaceValue = WritablePlace;
    type ToUpdatePlaceValue = RawAddress;
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
        self.take_region(*place)
    }

    fn set_place(&mut self, place: &Self::ToSetPlaceValue, value: Self::Value) {
        if !value.is_rel() {
            return;
        }
        self.set_region(place.memory_region(&self.type_manager), value)
    }

    fn update_place(
        &mut self,
        place: &Self::ToUpdatePlaceValue,
        value: Self::ValueForAddress,
    ) -> bool {
        self.update_addr(*place, value)
    }

    fn erase_place(&mut self, place: &Self::ToErasePlaceValue) {
        self.erase_region(*place)
    }
}

impl RawPointerVariableState {
    #[tracing::instrument(level = "debug", skip(self))]
    fn peek_addr(&self, addr: &RawAddress) -> Option<&MemObject> {
        self.memory.get_containing(*addr)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn update_addr(&mut self, addr: RawAddress, value: MemObject) -> bool {
        self.memory.update_containing(addr, value)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn erase_region(&mut self, region: MemoryRegion) {
        let addr = region.addr;
        let size = region.size;
        self.memory.erase_objects(addr, size);
    }
}

impl RawPointerVariableState {
    #[tracing::instrument(level = "debug", skip(self))]
    fn copy_region(&self, region: MemoryRegion) -> Value {
        let addr = region.addr;
        let size = region.size;

        let values = self.memory.read_objects(addr, size);
        let values = self.convert_to_offsets(addr, values);
        Value::new(values)
    }

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

impl InPlaceSelfHierarchical for RawPointerVariableState {
    fn add_layer(&mut self) {
        // Nothing to do.
    }

    fn drop_layer(&mut self) -> Option<Self> {
        None
    }
}
enum PlaceMdRelevance {
    Container(bool),
    Wrapped,
    WrappedOwned,
}

impl RawPointerVariableState {
    pub(super) fn get_place<'a, 'b>(&'a self, place: Place, usage: PlaceUsage) -> PlaceValue2 {
        match usage {
            PlaceUsage::Copy => self
                .opt_copied_md_wrapped_owned(&place)
                .map_or(PlaceValue2::NonRelevant {}, |addr| {
                    PlaceValue2::AccessedMdWrapped { addr }
                }),
            PlaceUsage::Ref => self
                .opt_referenced_md_wrapped(&place)
                .map_or(PlaceValue2::NonRelevant {}, |addr| {
                    PlaceValue2::AccessedMdWrapped { addr }
                }),
            PlaceUsage::Move => self
                .opt_moved_md_container(&place)
                .map_or(PlaceValue2::NonRelevant {}, |region| {
                    PlaceValue2::ToCarryMdContainer { mem_region: region }
                }),
            PlaceUsage::Drop => self
                .opt_dropped_md_wrapped(&place)
                .map_or(PlaceValue2::NonRelevant {}, |addr| {
                    PlaceValue2::ToDropMdWrapped { wrapped_addr: addr }
                }),
            PlaceUsage::Mark => self
                .opt_marked_md(&place)
                .map_or(PlaceValue2::NonRelevant {}, |region| {
                    PlaceValue2::LifetimeMarkedMd { mem_region: region }
                }),
            PlaceUsage::Write => PlaceValue2::LazyDestination(WritablePlace {
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
            .is_some_and(|id| self.type_manager.is_md_container_type2(id))
            .then(|| get_mem_region(place.metadata(), &self.type_manager))
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

    fn opt_dropped_md_wrapped(&self, place: &Place) -> Option<RawAddress> {
        // `ManuallyDrop::drop` directly calls `drop_in_place` so it is going to be a pointer-based access
        None
    }

    fn opt_marked_md(&self, place: &Place) -> Option<MemoryRegion> {
        place
            .metadata()
            .type_id()
            .is_some_and(|id| self.type_manager.is_md_container_type2(id))
            .then(|| get_mem_region(place.metadata(), &self.type_manager))
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
    ) -> PlaceValue2 {
        match usage {
            PlaceUsage::Copy => PlaceValue2::NonRelevant {},
            PlaceUsage::Write => PlaceValue2::LazyDestination(WritablePlace {
                addr: conc_ptr,
                type_id: None,
                pointer_type_id: Some(ptr_type_id),
            }),
            PlaceUsage::Drop => PlaceValue2::ToDropMdWrapped {
                wrapped_addr: conc_ptr,
            },
            PlaceUsage::Move => unimplemented!(),
            PlaceUsage::Ref => unimplemented!(),
            PlaceUsage::Mark => unimplemented!(),
        }
    }

    fn evaluate_place(&self, place: &Place) -> EvaluatedPlace {
        super::evaluate_place(place, &self.type_manager)
    }

    // pub(super) fn get_deref_of_ptr<'a>(
    //     &self,
    //     ptr_val: Value,
    //     conc_ptr: RawAddress,
    //     ptr_type_id: TypeId,
    //     usage: PlaceUsage,
    // ) -> PlaceValue {
    //     match usage {
    //         PlaceUsage::Write | PlaceUsage::Ref => {
    //             // PlaceValue::Unevaluated(todo!());
    //             // TODO
    //             PlaceValue::Evaluated(EvaluatedPlace::NonMdRelevant {
    //                 place_meta: {
    //                     let mut meta = backend::place::PlaceMetadata::default();
    //                     meta.set_address(conc_ptr);
    //                     // TODO
    //                     meta
    //                 },
    //             })
    //         }
    //         PlaceUsage::Read => {
    //             let pointee = self.type_manager.get_pointee_ty(&ptr_type_id).unwrap();
    //             if self.type_manager.is_md_container_type2(pointee) {
    //                 PlaceValue::Evaluated(EvaluatedPlace::MdContainer {
    //                     mem_region: MemoryRegion {
    //                         addr: conc_ptr,
    //                         size: self.type_manager.get_size(&pointee).unwrap(), // TODO
    //                     },
    //                     type_id: pointee,
    //                 })
    //             } else if self.has_label(conc_ptr) {
    //                 // FIXME: May be optimized by just checking the type
    //                 PlaceValue::Evaluated(EvaluatedPlace::MdWrapped {
    //                     md_region: todo!(),
    //                     md_type_id: pointee,
    //                 })
    //             } else {
    //                 PlaceValue::Evaluated(EvaluatedPlace::NonMdRelevant {
    //                     place_meta: {
    //                         let mut meta = backend::place::PlaceMetadata::default();
    //                         meta.set_address(conc_ptr);
    //                         // TODO
    //                         meta
    //                     },
    //                 })
    //             }
    //         }
    //     }
    // }

    // fn has_label(&self, addr: RawAddress) -> bool {
    //     self.memory.get_containing(addr)
    // }
}
