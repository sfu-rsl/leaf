mod pointer_based;

use core::num::NonZero;

use common::{log_debug, log_info};
use derive_more as dm;

use crate::abs::place::HasMetadata;
use crate::abs::{PointerOffset, RawAddress, TypeId, TypeSize};

use crate::backends::mdsan::{MdMemoryState, MdSanPlaceValue, MdTypeProvider, TypeDatabase};
use crate::pri::fluent::backend::MemoryHandler;

pub(super) use pointer_based::RawPointerVariableState;

use super::alias::backend;
use backend::{MdSanBackend, MdSanPlaceInfo, MdSanVariablesState};

pub(crate) struct MdSanMemoryHandler<'s> {
    vars_state: &'s mut MdSanVariablesState,
}

impl<'s> MdSanMemoryHandler<'s> {
    pub(super) fn new(backend: &'s mut MdSanBackend) -> Self {
        Self {
            vars_state: &mut backend.vars_state,
        }
    }
}

impl<'s> MemoryHandler for MdSanMemoryHandler<'s> {
    type Place = PlaceValue2;

    fn mark_live(self, _place: Self::Place) {
        // Nothing to do for now.
    }

    fn mark_dead(self, place: Self::Place) {
        match place {
            PlaceValue2::LifetimeMarkedMd { mem_region } => {
                self.vars_state.erase_place(&mem_region)
            }
            PlaceValue2::NonRelevant {} => {}
            PlaceValue2::AccessedMdWrapped { .. }
            | PlaceValue2::ToCarryMdContainer { .. }
            | PlaceValue2::LazyDestination(..)
            | PlaceValue2::ToDropMdWrapped { .. } => unreachable!(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub(super) enum MdState {
    Alive,
    Dropped,
}

pub(super) type Label = MdState;

#[derive(Clone, Debug)]
pub(crate) struct Value {
    labels: Vec<((PointerOffset, NonZero<TypeSize>), Label)>,
    pub(crate) referenced_md: Option<PlaceValue>,
}

impl Value {
    pub fn new(labels: Vec<((PointerOffset, NonZero<TypeSize>), Label)>) -> Self {
        Self {
            labels,
            referenced_md: None,
        }
    }

    pub fn non_rel() -> Self {
        Self {
            labels: Vec::new(),
            referenced_md: None,
        }
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
            referenced_md: None,
        }
    }

    pub fn ref_place(mut self, place: PlaceValue) -> Self {
        self.referenced_md = Some(place);
        self
    }

    pub fn dropped(size: TypeSize) -> Self {
        Self {
            labels: vec![(
                (0 as PointerOffset, size.try_into().unwrap()),
                MdState::Dropped,
            )],
            referenced_md: None,
        }
    }

    pub fn is_dropped(&self) -> bool {
        todo!()
    }

    pub fn labels_with_base(
        self,
        base: PointerOffset,
    ) -> Vec<((PointerOffset, NonZero<TypeSize>), Label)> {
        let mut labels = self.labels;
        labels.iter_mut().for_each(|((offset, size), _)| {
            *offset += base;
        });
        labels
    }
}

enum PlaceKind {
    NonMdRelevant,
    MdContainer,
    MdWrapped,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MemoryRegion {
    pub addr: RawAddress,
    pub size: TypeSize,
}

#[derive(Clone, Debug, dm::From, dm::Display)]
#[display("{_variant}")]
pub(crate) enum PlaceValue {
    Unevaluated(MdSanPlaceInfo),
    #[from]
    Evaluated(EvaluatedPlace),
}

#[derive(Clone, Debug, dm::Display)]
#[display("{_variant}")]
pub(crate) enum EvaluatedPlace {
    #[display("NonMdRelevant")]
    NonMdRelevant {
        place_meta: backend::place::PlaceMetadata,
    },
    #[display("MdContainer")]
    MdContainer {
        mem_region: MemoryRegion,
        type_id: TypeId,
    },
    #[display("MdWrapped")]
    MdWrapped {
        md_region: MemoryRegion,
        md_type_id: TypeId,
    },
}

impl PlaceValue {
    pub fn evaluate(&mut self, type_manager: &(impl TypeDatabase + ?Sized)) {
        if let PlaceValue::Unevaluated(place) = self {
            *self = evaluate_place(place, type_manager).into();
        }
    }

    pub fn is_md(&self, type_manager: &(impl MdTypeProvider + ?Sized)) -> bool {
        let type_id = match self {
            PlaceValue::Unevaluated(place) => place.metadata().type_id(),
            PlaceValue::Evaluated(EvaluatedPlace::MdContainer { type_id, .. }) => Some(*type_id),
            PlaceValue::Evaluated(
                EvaluatedPlace::NonMdRelevant { .. } | EvaluatedPlace::MdWrapped { .. },
            ) => None,
        };
        type_id.is_some_and(|id| type_manager.is_md_type2(id))
    }

    pub fn is_md_wrapped_value(&self) -> bool {
        match self {
            PlaceValue::Unevaluated(place_with_metadata) => todo!(),
            PlaceValue::Evaluated(evaluated_place) => match evaluated_place {
                EvaluatedPlace::NonMdRelevant { .. } => false,
                EvaluatedPlace::MdContainer { .. } => false,
                EvaluatedPlace::MdWrapped { .. } => true,
            },
        }
    }

    pub fn md(&self) -> Option<(MemoryRegion, TypeId)> {
        // TODO
        match self {
            PlaceValue::Unevaluated(_) => None,
            PlaceValue::Evaluated(evaluated_place) => match evaluated_place {
                EvaluatedPlace::NonMdRelevant { .. } => None,
                EvaluatedPlace::MdContainer {
                    mem_region,
                    type_id,
                } => Some((*mem_region, *type_id)),
                EvaluatedPlace::MdWrapped {
                    md_region,
                    md_type_id,
                } => Some((*md_region, *md_type_id)),
            },
        }
    }

    pub fn type_id(&self) -> TypeId {
        match self {
            PlaceValue::Unevaluated(place_with_metadata) => {
                place_with_metadata.metadata().unwrap_type_id()
            }
            PlaceValue::Evaluated(evaluated_place) => match evaluated_place {
                EvaluatedPlace::NonMdRelevant { .. } => todo!(),
                EvaluatedPlace::MdContainer { type_id, .. } => *type_id,
                EvaluatedPlace::MdWrapped { .. } => todo!(),
            },
        }
    }

    pub fn size(&self) -> TypeSize {
        match self {
            PlaceValue::Unevaluated(place_with_metadata) => todo!(),
            PlaceValue::Evaluated(evaluated_place) => match evaluated_place {
                EvaluatedPlace::NonMdRelevant { .. } => todo!(),
                EvaluatedPlace::MdContainer { mem_region, .. } => mem_region.size,
                EvaluatedPlace::MdWrapped { md_region, .. } => todo!(),
            },
        }
    }

    pub fn address(&self) -> RawAddress {
        match self {
            PlaceValue::Unevaluated(place_with_metadata) => {
                place_with_metadata.metadata().address()
            }
            PlaceValue::Evaluated(evaluated_place) => match evaluated_place {
                EvaluatedPlace::NonMdRelevant { place_meta } => place_meta.address(),
                EvaluatedPlace::MdContainer { mem_region, .. } => mem_region.addr,
                EvaluatedPlace::MdWrapped { md_region, .. } => todo!(),
            },
        }
    }
}

fn evaluate_place(
    place: &MdSanPlaceInfo,
    type_manager: &(impl TypeDatabase + ?Sized),
) -> EvaluatedPlace {
    let mut result = EvaluatedPlace::NonMdRelevant {
        place_meta: place.metadata().clone(),
    };
    let mut last_is_md = false;

    for metadata in core::iter::once(place.base().metadata()).chain(place.projs_metadata()) {
        match result {
            EvaluatedPlace::NonMdRelevant { .. } if is_md_container2(metadata, type_manager) => {
                metadata.type_id().unwrap_or_else(|| {
                    panic!("Projection without type ID after MD container{:?}", place)
                });
                result = EvaluatedPlace::MdContainer {
                    mem_region: get_mem_region(metadata, type_manager),
                    type_id: metadata.unwrap_type_id(),
                };
                last_is_md = is_md2(metadata, type_manager);
            }
            EvaluatedPlace::NonMdRelevant { .. } => {}
            EvaluatedPlace::MdContainer {
                mem_region,
                type_id,
            } if last_is_md => {
                result = EvaluatedPlace::MdWrapped {
                    md_region: mem_region,
                    md_type_id: type_id,
                };
                last_is_md = false;
                return result;
            }
            EvaluatedPlace::MdContainer { .. } if is_md_container2(metadata, type_manager) => {
                metadata.type_id().unwrap_or_else(|| {
                    panic!("Projection without type ID after MD container{:?}", place)
                });
                result = EvaluatedPlace::MdContainer {
                    mem_region: get_mem_region(metadata, type_manager),
                    type_id: metadata.type_id().unwrap_or_else(|| {
                        panic!("Projection without type ID after MD container{:?}", place)
                    }),
                };
                last_is_md = is_md2(metadata, type_manager);
            }
            EvaluatedPlace::MdContainer { .. } => {
                result = EvaluatedPlace::NonMdRelevant {
                    place_meta: metadata.clone(),
                };
                last_is_md = false;
                // Not possible to have another MD container, as deref comes as the first projection.
                return result;
            }
            EvaluatedPlace::MdWrapped { .. } => {
                unreachable!()
            }
        }
    }

    result
}

fn is_md_container2(
    metadata: &backend::place::PlaceMetadata,
    type_manager: &(impl backend::MdTypeProvider + ?Sized),
) -> bool {
    if let Some(type_id) = metadata.type_id() {
        type_manager.is_md_container_type2(type_id)
    } else {
        false
    }
}

fn is_md2(
    metadata: &backend::place::PlaceMetadata,
    type_manager: &(impl backend::MdTypeProvider + ?Sized),
) -> bool {
    if let Some(type_id) = metadata.type_id() {
        type_manager.is_md_type2(type_id)
    } else {
        false
    }
}

fn get_mem_region(
    metadata: &backend::place::PlaceMetadata,
    type_manager: &(impl backend::TypeDatabase + ?Sized),
) -> MemoryRegion {
    MemoryRegion {
        addr: metadata.address(),
        size: type_manager.get_size(&metadata.unwrap_type_id()).unwrap(), // TODO
    }
}

#[derive(Clone, Debug)]
pub(crate) struct WritablePlace {
    pub(crate) addr: RawAddress,
    pub(crate) type_id: Option<TypeId>,
    pub(crate) pointer_type_id: Option<TypeId>,
}

impl WritablePlace {
    pub fn type_id(&self) -> TypeId {
        self.type_id.expect("Type ID should be set")
    }

    pub fn type_id2(&self, type_manager: &(impl TypeDatabase + ?Sized)) -> TypeId {
        self.type_id.unwrap_or_else(|| {
            type_manager
                .get_pointee_ty(
                    self.pointer_type_id
                        .as_ref()
                        .expect("Expected pointer type ID"),
                )
                .unwrap()
        })
    }

    pub fn is_md(&self, type_manager: &(impl MdTypeProvider + ?Sized)) -> bool {
        type_manager.is_md_type2(self.type_id())
    }

    pub fn size(&self, type_manager: &(impl TypeDatabase + ?Sized)) -> TypeSize {
        type_manager.get_size(&self.type_id2(type_manager)).unwrap()
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
pub(crate) enum PlaceValue2 {
    #[display("AccessedMdWrapped({addr:p})")]
    AccessedMdWrapped { addr: RawAddress },
    #[display("ToCarryMdContainer({mem_region:?})")]
    ToCarryMdContainer { mem_region: MemoryRegion },
    #[display("LazyDestination({_0:?})")]
    LazyDestination(WritablePlace),
    #[display("LifetimeMarkedMd({mem_region:?})")]
    LifetimeMarkedMd { mem_region: MemoryRegion },
    #[display("ToDropMdWrapped({wrapped_addr:p})")]
    ToDropMdWrapped { wrapped_addr: RawAddress },
    #[display("NonRelevant")]
    NonRelevant {},
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
        match place {
            MdSanPlaceValue::AccessedMdWrapped { addr } => {
                if self
                    .vars_state
                    .peek_place(addr)
                    .is_some_and(|s| matches!(s, MdState::Dropped))
                {
                    common::log_error!("Accessing a dropped MD wrapper at {addr:p}");
                    panic!("Accessing a dropped MD wrapper at {addr:p}");
                }
            }
            MdSanPlaceValue::NonRelevant {} => {}
            MdSanPlaceValue::ToCarryMdContainer { .. }
            | MdSanPlaceValue::LazyDestination(WritablePlace { .. })
            | MdSanPlaceValue::LifetimeMarkedMd { .. }
            | MdSanPlaceValue::ToDropMdWrapped { .. } => {
                if cfg!(debug_assertions) {
                    common::log_warn!(
                        "Inspecting a place for access that is not MD wrapped: {place:?}"
                    );
                }
            }
        }
    }
}
