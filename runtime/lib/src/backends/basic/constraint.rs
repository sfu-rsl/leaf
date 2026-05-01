use std::cell::RefMut;

use crate::{
    abs::{
        self, AssertKind, BasicBlockIndex, BasicBlockLocation, ConstraintKind, SwitchCaseIndex,
        utils::BasicBlockLocationExt,
    },
    pri::fluent::backend::{ConstraintHandler, SwitchHandler},
    utils::alias::RRef,
};

use crate::backends::basic as backend;
use crate::call::CallFlowManager;
use backend::{
    SymExBackend, SymExExprBuilder, SymExTraceManager, SymExValue,
    alias::SymExValueUnaryExprBuilder, expr::prelude::ConstValue,
};

pub(super) type Constraint = crate::abs::Constraint<SymExValue, ConstValue>;
pub(super) type DecisionCase = ConstValue;

pub(crate) struct SymExConstraintHandler<'a, EB> {
    location: BasicBlockLocation,
    trace_manager: RefMut<'a, SymExTraceManager>,
    expr_builder: RRef<EB>,
}

impl<'a> SymExConstraintHandler<'a, SymExExprBuilder> {
    pub(super) fn new(backend: &'a mut SymExBackend, location: BasicBlockIndex) -> Self {
        Self {
            trace_manager: backend.trace_manager.borrow_mut(),
            expr_builder: backend.expr_builder.clone(),
            location: backend
                .call_flow_manager
                .current_func()
                .body_id
                .at_basic_block(location),
        }
    }
}

impl<'a, EB: SymExValueUnaryExprBuilder> ConstraintHandler for SymExConstraintHandler<'a, EB> {
    type Operand = SymExValue;
    type SwitchHandler = SymExSwitchHandler<'a, EB>;

    #[inline]
    fn switch(self, discriminant: Option<Self::Operand>) -> Self::SwitchHandler {
        let discr = discriminant.expect("Data is missing");
        let discr = self.expr_builder.borrow_mut().no_op(discr);
        SymExSwitchHandler {
            discr,
            parent: self,
        }
    }

    fn assert(
        mut self,
        cond: Self::Operand,
        expected: bool,
        _assert_kind: AssertKind<Self::Operand>,
    ) {
        // For now, we will call this function before the assert occurs and assume that assertions always succeed.
        // TODO: add a result: bool parameter to this function, and add support for it using a panic hook.
        if cond.is_symbolic() {
            // NOTE: This is a trick to pass the value through the expression builder
            // to ensure value resolving and simplifications.
            let cond = self.expr_builder.borrow_mut().no_op(cond);
            let mut constraint = Constraint {
                discr: cond,
                kind: ConstraintKind::True,
            };
            if !expected {
                constraint = constraint.not();
            }

            self.notify_constraint(constraint);
        }
    }
}

impl<'a, EB> SymExConstraintHandler<'a, EB> {
    fn notify_constraint(&mut self, constraint: Constraint) {
        self.trace_manager
            .notify_step(Into::into(self.location), constraint);
    }
}

pub(crate) struct SymExSwitchHandler<'a, EB> {
    discr: SymExValue,
    parent: SymExConstraintHandler<'a, EB>,
}

impl<'a, EB> SwitchHandler for SymExSwitchHandler<'a, EB> {
    #[inline]
    fn take(mut self, _case_index: SwitchCaseIndex, value: Option<abs::Constant>) {
        let value = value.expect("Data is missing");
        let constraint = self.create_constraint(vec![value]);
        self.parent.notify_constraint(constraint);
    }

    fn take_otherwise(mut self, non_values: Option<Vec<abs::Constant>>) {
        let non_values = non_values.expect("Data is missing");
        let constraint = self.create_constraint(non_values).not();
        self.parent.notify_constraint(constraint);
    }
}

impl<'a, EB> SymExSwitchHandler<'a, EB> {
    fn create_constraint(&mut self, values: Vec<abs::Constant>) -> Constraint {
        let kind = match values.first().unwrap() {
            abs::Constant::Bool(false) => ConstraintKind::False,
            _ => ConstraintKind::OneOf(
                values
                    .into_iter()
                    .map(|c| ConstValue::try_from(c).unwrap())
                    .collect(),
            ),
        };
        Constraint {
            discr: self.discr.clone(),
            kind,
        }
    }
}
