mod pointer_based;
mod utils;

use derive_more as dm;

use crate::abs::backend::MemoryHandler;

pub(super) use pointer_based::RawPointerVariableState;
pub(super) use pointer_based::sym_place::strategies::make_sym_place_handler;

use crate::backends::basic as backend;
use backend::{
    BasicBackend, CallStackInfo, ConcreteValueRef, PlaceValueRef, SymValueRef, VariablesState,
    concrete::ConcolicValueObtainer,
};

#[derive(Debug, dm::Deref)]
pub(super) struct SymPlaceSymEntity {
    #[deref]
    value: SymValueRef,
    is_index: bool,
}

pub(super) trait SymPlaceHandler {
    type SymEntity = SymPlaceSymEntity;
    type ConcEntity = ConcreteValueRef;
    type Entity: From<Self::SymEntity> + From<Self::ConcEntity>;

    fn handle<'a>(
        &mut self,
        sym_entity: Self::SymEntity,
        get_conc: Box<ConcolicValueObtainer<'a, Self::ConcEntity>>,
    ) -> Self::Entity;
}

pub(crate) struct BasicMemoryHandler<'s> {
    vars_state: &'s mut dyn VariablesState,
}

impl<'s> BasicMemoryHandler<'s> {
    pub(super) fn new(backend: &'s mut BasicBackend) -> Self {
        Self {
            vars_state: backend.call_stack_manager.top(),
        }
    }
}

impl<'s> MemoryHandler for BasicMemoryHandler<'s> {
    type Place = PlaceValueRef;

    fn mark_dead(self, place: Self::Place) {
        self.vars_state.drop_place(&place);
    }
}
