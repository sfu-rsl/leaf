use crate::utils::{RefView, alias::RRef};

use super::{Constraint, StepInspector, TraceInspector, TraceManager};

pub struct AggregatorTraceManager<S, V, C, I: TraceInspector<S, V, C>> {
    steps: RRef<Vec<S>>,
    constraints: RRef<Vec<Constraint<V, C>>>,
    inspector: I,
}

impl<S, V, C, I: TraceInspector<S, V, C>> AggregatorTraceManager<S, V, C, I> {
    pub fn new(inspector: I) -> Self {
        Self {
            steps: Default::default(),
            constraints: Default::default(),
            inspector,
        }
    }

    pub fn steps(&self) -> RefView<Vec<S>> {
        RefView::new(self.steps.clone())
    }

    pub fn constraints(&self) -> RefView<Vec<Constraint<V, C>>> {
        RefView::new(self.constraints.clone())
    }
}

impl<S, V, C, I: TraceInspector<S, V, C>> TraceManager<S, V, C>
    for AggregatorTraceManager<S, V, C, I>
{
    fn notify_step(&mut self, step: S, constraint: Constraint<V, C>) {
        self.steps.borrow_mut().push(step);
        self.constraints.borrow_mut().push(constraint);
        let steps = self.steps.borrow();
        let constraints = self.constraints.borrow();
        self.inspector.inspect(&steps, &constraints);
    }
}

pub struct AggregatorStepInspector<S, V, C> {
    steps: RRef<Vec<S>>,
    constraints: RRef<Vec<Constraint<V, C>>>,
}

impl<S, V, C> AggregatorStepInspector<S, V, C> {
    pub fn steps(&self) -> RefView<Vec<S>> {
        RefView::new(self.steps.clone())
    }

    pub fn constraints(&self) -> RefView<Vec<Constraint<V, C>>> {
        RefView::new(self.constraints.clone())
    }
}

impl<S, V, C> Default for AggregatorStepInspector<S, V, C> {
    fn default() -> Self {
        Self {
            steps: Default::default(),
            constraints: Default::default(),
        }
    }
}

impl<S: Clone, V: Clone, C: Clone> StepInspector<S, V, C> for AggregatorStepInspector<S, V, C> {
    fn inspect(&mut self, step: &S, constraint: Constraint<&V, &C>) {
        self.steps.borrow_mut().push(step.clone());
        self.constraints.borrow_mut().push(constraint.cloned());
    }
}
