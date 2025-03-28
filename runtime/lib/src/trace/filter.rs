use common::log_debug;

use super::{Constraint, StepInspector, TraceManager};

struct StepFilterTraceManager<F, S, V, C, M: TraceManager<S, V, C>> {
    inner: M,
    predicate: F,
    _phantom: core::marker::PhantomData<(S, V, C)>,
}

impl<F, S, V, C, M: TraceManager<S, V, C>> StepFilterTraceManager<F, S, V, C, M> {}

impl<F, S, V, C, M: TraceManager<S, V, C>> TraceManager<S, V, C>
    for StepFilterTraceManager<F, S, V, C, M>
where
    F: FnMut(&S, Constraint<&V, &C>) -> bool,
{
    fn notify_step(&mut self, step: S, constraint: Constraint<V, C>) {
        if (self.predicate)(&step, constraint.as_ref()) {
            self.inner.notify_step(step, constraint);
        } else {
            log_debug!("Filtered out step");
        }
    }
}

pub(crate) trait TraceManagerExt<S, V, C>: TraceManager<S, V, C> {
    fn filtered_by(
        self,
        f: impl FnMut(&S, Constraint<&V, &C>) -> bool,
    ) -> impl TraceManager<S, V, C>
    where
        Self: Sized;
}

impl<S, V, C, M: TraceManager<S, V, C>> TraceManagerExt<S, V, C> for M {
    fn filtered_by(
        self,
        f: impl FnMut(&S, Constraint<&V, &C>) -> bool,
    ) -> impl TraceManager<S, V, C>
    where
        Self: Sized,
    {
        StepFilterTraceManager {
            inner: self,
            predicate: f,
            _phantom: Default::default(),
        }
    }
}

struct FilterStepInspector<F, S, V, C, I: StepInspector<S, V, C>> {
    inner: I,
    predicate: F,
    _phantom: core::marker::PhantomData<(S, V, C)>,
}

impl<F, S, V, C, I: StepInspector<S, V, C>> FilterStepInspector<F, S, V, C, I> {}

impl<F, S, V, C, I: StepInspector<S, V, C>> StepInspector<S, V, C>
    for FilterStepInspector<F, S, V, C, I>
where
    F: FnMut(&S, Constraint<&V, &C>) -> bool,
{
    fn inspect(&mut self, step: &S, constraint: Constraint<&V, &C>) {
        if (self.predicate)(step, constraint.clone()) {
            self.inner.inspect(step, constraint);
        }
    }
}

pub(crate) trait StepInspectorExt<S, V, C>: StepInspector<S, V, C> {
    fn filtered_by(
        self,
        f: impl FnMut(&S, Constraint<&V, &C>) -> bool,
    ) -> impl StepInspector<S, V, C>
    where
        Self: Sized;

    fn timer_freq_filtered(self, min_interval: core::time::Duration) -> impl StepInspector<S, V, C>
    where
        Self: Sized,
    {
        use std::time::Instant;
        let mut last = Instant::now();
        self.filtered_by(move |_, _| {
            let diff = Instant::now() - last;
            if diff >= min_interval {
                last = Instant::now();
                true
            } else {
                false
            }
        })
    }
}
impl<S, V, C, I: StepInspector<S, V, C>> StepInspectorExt<S, V, C> for I {
    fn filtered_by(
        self,
        f: impl FnMut(&S, Constraint<&V, &C>) -> bool,
    ) -> impl StepInspector<S, V, C>
    where
        Self: Sized,
    {
        FilterStepInspector {
            inner: self,
            predicate: f,
            _phantom: Default::default(),
        }
    }
}
