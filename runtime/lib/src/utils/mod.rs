use core::{
    borrow::{Borrow, BorrowMut},
    fmt::Display,
    ops::{Deref, DerefMut, RangeBounds},
};

use derive_more as dm;

pub mod alias;
pub mod file;
pub mod logging;
pub mod meta;

pub use alias::RRef;

/// A trait for any hierarchical structure with a parent from the self type.
pub trait InPlaceSelfHierarchical {
    fn add_layer(&mut self);

    fn drop_layer(&mut self) -> Option<Self>
    where
        Self: Sized;
}

/// Guards a RefCell from mutable borrows.
#[derive(Clone, dm::From)]
pub struct RefView<T>(RRef<T>);

impl<T> RefView<T> {
    pub fn new(data: RRef<T>) -> Self {
        Self(data)
    }

    pub fn borrow(&self) -> impl Deref<Target = T> + '_ {
        self.0.as_ref().borrow()
    }

    pub fn borrow_map<'a, U: 'a>(
        &'a self,
        f: impl FnOnce(&T) -> &U,
    ) -> impl Deref<Target = U> + 'a {
        std::cell::Ref::map(self.0.as_ref().borrow(), f)
    }
}

pub trait RangeIntersection<T: PartialOrd> {
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

pub fn byte_offset_from<T: Sized>(at: *const T, base: *const T) -> usize {
    at.addr() - base.addr()
}

pub trait HasIndex {
    fn index(&self) -> usize;
}

#[derive(Clone, Copy, Debug, dm::Deref, dm::From, serde::Serialize)]
pub struct Indexed<T> {
    #[deref]
    pub value: T,
    pub index: usize,
}

impl<T> HasIndex for Indexed<T> {
    fn index(&self) -> usize {
        self.index
    }
}

impl<T> Borrow<T> for Indexed<T> {
    fn borrow(&self) -> &T {
        &self.value
    }
}

impl<T: Display> Display for Indexed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.index, self.value)
    }
}

#[derive(dm::From)]
pub enum MutAccess<'a, T> {
    Borrowed(&'a mut T),
    Owned(T),
}

impl<'a, T> Borrow<T> for MutAccess<'a, T> {
    fn borrow(&self) -> &T {
        match self {
            MutAccess::Borrowed(t) => t,
            MutAccess::Owned(t) => t,
        }
    }
}

impl<'a, T> BorrowMut<T> for MutAccess<'a, T> {
    fn borrow_mut(&mut self) -> &mut T {
        match self {
            MutAccess::Borrowed(t) => t,
            MutAccess::Owned(t) => t,
        }
    }
}

impl<'a, T> Deref for MutAccess<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            MutAccess::Borrowed(t) => t,
            MutAccess::Owned(t) => t,
        }
    }
}

impl<'a, T> DerefMut for MutAccess<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            MutAccess::Borrowed(t) => t,
            MutAccess::Owned(t) => t,
        }
    }
}

pub trait IntTypeExt {
    fn bit_mask(bit_size: u32) -> u128;
    fn all_one(&self) -> u128;
    fn masked(&self, bit_rep: u128) -> u128;
    fn signed_masked(&self, bit_rep: u128) -> i128;
}

impl IntTypeExt for crate::abs::IntType {
    #[inline]
    fn bit_mask(bit_size: u32) -> u128 {
        u128::MAX >> (u128::BITS - bit_size)
    }

    #[inline]
    fn all_one(&self) -> u128 {
        Self::bit_mask(self.bit_size as u32)
    }

    #[inline]
    fn masked(&self, bit_rep: u128) -> u128 {
        bit_rep & Self::bit_mask(self.bit_size as u32)
    }

    #[inline]
    fn signed_masked(&self, bit_rep: u128) -> i128 {
        (bit_rep as i128) << (128 - self.bit_size) >> (128 - self.bit_size)
    }
}
