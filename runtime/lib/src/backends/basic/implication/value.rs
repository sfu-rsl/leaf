use core::{
    borrow::Borrow,
    num::NonZero,
    ops::{DerefMut, FromResidual},
};
use std::{borrow::Cow, collections::BTreeSet, rc::Rc};

use derive_more as dm;

use crate::abs::{PointerOffset, TypeSize};

use super::ConstraintId;

pub(crate) type Antecedents = NonEmptyConstraintSet;

type RangedAntecedents = (PointerOffset, NonZero<TypeSize>, Antecedents);

mod empty_guarded {
    use super::*;

    #[derive(Debug, Clone, dm::Deref)]
    pub(crate) struct NonEmptyConstraintSet(Rc<BTreeSet<ConstraintId>>);

    impl NonEmptyConstraintSet {
        pub fn new(set: BTreeSet<ConstraintId>) -> Option<Self> {
            if set.is_empty() {
                None
            } else {
                Some(Self(Rc::new(set)))
            }
        }

        pub fn from_constraint(id: ConstraintId) -> Self {
            Self::new(BTreeSet::from([id])).unwrap()
        }

        pub fn extend_with(&mut self, other: &Self) {
            if self.is_superset(other) {
                return;
            }

            Rc::make_mut(&mut self.0).extend(other.iter().copied())
        }
    }

    #[derive(Debug, Clone, dm::Deref, dm::DerefMut, dm::IntoIterator)]
    pub(crate) struct NonEmptyRefinedConstraints(Vec<RangedAntecedents>);

    impl NonEmptyRefinedConstraints {
        pub fn from_iter(iter: impl IntoIterator<Item = RangedAntecedents>) -> Option<Self> {
            let iter = iter.into_iter();
            if iter.size_hint().1 == Some(0) {
                return None;
            }
            let constraints: Vec<_> = iter.collect();
            (!constraints.is_empty()).then_some(Self(constraints))
        }

        pub fn get(self) -> Vec<RangedAntecedents> {
            self.0
        }
    }

    impl TryFrom<Vec<RangedAntecedents>> for NonEmptyRefinedConstraints {
        type Error = ();

        #[inline]
        fn try_from(value: Vec<RangedAntecedents>) -> Result<Self, Self::Error> {
            if value.is_empty() {
                Err(())
            } else {
                Ok(Self(value))
            }
        }
    }
}
use empty_guarded::{NonEmptyConstraintSet, NonEmptyRefinedConstraints};

#[derive(Debug, Clone, Default, dm::From)]
pub(crate) enum Precondition {
    #[default]
    NoneOrUnknown,
    #[from(forward)]
    Constraints(PreconditionConstraints),
}

#[derive(Debug, Clone, dm::From)]
pub(crate) enum PreconditionConstraints {
    Whole(NonEmptyConstraintSet),
    Refined(NonEmptyRefinedConstraints),
}

impl Precondition {
    pub fn is_some(&self) -> bool {
        match self {
            Self::NoneOrUnknown => false,
            Self::Constraints(..) => true,
        }
    }

    pub fn take_constraints(self) -> Option<PreconditionConstraints> {
        match self {
            Self::Constraints(constraints) => Some(constraints),
            _ => None,
        }
    }

    pub fn add_antecedents(
        &mut self,
        antecedents: Cow<Antecedents>,
        whole_size: impl FnOnce() -> TypeSize,
    ) {
        match self {
            Self::NoneOrUnknown => {
                *self = PreconditionConstraints::Whole(antecedents.into_owned()).into()
            }
            Self::Constraints(constraints) => constraints.add(antecedents, whole_size),
        }
    }

    pub fn merge<'a>(preconditions: impl IntoIterator<Item = impl Borrow<Self>>) -> Self {
        let mut iter =
            preconditions
                .into_iter()
                .flat_map(|precondition| match precondition.borrow() {
                    Self::NoneOrUnknown => None,
                    Self::Constraints(constraints) => Some(constraints.merge().into_owned()),
                });
        iter.next()
            .map(|first| {
                iter.fold(first, |mut unified, antecedents| {
                    unified.extend_with(antecedents.borrow());
                    unified
                })
            })
            .map(PreconditionConstraints::Whole)
            .into()
    }
}

