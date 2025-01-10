use super::Constraint;

pub(crate) trait TraceInspector<S, V, C> {
    fn inspect(&mut self, steps: &[S], constraints: &[Constraint<V, C>]);
}

pub(crate) type NoopTraceInspector = ();

impl<S, V, C> TraceInspector<S, V, C> for NoopTraceInspector {
    fn inspect(&mut self, _steps: &[S], _constraints: &[Constraint<V, C>]) {}
}

pub(crate) struct StaticCompoundTraceInspector<
    S,
    V,
    C,
    I: TraceInspector<S, V, C>,
    J: TraceInspector<S, V, C>,
> {
    first: I,
    second: J,
    _phantom: core::marker::PhantomData<(S, V, C)>,
}

impl<S, V, C, I: TraceInspector<S, V, C>, J: TraceInspector<S, V, C>> From<(I, J)>
    for StaticCompoundTraceInspector<S, V, C, I, J>
{
    fn from((first, second): (I, J)) -> Self {
        Self {
            first,
            second,
            _phantom: Default::default(),
        }
    }
}

impl<S, V, C, I: TraceInspector<S, V, C>, J: TraceInspector<S, V, C>> TraceInspector<S, V, C>
    for StaticCompoundTraceInspector<S, V, C, I, J>
{
    fn inspect(&mut self, steps: &[S], constraints: &[Constraint<V, C>]) {
        self.first.inspect(steps, constraints);
        self.second.inspect(steps, constraints);
    }
}

pub(crate) struct CompoundTraceInspector<S, V, C> {
    inspectors: Vec<Box<dyn TraceInspector<S, V, C>>>,
}

impl<S, V, C> CompoundTraceInspector<S, V, C> {
    pub(crate) fn new(inspectors: Vec<Box<dyn TraceInspector<S, V, C>>>) -> Self {
        Self { inspectors }
    }
}

impl<S, V, C> TraceInspector<S, V, C> for CompoundTraceInspector<S, V, C> {
    fn inspect(&mut self, steps: &[S], constraints: &[Constraint<V, C>]) {
        for inspector in self.inspectors.iter_mut() {
            inspector.inspect(steps, constraints);
        }
    }
}
