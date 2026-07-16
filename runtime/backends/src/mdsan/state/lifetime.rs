use crate::pri::fluent::backend::LifetimeHandler;

use super::super::alias::backend;
use backend::{MdMemoryState, MdSanBackend, MdSanVariablesState};

use super::PlaceValue;

pub(crate) struct MdSanLifetimeHandler<'s> {
    vars_state: &'s mut MdSanVariablesState,
}

impl<'s> MdSanLifetimeHandler<'s> {
    pub(crate) fn new(backend: &'s mut MdSanBackend) -> Self {
        Self {
            vars_state: &mut backend.vars_state,
        }
    }
}

impl<'s> LifetimeHandler for MdSanLifetimeHandler<'s> {
    type Place = PlaceValue;

    fn mark_live(self, _place: Self::Place) {
        // Nothing to do for now.
    }

    fn mark_dead(self, place: Self::Place) {
        match place {
            PlaceValue::LifetimeMarkedMd { mem_region } => {
                self.vars_state.erase_place(&mem_region);
            }
            PlaceValue::NonRelevant {} => {}
            PlaceValue::AccessedMdWrapped { .. }
            | PlaceValue::ToCarryMdContainer { .. }
            | PlaceValue::LazyDestination(..)
            | PlaceValue::ToDropMaybeMdWrapped { .. } => unreachable!(),
        }
    }
}
