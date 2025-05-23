use core::ops::{Deref, RangeBounds};

use derive_more as dm;

pub(crate) mod alias;
pub(crate) mod file;
pub(crate) mod logging;
pub(crate) mod meta;

use alias::RRef;

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

/// Guards a RefCell from mutable borrows.
#[derive(dm::From)]
pub(crate) struct RefView<T>(RRef<T>);

impl<T> RefView<T> {
    pub(crate) fn new(data: RRef<T>) -> Self {
        Self(data)
    }

    pub(crate) fn borrow(&self) -> impl Deref<Target = T> + '_ {
        self.0.borrow()
    }

    pub(crate) fn borrow_map<'a, U: 'a>(
        &'a self,
        f: impl FnOnce(&T) -> &U,
    ) -> impl Deref<Target = U> + 'a {
        std::cell::Ref::map(self.0.borrow(), f)
    }
}

pub(crate) trait RangeIntersection<T: PartialOrd> {
    fn is_overlapping(&self, other: &impl RangeBounds<T>) -> bool;

    fn contains(&self, other: &impl RangeBounds<T>) -> bool;
}

impl<T: PartialOrd, R: RangeBounds<T>> RangeIntersection<T> for R {
    fn is_overlapping(&self, other: &impl RangeBounds<T>) -> bool {
        use core::ops::Bound::*;
        let x = (self.start_bound(), self.end_bound());
        let y = (other.start_bound(), other.end_bound());
        (match (x, y) {
            ((Included(s0), _), (_, Included(e1))) => s0 <= e1,
            ((Included(s0) | Excluded(s0), _), (_, Included(e1) | Excluded(e1))) => s0 < e1,
            ((Unbounded, _), _) | (_, (_, Unbounded)) => true,
        } && match (x, y) {
            ((_, Included(e0)), (Included(s1), _)) => s1 <= e0,
            ((_, Included(e0) | Excluded(e0)), (Included(s1) | Excluded(s1), _)) => s1 < e0,
            ((_, Unbounded), _) | (_, (Unbounded, _)) => true,
        })
    }

    fn contains(&self, other: &impl RangeBounds<T>) -> bool {
        use core::ops::Bound::*;
        let x = (self.start_bound(), self.end_bound());
        let y = (other.start_bound(), other.end_bound());
        (match (x, y) {
            ((Excluded(s0), _), (Included(s1), _)) => s0 < s1,
            ((Included(s0) | Excluded(s0), _), (Included(s1) | Excluded(s1), _)) => s0 <= s1,
            ((Unbounded, _), _) => true,
            (_, (Unbounded, _)) => false,
        } && match (x, y) {
            ((_, Excluded(e0)), (_, Included(e1))) => e0 > e1,
            ((_, Included(e0) | Excluded(e0)), (_, Included(e1) | Excluded(e1))) => e0 >= e1,
            ((_, Unbounded), _) => true,
            (_, (_, Unbounded)) => false,
        })
    }
}