impl PreconditionConstraints {
    pub fn refined(
        constraints: impl IntoIterator<Item = (PointerOffset, NonZero<TypeSize>, Antecedents)>,
    ) -> Option<Self> {
        NonEmptyRefinedConstraints::from_iter(constraints).map(Self::Refined)
    }

    pub fn expect_whole(&self) -> &NonEmptyConstraintSet {
        match self {
            Self::Whole(antecedents) => antecedents,
            Self::Refined(..) => panic!("Fine information is not expected"),
        }
    }

    pub fn merge(&self) -> Cow<NonEmptyConstraintSet> {
        match self {
            Self::Whole(antecedents) => Cow::Borrowed(antecedents),
            Self::Refined(refined) => Cow::Owned({
                let mut iter = refined.iter().map(|(_, _, antecedents)| antecedents);
                let first = iter.next().unwrap();
                iter.fold(first.clone(), |mut unified, antecedents| {
                    unified.extend_with(antecedents);
                    unified
                })
            }),
        }
    }

    pub fn add(&mut self, other: impl Borrow<Antecedents>, whole_size: impl FnOnce() -> TypeSize) {
        let other = other.borrow();
        match self {
            Self::Whole(antecedents) => antecedents.extend_with(other),
            Self::Refined(ref mut refined) => {
                let mut last_end: PointerOffset = 0;
                let whole_size = whole_size();

                let result = Vec::with_capacity(refined.len());
                let subs = core::mem::replace(refined.deref_mut(), result);
                for (sub_offset, sub_size, mut antecedents) in subs {
                    if sub_offset > last_end {
                        let gap_size = NonZero::new(sub_offset - last_end).unwrap();
                        refined.push((last_end, gap_size, other.clone()));
                    } else {
                        debug_assert!(sub_offset == last_end, "Overlapping constraints");
                    }
                    antecedents.extend_with(other);
                    refined.push((sub_offset, sub_size, antecedents));
                    last_end = sub_offset + sub_size.get();
                }

                if last_end < whole_size {
                    refined.push((
                        last_end,
                        NonZero::new(whole_size - last_end).unwrap(),
                        other.clone(),
                    ));
                } else {
                    debug_assert!(last_end == whole_size, "Overflowing constraints");
                }
            }
        }
    }

    pub fn at_loc(self, at: PointerOffset, size: NonZero<TypeSize>) -> NonEmptyRefinedConstraints {
        NonEmptyRefinedConstraints::try_from(match self {
            Self::Whole(antecedents) => {
                vec![(at, size, antecedents)]
            }
            Self::Refined(refined) => refined
                .get()
                .into_iter()
                .map(|(sub_offset, sub_size, antecedents)| {
                    debug_assert!(sub_offset + sub_size.get() <= size.into());
                    (sub_offset + at, sub_size, antecedents)
                })
                .collect::<Vec<_>>(),
        })
        .unwrap()
    }
}

impl<T: Into<PreconditionConstraints>> From<Option<T>> for Precondition {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(value) => Precondition::Constraints(value.into()),
            None => Precondition::NoneOrUnknown,
        }
    }
}

#[derive(Debug, Clone, dm::Deref)]
pub(crate) struct Implied<V> {
    pub by: Precondition,
    #[deref]
    pub value: V,
}

impl<V> Implied<V> {
    pub fn always(value: V) -> Self {
        Self {
            by: Precondition::NoneOrUnknown,
            value,
        }
    }

    pub fn by_unknown(value: V) -> Self {
        Self {
            by: Precondition::NoneOrUnknown,
            value,
        }
    }

    pub fn map_value<VTo>(self, f: impl FnOnce(V) -> VTo) -> Implied<VTo> {
        Implied {
            by: self.by,
            value: f(self.value),
        }
    }

    pub fn into_tuple(self) -> (Precondition, V) {
        (self.by, self.value)
    }
}

impl<V> Borrow<Precondition> for Implied<V> {
    fn borrow(&self) -> &Precondition {
        &self.by
    }
}
