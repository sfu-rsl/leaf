use crate::utils::alias::RRef;

use super::{Constraint, TraceManager};

pub(crate) trait StepInspector<S, V, C> {
    fn inspect(&mut self, step: &S, constraint: &Constraint<V, C>);
}

pub(crate) trait TraceInspector<S, V, C> {
    fn inspect(&mut self, steps: &[S], constraints: &[Constraint<V, C>]);
}

pub(crate) type NoopInspector = ();

impl<S, V, C> StepInspector<S, V, C> for NoopInspector {
    fn inspect(&mut self, _step: &S, _constraint: &Constraint<V, C>) {}
}

impl<S, V, C> TraceInspector<S, V, C> for NoopInspector {
    fn inspect(&mut self, _steps: &[S], _constraints: &[Constraint<V, C>]) {}
}

impl<S, V, C, F> StepInspector<S, V, C> for F
where
    F: FnMut(&S, &Constraint<V, C>),
{
    fn inspect(&mut self, step: &S, constraint: &Constraint<V, C>) {
        self(step, constraint);
    }
}

impl<S, V, C> StepInspector<S, V, C> for Box<dyn StepInspector<S, V, C>> {
    fn inspect(&mut self, step: &S, constraint: &Constraint<V, C>) {
        self.as_mut().inspect(step, constraint);
    }
}

impl<S, V, C, I: StepInspector<S, V, C>> StepInspector<S, V, C> for RRef<I> {
    fn inspect(&mut self, step: &S, constraint: &Constraint<V, C>) {
        self.borrow_mut().inspect(step, constraint);
    }
}

impl<S, V, C> TraceInspector<S, V, C> for Box<dyn TraceInspector<S, V, C>> {
    fn inspect(&mut self, steps: &[S], constraints: &[Constraint<V, C>]) {
        self.as_mut().inspect(steps, constraints);
    }
}

impl<S, V, C, I: StepInspector<S, V, C>> StepInspector<S, V, C> for Vec<I> {
    fn inspect(&mut self, step: &S, constraint: &Constraint<V, C>) {
        for inspector in self.iter_mut() {
            inspector.inspect(step, constraint);
        }
    }
}

impl<S, V, C, I: TraceInspector<S, V, C>> TraceInspector<S, V, C> for Vec<I> {
    fn inspect(&mut self, steps: &[S], constraints: &[Constraint<V, C>]) {
        for inspector in self.iter_mut() {
            inspector.inspect(steps, constraints);
        }
    }
}

pub(crate) struct InspectedTraceManager<
    S,
    V,
    C,
    M: TraceManager<S, V, C>,
    I: StepInspector<S, V, C>,
> {
    inner: M,
    inspector: I,
    _phantom: core::marker::PhantomData<(S, V, C)>,
}

impl<S, V, C, M: TraceManager<S, V, C>, I: StepInspector<S, V, C>> TraceManager<S, V, C>
    for InspectedTraceManager<S, V, C, M, I>
{
    fn notify_step(&mut self, step: S, constraint: Constraint<V, C>) {
        self.inspector.inspect(&step, &constraint);
        self.inner.notify_step(step, constraint);
    }
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

pub(crate) trait TraceManagerExt<S, V, C>: TraceManager<S, V, C> {
    fn inspected_by(self, inspector: impl StepInspector<S, V, C>) -> impl TraceManager<S, V, C>
    where
        Self: Sized;
}

impl<S, V, C, M: TraceManager<S, V, C>> TraceManagerExt<S, V, C> for M {
    fn inspected_by(self, inspector: impl StepInspector<S, V, C>) -> impl TraceManager<S, V, C>
    where
        Self: Sized,
    {
        InspectedTraceManager {
            inner: self,
            inspector,
            _phantom: Default::default(),
        }
    }
}
