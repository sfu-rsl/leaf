use std::{error::Error, ops::Coroutine, pin::Pin};

use serde::{Serialize, de::DeserializeOwned};

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
