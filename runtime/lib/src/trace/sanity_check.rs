use core::borrow::Borrow;

use common::{log_debug, log_warn};

use crate::abs::backend::{SolveResult, Solver};

use super::{Constraint, TraceInspector};

/// Ensures the constraints generated for the trace are satisfiable,
/// given that the assumptions are held, i.e.:
/// constraints && assumptions === True
/// For example, the constraints must be satisfiable when the symbolic variables are tied
/// to their concrete values.
pub(crate) struct ConstraintSanityChecker<TSolver: Solver, AI, const PANIC: bool = true> {
    solver: TSolver,
    assumptions: AI,
    _phantom: core::marker::PhantomData<()>,
}

impl<S: Solver, AI> ConstraintSanityChecker<S, AI> {
    pub(crate) fn new<const PANIC: bool>(
        solver: S,
        assumptions: AI,
    ) -> ConstraintSanityChecker<S, AI, PANIC> {
        ConstraintSanityChecker {
            solver,
            assumptions,
            _phantom: Default::default(),
        }
    }
}

impl<TSolver: Solver, S, V, C, AI, const PANIC: bool> TraceInspector<S, V, C>
    for ConstraintSanityChecker<TSolver, AI, PANIC>
where
    V: Borrow<TSolver::Value>,
    C: Borrow<TSolver::Case>,
    V: core::fmt::Debug,
    C: core::fmt::Debug,
    TSolver::Value: Clone,
    TSolver::Case: Clone,
    for<'a> &'a mut AI: IntoIterator<Item = Constraint<TSolver::Value, TSolver::Case>>,
{
    #[tracing::instrument(level = "debug", skip_all)]
    fn inspect(&mut self, _steps: &[S], constraints: &[Constraint<V, C>]) {
        let constrained = self.assumptions.into_iter().chain(
            constraints
                .into_iter()
                .map(Constraint::as_ref)
                .map(|c| c.map(Borrow::borrow, Borrow::borrow))
                .map(Constraint::cloned),
        );
        if !matches!(self.solver.check(constrained), SolveResult::Sat(_)) {
            if PANIC {
                panic!("Unsatisfiable constraints: {:?}", constraints);
            } else {
                log_warn!("Unsatisfiable constraints");
                log_debug!("Constraints: {:?}", constraints);
            }
        }
    }
}
