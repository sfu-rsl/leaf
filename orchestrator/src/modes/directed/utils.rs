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

pub(crate) fn cache_serialize<T: Serialize>(
    obj: &T,
    w: &mut impl std::io::Write,
) -> Result<(), impl Error + 'static> {
    bincode::serde::encode_into_std_write(obj, w, bincode::config::standard()).map(|_| {})
}

pub(crate) fn cache_deserialize<T: DeserializeOwned>(
    r: &mut impl std::io::Read,
) -> Result<T, impl Error + 'static> {
    bincode::serde::decode_from_std_read(r, bincode::config::standard())
}
