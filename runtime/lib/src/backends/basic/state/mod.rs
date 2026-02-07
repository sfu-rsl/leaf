mod pointer_based;
mod sym_place;

use crate::abs::backend::MemoryHandler;

pub(super) use pointer_based::RawPointerVariableState;
pub(super) use sym_place::{
    SymPlaceHandler, SymPlaceSymEntity, strategies::make_sym_place_handler,
};

use crate::backends::basic as backend;
use backend::{BasicBackend, BasicPlaceValue, VariablesState};

pub(crate) struct BasicMemoryHandler<'s> {
    vars_state: &'s mut dyn VariablesState,
}

impl<'s> BasicMemoryHandler<'s> {
    pub(super) fn new(backend: &'s mut BasicBackend) -> Self {
        Self {
            vars_state: &mut backend.vars_state,
        }
    }
}

impl<'s> MemoryHandler for BasicMemoryHandler<'s> {
    type Place = BasicPlaceValue;

    fn mark_dead(self, place: Self::Place) {
        self.vars_state.drop_place(&place);
    }
}
