use super::{Constraint, TraceInspector, TraceManager};

pub(crate) struct AggregatorTraceManager<S, V, C, I: TraceInspector<S, V, C>> {
    steps: Vec<S>,
    constraints: Vec<Constraint<V, C>>,
    inspector: I,
}

impl<S, V, C, I: TraceInspector<S, V, C>> AggregatorTraceManager<S, V, C, I> {
    pub(crate) fn new(inspector: I) -> Self {
        Self {
            steps: Vec::new(),
            constraints: Vec::new(),
            inspector,
        }
    }
}

impl<S, V, C, I: TraceInspector<S, V, C>> TraceManager<S, V, C>
    for AggregatorTraceManager<S, V, C, I>
{
    fn notify_step(&mut self, step: S, constraint: Constraint<V, C>) {
        self.steps.push(step);
        self.constraints.push(constraint);
        self.inspector.inspect(&self.steps, &self.constraints);
    }
}
