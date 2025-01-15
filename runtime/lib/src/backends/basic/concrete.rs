use crate::abs::ConstraintKind;

use super::{
    alias::{RRef, ValueRefBinaryExprBuilder},
    BasicTraceManager, ConcreteValueRef, Constraint, SymValueRef,
};
use common::log_debug;

pub(super) trait Concretizer {
    fn stamp(&mut self, value: SymValueRef, concrete_value: ConcreteValueRef);
}

pub(super) struct BasicConcretizer<EB: ValueRefBinaryExprBuilder> {
    expr_builder: RRef<EB>,
    trace_manager: RRef<BasicTraceManager>,
}

impl<EB: ValueRefBinaryExprBuilder> BasicConcretizer<EB> {
    pub fn new(expr_builder: RRef<EB>, trace_manager: RRef<BasicTraceManager>) -> Self {
        Self {
            expr_builder,
            trace_manager,
        }
    }
}

impl<EB: ValueRefBinaryExprBuilder> Concretizer for BasicConcretizer<EB> {
    fn stamp(&mut self, value: SymValueRef, concrete_value: ConcreteValueRef) {
        log_debug!("Stamping symbolic value {} == {}", value, concrete_value);
        let eq_expr = self
            .expr_builder
            .borrow_mut()
            .eq((value.0, concrete_value.0).into());
        // NOTE: We do not use equality constraint here because that is meant for switch cases.
        let constraint = Constraint {
            discr: eq_expr,
            kind: ConstraintKind::Bool,
        };
        self.trace_manager.borrow_mut().notify_step(
            Default::default(), /* TODO: A unique index like basic block index. */
            constraint,
        );
    }
}
