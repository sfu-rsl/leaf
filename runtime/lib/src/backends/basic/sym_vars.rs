use delegate::delegate;

use std::collections::HashMap;

use common::{log_info, types::trace::Constraint};

use crate::abs::SymVariable;

use crate::backends::basic as backend;
use backend::{
    ConcreteValue, ConcreteValueRef, Implied, SymValue, SymValueRef, SymVarId, SymVariablesManager,
    SymbolicVar, Value, ValueRef, expr::ConstValue,
};

pub(super) struct BasicSymVariablesManager {
    variables: HashMap<SymVarId, (SymValueRef, ConcreteValueRef)>,
    conc_constraints: HashMap<SymVarId, Constraint<SymValueRef, ConstValue>>,
}

impl BasicSymVariablesManager {
    pub(crate) fn new() -> Self {
        Self {
            variables: HashMap::new(),
            conc_constraints: HashMap::new(),
        }
    }

    delegate! {
        to self.variables {
            pub(crate) fn len(&self) -> usize;
        }
    }
}

impl SymVariablesManager for BasicSymVariablesManager {
    fn add_variable(&mut self, var: SymVariable<Implied<ValueRef>>) -> SymValueRef {
        let conc_val = var
            .conc_value
            .expect("Concrete value of symbolic variables is required.");

        let Value::Concrete(ConcreteValue::Const(const_val)) = conc_val.value.as_ref() else {
            panic!("Only constant values are currently expected to be used as the concrete value.");
        };

        let id = self.len() as u32 + 1;

        let sym_val = SymValue::Variable(SymbolicVar::new(id, var.ty)).to_value_ref();
        let conc_val = ConcreteValueRef::new(conc_val.value.clone());

        self.variables
            .insert(id, (sym_val.clone(), conc_val.clone()));

        log_info!("Added a new symbolic variable: {} = {}", sym_val, conc_val);

        self.conc_constraints
            .insert(id, Constraint::equality(sym_val.clone(), const_val.clone()));

        sym_val.into()
    }

    fn iter_variables(
        &self,
    ) -> impl ExactSizeIterator<Item = (&SymVarId, &SymValueRef, &ConcreteValueRef)> {
        self.variables
            .iter()
            .map(|(id, (sym_val, conc_val))| (id, sym_val, conc_val))
    }

    #[inline]
    fn iter_concretization_constraints(
        &self,
    ) -> impl ExactSizeIterator<Item = (&SymVarId, &Constraint<SymValueRef, ConstValue>)> {
        self.conc_constraints.iter()
    }
}
