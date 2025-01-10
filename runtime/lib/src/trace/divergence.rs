use core::iter;

use common::{log_debug, log_info, log_warn};

use crate::abs::backend::{SolveResult, Solver};

use super::{inspect::TraceInspector, Constraint};

pub(crate) trait DivergenceFilter<S> {
    fn should_find(&mut self, trace: &[S]) -> bool;
}

impl<S, T: FnMut(&[S]) -> bool> DivergenceFilter<S> for T {
    fn should_find(&mut self, trace: &[S]) -> bool {
        self(trace)
    }
}

pub(crate) struct ImmediateDivergingAnswerFinder<S: Solver, TS, C> {
    solver: S,
    filter: C,
    optimistic_divergence_solver: Option<S>,
    model_consumer: Box<dyn FnMut(S::Model)>,
    _phantom: core::marker::PhantomData<TS>,
}

impl<S: Solver, TS, C: DivergenceFilter<TS>> TraceInspector<TS, S::Value, S::Case>
    for ImmediateDivergingAnswerFinder<S, TS, C>
where
    S::Value: Clone,
    S::Case: Clone,
{
    fn inspect(&mut self, steps: &[TS], constraints: &[Constraint<S::Value, S::Case>]) {
        if !self.filter.should_find(steps) {
            return;
        }

        log_debug!("Negating the last constraint");
        let not_last = constraints.last().cloned().unwrap().not();

        if !Self::check(
            &mut self.solver,
            constraints[..constraints.len() - 1]
                .into_iter()
                .chain(iter::once(&not_last)),
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
                Self::check(solver, iter::once(&not_last), &mut self.model_consumer);
            }
        }
    }
}

impl<S: Solver, TS, C> ImmediateDivergingAnswerFinder<S, TS, C> {
    pub fn new(
        solver: S,
        filter: C,
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

impl<S: Solver, TS, C> ImmediateDivergingAnswerFinder<S, TS, C> {
    pub(crate) fn check<'a>(
        solver: &mut S,
        constraints: impl Iterator<Item = &'a Constraint<S::Value, S::Case>>,
        model_consumer: &'a mut dyn FnMut(S::Model),
    ) -> bool
    where
        S: 'a,
    {
        let result = solver.check(constraints);
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
