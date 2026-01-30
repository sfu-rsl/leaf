use crate::{
    abs::{Constant, SymVariable, backend::OperandHandler},
    utils::alias::RRef,
};

use crate::backends::basic as backend;
use backend::{
    BasicBackend, BasicSymVariablesManager, CallStackInfo, Implied, PlaceValueRef,
    SymVariablesManager, VariablesState, expr::prelude::ConcreteValue,
};

use super::BasicValue;

pub(crate) struct BasicOperandHandler<'a> {
    vars_state: &'a mut dyn VariablesState,
    sym_values: RRef<BasicSymVariablesManager>,
}

impl<'a> BasicOperandHandler<'a> {
    pub fn new(backend: &'a mut BasicBackend) -> Self {
        Self {
            vars_state: backend.call_stack_manager.top(),
            sym_values: backend.sym_values.clone(),
        }
    }
}

impl OperandHandler for BasicOperandHandler<'_> {
    type Place = PlaceValueRef;
    type Operand = BasicValue;

    fn copy_of(self, place: Self::Place) -> Self::Operand {
        self.vars_state.copy_place(&place)
    }

    fn move_of(self, place: Self::Place) -> Self::Operand {
        self.vars_state.take_place(&place)
    }

    fn const_from(self, info: Constant) -> Self::Operand {
        Implied::always(ConcreteValue::from(info).to_value_ref())
    }

    fn new_symbolic(self, var: SymVariable<Self::Operand>) -> Self::Operand {
        let value = self.sym_values.borrow_mut().add_variable(var).into();
        Implied::by_unknown(value)
    }
}
