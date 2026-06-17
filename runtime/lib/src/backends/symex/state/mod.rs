mod pointer_based;
mod sym_place;

use crate::pri::fluent::backend::LifetimeHandler;

pub(super) use pointer_based::RawPointerVariableState;
pub(super) use sym_place::{
    SymPlaceHandler, SymPlaceSymEntity, strategies::make_sym_place_handler,
};

use super::alias::backend;
use backend::{SymExBackend, SymExPlaceValue, VariablesState};

pub(crate) struct SymExLifetimeHandler<'s> {
    vars_state: &'s mut dyn VariablesState,
}

impl<'s> SymExLifetimeHandler<'s> {
    pub(super) fn new(backend: &'s mut SymExBackend) -> Self {
        Self {
            vars_state: &mut backend.vars_state,
        }
    }
}

impl<'s> LifetimeHandler for SymExLifetimeHandler<'s> {
    type Place = SymExPlaceValue;

    fn mark_live(self, _place: Self::Place) {
        // Nothing to do for now.
    }

    fn mark_dead(self, place: Self::Place) {
        self.vars_state.drop_place(&place);
    }
}
