use crate::{
    abs::{Constant, SymVariable},
    backends::mdsan::state::WritablePlace,
    pri::fluent::backend::OperandHandler,
};

use super::alias::backend;
use backend::{
    MdMemoryState, MdSanBackend, MdSanPlaceInspector, MdSanPlaceValue, MdSanValue,
    MdSanVariablesState, PlaceInspector,
};

pub(crate) struct MdSanOperandHandler<'a> {
    vars_state: &'a mut MdSanVariablesState,
}

impl<'a> MdSanOperandHandler<'a> {
    pub fn new(backend: &'a mut MdSanBackend) -> Self {
        Self {
            vars_state: &mut backend.vars_state,
        }
    }
}

impl OperandHandler for MdSanOperandHandler<'_> {
    type Place = MdSanPlaceValue;
    type Operand = MdSanValue;

    fn copy_of(self, place: Self::Place) -> Self::Operand {
        MdSanPlaceInspector::new(self.vars_state).inspect_place_for_access(&place);
        MdSanValue::non_rel()
    }

    fn move_of(self, place: Self::Place) -> Self::Operand {
        match place {
            backend::state::PlaceValue2::NonRelevant {} => MdSanValue::non_rel(),
            backend::state::PlaceValue2::AccessedMdWrapped { .. } => {
                // Calling into_inner: Decommission of the MD wrapper, nothing to carry
                MdSanValue::non_rel()
            }
            backend::state::PlaceValue2::ToCarryMdContainer { mem_region } => {
                self.vars_state.take_place(&mem_region)
            }
            backend::state::PlaceValue2::LazyDestination(WritablePlace { .. }) => unreachable!(),
            backend::state::PlaceValue2::LifetimeMarkedMd { .. } => unreachable!(),
            backend::state::PlaceValue2::ToDropMdWrapped { .. } => unreachable!(),
        }
    }

    fn const_from(self, _info: Constant) -> Self::Operand {
        MdSanValue::non_rel()
    }

    fn some(self) -> Self::Operand {
        MdSanValue::non_rel()
    }

    fn new_symbolic(self, var: SymVariable<Self::Operand>) -> Self::Operand {
        unimplemented!()
    }
}
