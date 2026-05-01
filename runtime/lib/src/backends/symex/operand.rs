use common::log_error;

use crate::{
    abs::{Constant, SymVariable},
    pri::fluent::backend::OperandHandler,
    utils::alias::RRef,
};

use super::alias::backend;
use backend::{
    Implied, PlaceValueRef, SymExBackend, SymExSymVariablesManager, SymVariablesManager,
    VariablesState, expr::prelude::ConcreteValue,
};

use super::SymExValue;

pub(crate) struct SymExOperandHandler<'a> {
    vars_state: &'a mut dyn VariablesState,
    sym_values: RRef<SymExSymVariablesManager>,
}

impl<'a> SymExOperandHandler<'a> {
    pub fn new(backend: &'a mut SymExBackend) -> Self {
        Self {
            vars_state: &mut backend.vars_state,
            sym_values: backend.sym_values.clone(),
        }
    }
}

impl OperandHandler for SymExOperandHandler<'_> {
    type Place = PlaceValueRef;
    type Operand = SymExValue;

    fn copy_of(self, place: Self::Place) -> Self::Operand {
        self.vars_state.copy_place(&place)
    }

    fn move_of(self, place: Self::Place) -> Self::Operand {
        self.vars_state.take_place(&place)
    }

    fn const_from(self, info: Constant) -> Self::Operand {
        Implied::always(ConcreteValue::from(info).to_value_ref())
    }

    fn some(self) -> Self::Operand {
        log_error!("Operand info is not available.");
        panic!("Operand details are expected in this backend.")
    }

    fn new_symbolic(self, var: SymVariable<Self::Operand>) -> Self::Operand {
        let value = self.sym_values.borrow_mut().add_variable(var).into();
        Implied::by_unknown(value)
    }
}
