use std::{ops::Coroutine, pin::Pin};

#[inline]
pub(crate) fn from_pinned_coroutine<G: Coroutine<Return = ()>>(
    coroutine: G,
) -> impl Iterator<Item = G::Yield> {
    pub struct FromCoroutine<G>(Pin<Box<G>>);

    impl<G: Coroutine<Return = ()>> Iterator for FromCoroutine<G> {
        type Item = G::Yield;

        fn next(&mut self) -> Option<Self::Item> {
            use std::ops::CoroutineState::*;
            match self.0.as_mut().resume(()) {
                Yielded(n) => Some(n),
                Complete(()) => None,
            }
        }
    }

    FromCoroutine(Box::pin(coroutine))
}

pub(crate) mod tracing {
    use tracing::Span;

    /// Iterator adapter that enters a span on each iteration.
    pub struct InstrumentedIter<I> {
        iter: I,
        span: Span,
    }

    impl<I> Iterator for InstrumentedIter<I>
    where
        I: Iterator,
    {
        type Item = I::Item;

        fn next(&mut self) -> Option<Self::Item> {
            let _enter = self.span.enter();
            self.iter.next()
        }
    }

    /// Extension trait for instrumenting iterators with a tracing span.
    pub trait IteratorInstrumentExt: Iterator + Sized {
        fn instrumented(self, span: Span) -> InstrumentedIter<Self> {
            InstrumentedIter { iter: self, span }
        }
    }

    impl<I: Iterator> IteratorInstrumentExt for I {}
}
