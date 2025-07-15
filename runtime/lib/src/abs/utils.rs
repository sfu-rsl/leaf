use core::{borrow::Borrow, fmt::Display};

use delegate::delegate;
use derive_more as dm;

use crate::utils::{HasIndex, Indexed};

use super::{BasicBlockIndex, BasicBlockLocation, HasTags, Tag};

pub(crate) trait BasicBlockLocationExt {
    fn at_basic_block(self, index: BasicBlockIndex) -> BasicBlockLocation<Self>
    where
        Self: Copy;
}

impl<T> BasicBlockLocationExt for T {
    #[inline]
    fn at_basic_block(self, index: BasicBlockIndex) -> BasicBlockLocation<Self>
    where
        Self: Copy,
    {
        BasicBlockLocation { body: self, index }
    }
}

#[derive(Clone, Debug, dm::Deref)]
pub(crate) struct Tagged<T> {
    #[deref]
    pub value: T,
    pub tags: Vec<Tag>,
}

impl<T> Borrow<T> for Tagged<T> {
    fn borrow(&self) -> &T {
        &self.value
    }
}

impl<T> HasTags for Tagged<T> {
    fn tags(&self) -> &[Tag] {
        &self.tags
    }
}

impl<T: Display> Display for Tagged<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.tags.is_empty() {
            return self.value.fmt(f);
        } else {
            write!(f, "{} #[{}]", self.value, self.tags.join(", "))
        }
    }
}

impl<T: HasIndex> HasIndex for Tagged<T> {
    delegate! {
        to self.value {
            fn index(&self) -> usize;
        }
    }
}

impl<T: HasTags> HasTags for Indexed<T> {
    delegate! {
        to self.value {
            fn tags(&self) -> &[Tag];
        }
    }
}
