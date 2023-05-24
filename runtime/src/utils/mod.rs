pub(crate) mod logging;

use std::ops::Deref;

pub(crate) struct UnsafeSync<T>(T);

unsafe impl<T> Sync for UnsafeSync<T> {}

impl<T> UnsafeSync<T> {
    pub fn new(obj: T) -> Self {
        Self(obj)
    }
}

impl<T> Deref for UnsafeSync<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}