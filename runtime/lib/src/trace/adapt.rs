use super::{Constraint, TraceManager};

/// Adapts a trace manager to accept other types of steps and values (contravariance).
pub(crate) struct AdapterTraceManager<
    F,
    SFrom,
    VFrom,
    CFrom,
    STo,
    VTo,
    CTo,
    M: TraceManager<STo, VTo, CTo>,
> {
    inner: M,
    adapter: F,
    _phantom: core::marker::PhantomData<(SFrom, VFrom, CFrom, STo, VTo, CTo)>,
}

impl<F, SFrom, VFrom, CFrom, STo, VTo, CTo, M: TraceManager<STo, VTo, CTo>>
    AdapterTraceManager<F, SFrom, VFrom, CFrom, STo, VTo, CTo, M>
{
    pub fn new(inner: M, adapter: F) -> Self
    where
        F: FnMut(SFrom, Constraint<VFrom, CFrom>) -> (STo, Constraint<VTo, CTo>),
    {
        Self {
            inner,
            adapter,
            _phantom: core::marker::PhantomData,
        }
    }
}

impl<F, SFrom, VFrom, CFrom, STo, VTo, CTo, M: TraceManager<STo, VTo, CTo>>
    TraceManager<SFrom, VFrom, CFrom>
    for AdapterTraceManager<F, SFrom, VFrom, CFrom, STo, VTo, CTo, M>
where
    F: FnMut(SFrom, Constraint<VFrom, CFrom>) -> (STo, Constraint<VTo, CTo>),
{
    fn notify_step(&mut self, step: SFrom, constraint: Constraint<VFrom, CFrom>) {
        let (new_step, constraint) = (self.adapter)(step, constraint);
        self.inner.notify_step(new_step, constraint);
    }
}

pub(crate) trait TraceManagerExt<STo, VTo, CTo>: TraceManager<STo, VTo, CTo> {
    fn adapt<SFrom, VFrom, CFrom, SF, VF, CF>(
        self,
        step_adaptor: SF,
        value_adaptor: VF,
        case_adaptor: CF,
    ) -> impl TraceManager<SFrom, VFrom, CFrom>
    where
        SF: FnMut(SFrom) -> STo,
        VF: FnMut(VFrom) -> VTo,
        CF: FnMut(CFrom) -> CTo,
        Self: Sized;

    fn adapt_step<SFrom, SF>(self, step_adaptor: SF) -> impl TraceManager<SFrom, VTo, CTo>
    where
        SF: FnMut(SFrom) -> STo,
        Self: Sized,
    {
        self.adapt(step_adaptor, |v| v, |c| c)
    }

    fn adapt_value<VFrom, VF>(self, value_adaptor: VF) -> impl TraceManager<STo, VFrom, CTo>
    where
        VF: FnMut(VFrom) -> VTo,
        Self: Sized,
    {
        self.adapt(|s| s, value_adaptor, |c| c)
    }
}
impl<STo, VTo, CTo, M: TraceManager<STo, VTo, CTo>> TraceManagerExt<STo, VTo, CTo> for M {
    fn adapt<SFrom, VFrom, CFrom, SF, VF, CF>(
        self,
        mut step_adaptor: SF,
        mut value_adaptor: VF,
        mut case_adaptor: CF,
    ) -> impl TraceManager<SFrom, VFrom, CFrom>
    where
        SF: FnMut(SFrom) -> STo,
        VF: FnMut(VFrom) -> VTo,
        CF: FnMut(CFrom) -> CTo,
        Self: Sized,
    {
        AdapterTraceManager::new(self, move |step, constraint| {
            (
                step_adaptor(step),
                constraint.map(&mut value_adaptor, &mut case_adaptor),
            )
        })
    }
}
