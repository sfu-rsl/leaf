mod pointer_based;

use crate::abs::backend::MemoryHandler;

pub(super) use pointer_based::RawPointerVariableState;
pub(super) use pointer_based::sym_place::strategies::make_sym_place_handler;

use crate::backends::basic as backend;
use backend::{
    BasicBackend, CallStackInfo, ConcreteValueRef, PlaceValueRef, SymValueRef, VariablesState,
};

pub(super) trait SymPlaceHandler {
    type SymEntity = SymValueRef;
    type ConcEntity = ConcreteValueRef;
    type Entity: From<Self::SymEntity> + From<Self::ConcEntity>;

    fn handle<'a>(
        &mut self,
        sym_entity: Self::SymEntity,
        get_conc: Box<dyn FnOnce(&Self::SymEntity) -> Self::ConcEntity + 'a>,
    ) -> Self::Entity;
}

impl<SE, CE, E: From<SE> + From<CE>> SymPlaceHandler
    for Box<dyn SymPlaceHandler<SymEntity = SE, ConcEntity = CE, Entity = E>>
{
    type SymEntity = SE;
    type ConcEntity = CE;
    type Entity = E;

    fn handle<'a>(
        &mut self,
        sym_entity: Self::SymEntity,
        get_conc: Box<dyn FnOnce(&Self::SymEntity) -> Self::ConcEntity + 'a>,
    ) -> Self::Entity {
        self.as_mut().handle(sym_entity, get_conc)
    }
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
