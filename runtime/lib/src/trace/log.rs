use core::fmt::Display;

use common::log_info;

use super::{Constraint, TraceManager};

pub(crate) struct LoggerTraceManager<S, V, C, M: TraceManager<S, V, C>> {
    inner: M,
    _phantom: core::marker::PhantomData<(S, V, C)>,
}

impl<S: Display, V: Display, C: Display, M: TraceManager<S, V, C>> TraceManager<S, V, C>
    for LoggerTraceManager<S, V, C, M>
{
    fn notify_step(&mut self, step: S, constraint: Constraint<V, C>) {
        log_info!("Notified about constraint {} at step {}", constraint, step,);

        self.inner.notify_step(step, constraint);
    }
}

pub(crate) trait TraceManagerExt<S, V, C>: TraceManager<S, V, C> {
    fn into_logger(self) -> LoggerTraceManager<S, V, C, Self>
    where
        Self: Sized;
}
impl<S, V, C, M: TraceManager<S, V, C>> TraceManagerExt<S, V, C> for M {
    fn into_logger(self) -> LoggerTraceManager<S, V, C, Self> {
        LoggerTraceManager {
            inner: self,
            _phantom: core::marker::PhantomData,
        }
    }
}
