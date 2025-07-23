use core::borrow::Borrow;

use common::log_warn;

use crate::abs::backend::{SolveResult, Solver};

use super::{Constraint, TraceInspector};

pub(crate) const TAG: &str = "constraint_sanity";

/// Ensures the constraints generated for the trace are satisfiable,
/// given that the assumptions are held, i.e.:
/// constraints && assumptions === True
/// For example, the constraints must be satisfiable when the symbolic variables are tied
/// to their concrete values.
pub(crate) struct FullTraceSanityChecker<'o, TSolver, AI, S, V, C, const PANIC: bool = true> {
    solver: TSolver,
    assumptions: AI,
    disable_further_checks: bool,
    unsat_observer: Option<Box<dyn FnOnce(&[S], &[Constraint<V, C>]) + 'o>>,
}

impl<'o, TSolver: 'o, AI: 'o, S: 'o, V: 'o, C: 'o>
    FullTraceSanityChecker<'o, TSolver, AI, S, V, C>
{
    pub(crate) fn new<const PANIC: bool>(
        solver: TSolver,
        assumptions: AI,
        unsat_observer: Option<impl FnOnce(&[S], &[Constraint<V, C>]) + 'o>,
    ) -> FullTraceSanityChecker<'o, TSolver, AI, S, V, C, PANIC> {
        FullTraceSanityChecker {
            solver,
            assumptions,
            disable_further_checks: false,
            unsat_observer: unsat_observer
                .map(|f| Box::new(f) as Box<dyn FnOnce(&[S], &[Constraint<V, C>])>),
        }
    }
}

impl<'o, TSolver: Solver + 'o, S: 'o, V: 'o, C: 'o, AI: 'o, const PANIC: bool>
    TraceInspector<S, V, C> for FullTraceSanityChecker<'o, TSolver, AI, S, V, C, PANIC>
where
    V: Borrow<TSolver::Value>,
    C: Borrow<TSolver::Case>,
    TSolver::Value: Clone,
    TSolver::Case: Clone,
    for<'a> &'a mut AI: IntoIterator<Item = Constraint<TSolver::Value, TSolver::Case>>,
{
    #[tracing::instrument(level = "debug", skip_all)]
    fn inspect(&mut self, steps: &[S], constraints: &[Constraint<V, C>]) {
        if !PANIC {
            if self.disable_further_checks {
                return;
            }
        }

        let constrained = self.assumptions.into_iter().chain(
            constraints
                .into_iter()
                .map(Constraint::as_ref)
                .map(|c| c.map(Borrow::borrow, Borrow::borrow))
                .map(Constraint::cloned),
        );
        if !matches!(self.solver.check(constrained), SolveResult::Sat(_)) {
            if let Some(observer) = self.unsat_observer.take() {
                observer(steps, constraints);
            }

            if PANIC {
                panic!("Trace is not satisfiable.");
            } else {
                log_warn!("Trace is not satisfiable, disabling further checks");
                self.disable_further_checks = true;
            }
        }
    }
}

/// Ensures that each constraint generated is individually satisfiable,
/// given that the assumptions are held, i.e.:
/// constraint && assumptions === True
/// For example, the constraints must be satisfiable when the symbolic variables are tied
/// to their concrete values.
pub(crate) struct StepSanityChecker<'o, TSolver, AI, S, V, C> {
    solver: TSolver,
    assumptions: AI,
    unsat_observer: Option<Box<dyn FnMut(&S, Constraint<&V, &C>) + 'o>>,
}

impl<'o, TSolver: Solver + 'o, AI: 'o, S: 'o, V: 'o, C: 'o>
    StepSanityChecker<'o, TSolver, AI, S, V, C>
where
    TSolver::Value: Clone,
    TSolver::Case: Clone,
    for<'a> &'a mut AI: IntoIterator<Item = Constraint<TSolver::Value, TSolver::Case>>,
{
    pub(crate) fn new(
        solver: TSolver,
        assumptions: AI,
        unsat_observer: Option<impl FnMut(&S, Constraint<&V, &C>) + 'o>,
    ) -> StepSanityChecker<'o, TSolver, AI, S, V, C> {
        StepSanityChecker {
            solver,
            assumptions,
            unsat_observer: unsat_observer
                .map(|f| Box::new(f) as Box<dyn FnMut(&S, Constraint<&V, &C>)>),
        }
    }

    pub(crate) fn into_filter(mut self) -> impl FnMut(&S, Constraint<&V, &C>) -> bool + 'o
    where
        V: Borrow<TSolver::Value>,
        C: Borrow<TSolver::Case>,
    {
        move |step, constraint| {
            let result = self.check(&constraint);
            if !result {
                log_warn!(
                    target: TAG,
                    "Unsatisfiable constraint observed, filtering it out",
                );
                if let Some(observer) = &mut self.unsat_observer {
                    observer(step, constraint);
                }
            }
            result
        }
    }

    fn check(&mut self, constraint: &Constraint<&V, &C>) -> bool
    where
        V: Borrow<TSolver::Value>,
        C: Borrow<TSolver::Case>,
    {
        let constrained = self.assumptions.into_iter().chain(
            std::iter::once(constraint.as_ref())
                .map(|c| c.map(|v| (*v).borrow(), |c| (*c).borrow()))
                .map(Constraint::cloned),
        );
        matches!(self.solver.check(constrained), SolveResult::Sat(_))
    }
}
