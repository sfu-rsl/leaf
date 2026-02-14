use std::cell::RefMut;

use crate::{
    abs::{
        self, AssertKind, BasicBlockIndex, BasicBlockLocation, ConstraintKind,
        utils::BasicBlockLocationExt,
    },
    pri::fluent::backend::{ConstraintHandler, SwitchHandler},
    utils::alias::RRef,
};

use crate::backends::basic as backend;
use crate::call::CallFlowManager;
use backend::{
    BasicBackend, BasicExprBuilder, BasicTraceManager, BasicValue, BasicValueUnaryExprBuilder,
    expr::prelude::ConstValue,
};

pub(super) type Constraint = crate::abs::Constraint<BasicValue, ConstValue>;
pub(super) type DecisionCase = ConstValue;

pub(crate) struct BasicConstraintHandler<'a, EB> {
    location: BasicBlockLocation,
    trace_manager: RefMut<'a, BasicTraceManager>,
    expr_builder: RRef<EB>,
}

impl<'a> BasicConstraintHandler<'a, BasicExprBuilder> {
    pub(super) fn new(backend: &'a mut BasicBackend, location: BasicBlockIndex) -> Self {
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

impl<'a, EB: BasicValueUnaryExprBuilder> ConstraintHandler for BasicConstraintHandler<'a, EB> {
    type Operand = BasicValue;
    type SwitchHandler = BasicSwitchHandler<'a, EB>;

    fn switch(self, discriminant: Self::Operand) -> Self::SwitchHandler {
        let discr = self.expr_builder.borrow_mut().no_op(discriminant);
        BasicSwitchHandler {
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

impl<'a, EB> BasicConstraintHandler<'a, EB> {
    fn notify_constraint(&mut self, constraint: Constraint) {
        self.trace_manager
            .notify_step(Into::into(self.location), constraint);
    }
}

pub(crate) struct BasicSwitchHandler<'a, EB> {
    discr: BasicValue,
    parent: BasicConstraintHandler<'a, EB>,
}

impl<'a, EB> SwitchHandler for BasicSwitchHandler<'a, EB> {
    fn take(mut self, value: abs::Constant) {
        let constraint = self.create_constraint(vec![value]);
        self.parent.notify_constraint(constraint);
    }

    fn take_otherwise(mut self, non_values: Vec<abs::Constant>) {
        let constraint = self.create_constraint(non_values).not();
        self.parent.notify_constraint(constraint);
    }
}

impl<'a, EB> BasicSwitchHandler<'a, EB> {
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
