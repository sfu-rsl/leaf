use core::borrow::Borrow;
use core::iter;

use common::{log_debug, log_info, log_warn};

use crate::abs::backend::{SolveResult, Solver};

use super::{Constraint, inspect::TraceInspector};

mod coverage;
pub(crate) mod filter;

pub(crate) use coverage::{BranchCoverageDepthDivergenceFilter, DepthProvider};
pub(crate) use filter::DivergenceFilter;

pub(crate) struct ImmediateDivergingAnswerFinder<TSolver: Solver, F> {
    solver: TSolver,
    filter: F,
    optimistic_divergence_solver: Option<TSolver>,
    model_consumer: Box<dyn FnMut(TSolver::Model)>,
    _phantom: core::marker::PhantomData<()>,
}

impl<S, V, C, TSolver: Solver, F: DivergenceFilter<S, V, C>> TraceInspector<S, V, C>
    for ImmediateDivergingAnswerFinder<TSolver, F>
where
    V: Borrow<TSolver::Value>,
    C: Borrow<TSolver::Case>,
    TSolver::Value: Clone,
    TSolver::Case: Clone,
{
    fn inspect(&mut self, steps: &[S], constraints: &[Constraint<V, C>]) {
        if !self.filter.should_find(steps, constraints) {
            log_debug!("Diverging answer finding will be skipped.");
            return;
        }

        log_debug!("Negating the last constraint");
        let not_last = constraints.last().unwrap().as_ref().not();

        if !Self::check(
            &mut self.solver,
            constraints[..constraints.len() - 1]
                .iter()
                .map(Constraint::as_ref)
                .chain(iter::once(not_last.clone())),
            &mut self.model_consumer,
        ) {
            /* NOTE: What is optimistic checking?
             * Consider two independent branch conditions at the same level
             * that the current execution has taken neither.
             * Even if we satisfy the condition for the second one, we
             * have a chance to make a change in the execution path.
             * Thus we do not necessary need to satisfy the constraints for the
             * first one.
             */
            if let Some(ref mut solver) = self.optimistic_divergence_solver {
                log_debug!("Checking optimistically using the last constraint");
                Self::check(
                    solver,
                    iter::once(not_last.clone()),
                    &mut self.model_consumer,
                );
            }
        }
    }
}

impl<S: Solver, F> ImmediateDivergingAnswerFinder<S, F> {
    pub fn new(
        solver: S,
        filter: F,
        optimistic_divergence_solver: Option<S>,
        model_consumer: Box<dyn FnMut(S::Model)>,
    ) -> Self {
        Self {
            solver,
            filter,
            optimistic_divergence_solver,
            model_consumer,
            _phantom: Default::default(),
        }
    }
}

impl<TSolver: Solver, F> ImmediateDivergingAnswerFinder<TSolver, F> {
    pub(crate) fn check<'a, 'b, V: 'a, C: 'a>(
        solver: &mut TSolver,
        constraints: impl Iterator<Item = Constraint<&'a V, &'a C>>,
        model_consumer: &'b mut dyn FnMut(TSolver::Model),
    ) -> bool
    where
        V: Borrow<TSolver::Value>,
        C: Borrow<TSolver::Case>,
        TSolver::Value: Clone,
        TSolver::Case: Clone,
    {
        let result = solver.check(
            constraints
                .map(|c| c.map(Borrow::borrow, Borrow::borrow))
                .map(Constraint::cloned),
        );
        match result {
            SolveResult::Sat(model) => {
                model_consumer(model);
                true
            }
            _ => {
                log_info!("Unsatisfiable or unknown result.");
                false
            }
        }
    }
}
