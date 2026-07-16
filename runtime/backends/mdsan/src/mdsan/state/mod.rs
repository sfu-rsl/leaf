mod lifetime;
mod pointer_based;

use core::num::NonZero;

use common::log_error;
use derive_more as dm;

use crate::abs::{PointerOffset, RawAddress, TypeId, TypeSize};

use super::alias::backend;
use backend::{MdMemoryState, MdSanPlaceValue, MdSanVariablesState, TypeDatabase};

pub(super) use lifetime::MdSanLifetimeHandler;
pub(super) use pointer_based::RawPointerVariableState;

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub(crate) enum MdState {
    Alive,
    Dropped,
}

#[derive(Clone, Debug)]
pub(crate) struct Value {
    labels: Vec<((PointerOffset, NonZero<TypeSize>), MdState)>,
}

impl Value {
    pub fn new(labels: Vec<((PointerOffset, NonZero<TypeSize>), MdState)>) -> Self {
        Self { labels }
    }

    pub fn non_rel() -> Self {
        Self { labels: Vec::new() }
    }

    pub fn is_rel(&self) -> bool {
        !self.labels.is_empty()
    }

    pub fn fresh(size: TypeSize) -> Self {
        Self {
            labels: vec![(
                (0 as PointerOffset, size.try_into().unwrap()),
                MdState::Alive,
            )],
        }
    }

