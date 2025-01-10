use common::{log_debug, log_warn};

use crate::abs::backend::{SolveResult, Solver};

use super::{Constraint, TraceInspector};

/// Ensures the constraints generated for the trace are satisfiable,
/// given that the assumptions are held, i.e.:
/// constraints && assumptions === True
/// For example, the constraints must be satisfiable when the symbolic variables are tied
/// to their concrete values.
pub(crate) struct ConstraintSanityChecker<S: Solver, TS, AI, const PANIC: bool = true> {
    solver: S,
    assumptions: AI,
    _phantom: core::marker::PhantomData<TS>,
}

impl<S: Solver, TS, AI> ConstraintSanityChecker<S, TS, AI> {
    pub(crate) fn new<const PANIC: bool>(
        solver: S,
        assumptions: AI,
    ) -> ConstraintSanityChecker<S, TS, AI, PANIC> {
        ConstraintSanityChecker {
            solver,
            assumptions,
            _phantom: Default::default(),
        }
    }
}

impl<S: Solver, TS, AI, const PANIC: bool> TraceInspector<TS, S::Value, S::Case>
    for ConstraintSanityChecker<S, TS, AI, PANIC>
where
    S::Value: core::fmt::Debug,
    S::Case: core::fmt::Debug,
    for<'a> &'a mut AI: IntoIterator<Item = &'a Constraint<S::Value, S::Case>>,
{
    #[tracing::instrument(level = "debug", skip_all)]
    fn inspect(&mut self, _steps: &[TS], constraints: &[Constraint<S::Value, S::Case>]) {
        let constrained = self.assumptions.into_iter().chain(constraints.into_iter());
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
