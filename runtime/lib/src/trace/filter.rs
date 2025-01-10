use common::log_debug;

use super::{Constraint, TraceManager};

struct StepFilterTraceManager<F, S, V, C, M: TraceManager<S, V, C>> {
    inner: M,
    filter: F,
    _phantom: core::marker::PhantomData<(S, V, C)>,
}

impl<F, S, V, C, M: TraceManager<S, V, C>> StepFilterTraceManager<F, S, V, C, M> {}

impl<F, S, V, C, M: TraceManager<S, V, C>> TraceManager<S, V, C>
    for StepFilterTraceManager<F, S, V, C, M>
where
    F: Fn(&S) -> bool,
{
    fn notify_step(&mut self, step: S, constraint: Constraint<V, C>) {
        if (self.filter)(&step) {
            self.inner.notify_step(step, constraint);
        } else {
            log_debug!("Filtered out step");
        }
    }
}
