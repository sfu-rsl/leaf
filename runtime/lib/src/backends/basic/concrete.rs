use common::{log_debug, log_info};

use crate::backends::basic::expr::{Expr, PorterValue, RawConcreteValue, SymValue};
use crate::{abs::ConstraintKind, utils::alias::RRef};

use crate::backends::basic as backend;
use backend::{
    BasicConstraint, BasicTraceManager, ConcreteValueRef, Implied, SymValueRef,
    alias::ValueRefBinaryExprBuilder,
};

pub(super) type ConcolicValueObtainer<'a, ConcEntity> = dyn FnOnce() -> ConcEntity + 'a;

pub(super) trait Concretizer {
    fn stamp(
        &mut self,
        value: SymValueRef,
        get_conc: Box<ConcolicValueObtainer<ConcreteValueRef>>,
    ) -> ConcreteValueRef;
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
    fn stamp(
        &mut self,
        sym_value: SymValueRef,
        get_conc: Box<ConcolicValueObtainer<ConcreteValueRef>>,
    ) -> ConcreteValueRef {
        log_info!("Stamping {} to its concrete value", sym_value);

        let (eq_expr, conc_value) = match sym_value.as_ref() {
            SymValue::Expression(Expr::Partial(PorterValue {
                as_concrete,
                sym_values,
            })) => {
                let eq_expr = sym_values
                    .iter()
                    .map(|(offset, ty_id, sym_value)| {
                        (
                            sym_value,
                            RawConcreteValue(
                                as_concrete.0.wrapping_byte_add(*offset as usize),
                                (*ty_id).into(),
                            ),
                        )
                    })
                    .map(|(sym_value, conc_value)| {
                        self.expr_builder.borrow_mut().eq((
                            sym_value.clone_to(),
                            conc_value.to_value_ref(),
                        )
                            .into())
                    })
                    .reduce(|all, part| self.expr_builder.borrow_mut().and((all, part).into()))
                    .unwrap();
                (
                    eq_expr,
                    ConcreteValueRef::new(as_concrete.clone().to_value_ref()),
                )
            }
            _ => {
                let conc_value = get_conc();
                let eq_expr =
                    self.expr_builder
                        .borrow_mut()
                        .eq((sym_value.0, conc_value.clone_to()).into());
                (eq_expr, conc_value)
            }
        };

        // NOTE: We do not use equality constraint here because that is meant for switch cases.
        let constraint = BasicConstraint {
            discr: Implied::by_unknown(eq_expr), // TODO
            kind: ConstraintKind::True,
        };
        self.trace_manager.borrow_mut().notify_step(
            Default::default(), /* TODO: A unique index like basic block index. */
            constraint,
        );
        conc_value
    }
}
