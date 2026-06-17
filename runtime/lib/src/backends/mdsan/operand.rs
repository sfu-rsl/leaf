use crate::{
    abs::{Constant, SymVariable},
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
        /* MD wrapped values that implement Copy, do not implement Drop, so we do not detect them.
         * Other than those, it is possible to copy a value owned by the wrapped value,
         * which need to be only checked for liveness and does not propagate the state.*/
        MdSanValue::non_rel()
    }

    fn move_of(self, place: Self::Place) -> Self::Operand {
        self.vars_state.take_place(&place)
    }

    fn const_from(self, _info: Constant) -> Self::Operand {
        MdSanValue::non_rel()
    }

    fn some(self) -> Self::Operand {
        MdSanValue::non_rel()
    }

    fn new_symbolic(self, _var: SymVariable<Self::Operand>) -> Self::Operand {
        unimplemented!()
    }
}