    pub fn labels_with_base(
        self,
        base: PointerOffset,
    ) -> Vec<((PointerOffset, NonZero<TypeSize>), MdState)> {
        let mut labels = self.labels;
        labels.iter_mut().for_each(|((offset, _size), _)| {
            *offset += base;
        });
        labels
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MemoryRegion {
    pub addr: RawAddress,
    pub size: TypeSize,
}

#[derive(Clone, Debug)]
pub(crate) struct WritablePlace {
    addr: RawAddress,
    type_id: DirectOrPointerTypeId,
}

#[derive(Clone, Debug)]
enum DirectOrPointerTypeId {
    Direct(TypeId),
    Pointer(TypeId),
}

impl WritablePlace {
    fn type_id(&self, type_manager: &(impl TypeDatabase + ?Sized)) -> TypeId {
        match self.type_id {
            DirectOrPointerTypeId::Direct(type_id) => type_id,
            DirectOrPointerTypeId::Pointer(ptr_type_id) => {
                type_manager.get_pointee_ty(&ptr_type_id).unwrap()
            }
        }
    }

    pub fn is_md(&self, type_manager: &(impl TypeDatabase + ?Sized)) -> bool {
        type_manager.is_md_type(&self.type_id(type_manager))
    }

    pub fn size(&self, type_manager: &(impl TypeDatabase + ?Sized)) -> TypeSize {
        type_manager.get_size(&self.type_id(type_manager)).unwrap()
    }

    pub fn memory_region(&self, type_manager: &(impl TypeDatabase + ?Sized)) -> MemoryRegion {
        MemoryRegion {
            addr: self.addr,
            size: self.size(type_manager),
        }
    }
}

#[derive(Clone, Debug, dm::Display)]
#[display("{_variant}")]
pub(crate) enum PlaceValue {
    #[display("AccessedMdWrapped({addr:p})")]
    AccessedMdWrapped { addr: RawAddress },
    #[display("ToCarryMdContainer({mem_region:?})")]
    ToCarryMdContainer { mem_region: MemoryRegion },
    #[display("LazyDestination({_0:?})")]
    LazyDestination(WritablePlace),
    #[display("LifetimeMarkedMd({mem_region:?})")]
    LifetimeMarkedMd { mem_region: MemoryRegion },
    #[display("ToDropMdWrapped({wrapped_addr:p})")]
    ToDropMaybeMdWrapped { wrapped_addr: RawAddress },
    #[display("NonRelevant")]
    NonRelevant {},
}

impl PlaceValue {
    pub(super) fn is_md(&self, type_manager: &(impl TypeDatabase + ?Sized)) -> Option<bool> {
        match self {
            PlaceValue::AccessedMdWrapped { .. } => Some(false),
            PlaceValue::LifetimeMarkedMd { .. } => Some(true),
            PlaceValue::ToDropMaybeMdWrapped { .. } => None,
            PlaceValue::ToCarryMdContainer { .. } => None,
            PlaceValue::LazyDestination(writable_place) => Some(writable_place.is_md(type_manager)),
            PlaceValue::NonRelevant {} => Some(false),
        }
    }

    pub(super) fn type_id(&self, type_manager: &(impl TypeDatabase + ?Sized)) -> Option<TypeId> {
        match self {
            PlaceValue::AccessedMdWrapped { .. } => None,
            PlaceValue::LifetimeMarkedMd { .. } => None,
            PlaceValue::ToDropMaybeMdWrapped { .. } => None,
            PlaceValue::ToCarryMdContainer { .. } => None,
            PlaceValue::LazyDestination(writable_place) => {
                Some(writable_place.type_id(type_manager))
            }
            PlaceValue::NonRelevant {} => None,
        }
    }

    pub(crate) fn address(&self) -> Option<RawAddress> {
        match self {
            PlaceValue::AccessedMdWrapped { addr } => Some(*addr),
            PlaceValue::ToCarryMdContainer { mem_region } => Some(mem_region.addr),
            PlaceValue::LazyDestination(writable_place) => Some(writable_place.addr),
            PlaceValue::LifetimeMarkedMd { mem_region } => Some(mem_region.addr),
            PlaceValue::ToDropMaybeMdWrapped { wrapped_addr } => Some(*wrapped_addr),
            PlaceValue::NonRelevant {} => None,
        }
    }

    pub(crate) fn project_field(
        &self,
        offset: PointerOffset,
        get_size: impl FnOnce() -> TypeSize,
        get_type_id: impl FnOnce() -> TypeId,
    ) -> Self {
        let offset = offset.try_into().unwrap();
        match self {
            PlaceValue::AccessedMdWrapped { addr } => PlaceValue::AccessedMdWrapped {
                addr: (*addr).wrapping_add(offset),
            },
            PlaceValue::ToCarryMdContainer { mem_region } => PlaceValue::ToCarryMdContainer {
                mem_region: MemoryRegion {
                    addr: mem_region.addr.wrapping_add(offset),
                    size: get_size(),
                },
            },
            PlaceValue::LazyDestination(writable_place) => {
                PlaceValue::LazyDestination(WritablePlace {
                    addr: writable_place.addr.wrapping_add(offset),
                    type_id: DirectOrPointerTypeId::Direct(get_type_id()),
                })
            }
            PlaceValue::LifetimeMarkedMd { mem_region: _ } => PlaceValue::NonRelevant {}, // FIXME: MD<MD<T>>
            PlaceValue::ToDropMaybeMdWrapped { wrapped_addr } => PlaceValue::ToDropMaybeMdWrapped {
                wrapped_addr: (*wrapped_addr).wrapping_add(offset),
            },
            PlaceValue::NonRelevant {} => PlaceValue::NonRelevant {},
        }
    }
}

pub(crate) struct DefaultPlaceInspector<'a> {
    vars_state: &'a MdSanVariablesState,
}

impl<'a> DefaultPlaceInspector<'a> {
    pub fn new(vars_state: &'a MdSanVariablesState) -> Self {
        Self { vars_state }
    }
}

impl super::PlaceInspector for DefaultPlaceInspector<'_> {
    fn inspect_place_for_access(&self, place: &MdSanPlaceValue) {
        if self
            .vars_state
            .peek_place(place)
            .is_some_and(|s| matches!(s, MdState::Dropped))
        {
            let addr = place.address().unwrap();
            log_error!("Accessing a dropped MD wrapper at {addr:p}");
            panic!("Accessing a dropped MD wrapper at {addr:p}");
        }
    }
}
