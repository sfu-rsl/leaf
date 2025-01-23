use derive_more as dm;

pub(crate) mod alias;
pub(crate) mod file;
pub(crate) mod logging;
pub(crate) mod meta;

#[derive(dm::Deref, dm::DerefMut)]
pub(crate) struct UnsafeSync<T>(T);

impl<T> UnsafeSync<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self(value)
    }
}

unsafe impl<T> Sync for UnsafeSync<T> {}

#[derive(dm::Deref, dm::DerefMut)]
pub(crate) struct UnsafeSend<T>(T);

impl<T> UnsafeSend<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self(value)
    }
}

unsafe impl<T> Send for UnsafeSend<T> {}

/// A trait for any hierarchical structure that may take a parent.
pub(crate) trait Hierarchical<T> {
    fn set_parent(&mut self, parent: T);

    fn give_back_parent(&mut self) -> Option<T>;
}

/// A trait for any hierarchical structure with a parent from the self type.
pub(crate) trait SelfHierarchical {
    fn add_layer(self) -> Self;

    fn drop_layer(self) -> Option<Self>
    where
        Self: Sized;
}

/// A trait for any hierarchical structure with a parent from the self type.
pub(crate) trait InPlaceSelfHierarchical {
    fn add_layer(&mut self);

    fn drop_layer(&mut self) -> Option<Self>
    where
        Self: Sized;
}
