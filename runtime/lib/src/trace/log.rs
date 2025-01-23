use core::fmt::Display;

use common::log_info;

use super::{Constraint, InspectionTraceManagerExt, StepInspector, TraceManager};

pub(crate) struct LoggerTraceManager<S, V, C> {
    _phantom: core::marker::PhantomData<(S, V, C)>,
}

impl<S, V, C> Default for LoggerTraceManager<S, V, C> {
    fn default() -> Self {
        Self {
            _phantom: Default::default(),
        }
    }
}

impl<S: Display, V: Display, C: Display> StepInspector<S, V, C> for LoggerTraceManager<S, V, C> {
    fn inspect(&mut self, step: &S, constraint: Constraint<&V, &C>) {
        log_info!("Notified about constraint {} at step {}", constraint, step,);
    }
}

pub(crate) trait TraceManagerExt<S, V, C>: TraceManager<S, V, C> {
    fn logged(self) -> impl TraceManager<S, V, C>
    where
        Self: Sized;
}
impl<S: Display, V: Display, C: Display, M: InspectionTraceManagerExt<S, V, C>>
    TraceManagerExt<S, V, C> for M
{
    fn logged(self) -> impl TraceManager<S, V, C>
    where
        Self: Sized,
    {
        self.inspected_by(LoggerTraceManager::default())
    }
}
