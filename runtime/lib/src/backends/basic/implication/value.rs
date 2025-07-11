use core::{
    borrow::Borrow,
    num::NonZero,
    ops::{Deref, DerefMut},
};
use std::borrow::Cow;

use cfg_if::cfg_if;
use derive_more as dm;

use crate::abs::{PointerOffset, TypeSize};

use super::ConstraintId;

pub(crate) type Antecedents = NonEmptyConstraints;

type RangedAntecedents = (PointerOffset, NonZero<TypeSize>, Antecedents);

mod empty_guarded {
    use std::{collections::BTreeSet, rc::Rc};

    use super::*;

    #[derive(Debug, Clone, dm::Deref)]
    pub(crate) struct NonEmptyConstraints(Rc<BTreeSet<ConstraintId>>);

    impl NonEmptyConstraints {
        fn new(set: BTreeSet<ConstraintId>) -> Option<Self> {
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
use empty_guarded::{NonEmptyConstraints, NonEmptyRefinedConstraints};

#[derive(Debug, Clone, dm::From)]
pub(crate) enum PreconditionConstraints {
    Whole(NonEmptyConstraints),
    Refined(NonEmptyRefinedConstraints),
}

impl PreconditionConstraints {
    pub fn refined(
        constraints: impl IntoIterator<Item = (PointerOffset, NonZero<TypeSize>, Antecedents)>,
    ) -> Option<Self> {
        NonEmptyRefinedConstraints::from_iter(constraints).map(Self::Refined)
    }

    pub fn expect_whole(&self) -> &NonEmptyConstraints {
        match self {
            Self::Whole(antecedents) => antecedents,
            Self::Refined(..) => panic!("Fine information is not expected"),
        }
    }

    pub fn merge(&self) -> Cow<NonEmptyConstraints> {
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

    pub fn add(&mut self, other: Cow<Antecedents>, whole_size: impl FnOnce() -> TypeSize) {
        match self {
            Self::Whole(antecedents) => antecedents.extend_with(&other),
            Self::Refined(ref mut refined) => {
                let mut last_end: PointerOffset = 0;
                let whole_size = whole_size();

                let result = Vec::with_capacity(refined.len());
                let subs = core::mem::replace(refined.deref_mut(), result);
                for (sub_offset, sub_size, mut antecedents) in subs {
                    if sub_offset > last_end {
                        let gap_size = NonZero::new(sub_offset - last_end).unwrap();
                        refined.push((last_end, gap_size, other.deref().clone()));
                    } else {
                        debug_assert!(sub_offset == last_end, "Overlapping constraints");
                    }
                    antecedents.extend_with(&other);
                    refined.push((sub_offset, sub_size, antecedents));
                    last_end = sub_offset + sub_size.get();
                }

                if last_end < whole_size {
                    refined.push((
                        last_end,
                        NonZero::new(whole_size - last_end).unwrap(),
                        other.into_owned(),
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

cfg_if! {
    if #[cfg(feature = "implicit_flow")] {
        type PreconditionImpl = enabled::Precondition;
    } else {
        type PreconditionImpl = disabled::Precondition;
    }
}

pub(crate) trait ImpliedValue {
    type Precondition: PreconditionConstruct + PreconditionQuery;
}

pub(crate) trait PreconditionConstruct: Default {
    fn always() -> Self;
    fn unknown() -> Self;
    fn new(constraints: Option<impl Into<PreconditionConstraints>>) -> Self;

    fn merge<'a>(preconditions: impl IntoIterator<Item = impl Borrow<Self>>) -> Self;

    fn add(self, antecedents: Cow<Antecedents>, whole_size: impl FnOnce() -> TypeSize) -> Self;
}

pub(crate) trait PreconditionQuery {
    fn is_some(&self) -> bool;

    fn constraints(&self) -> Option<&PreconditionConstraints>;

    fn take_constraints(self) -> Option<PreconditionConstraints>;
}

/* NOTE: It would be neater if P was first.
 * But V is more of a parameter than P. P is kept as generic for better abstraction and maintainability.
 * It is supposed to be fixed based on the feature enabled or not. */
#[derive(Debug, Clone, dm::Deref)]
pub(crate) struct Implied<V, P = PreconditionImpl> {
    pub by: P,
    #[deref]
    pub value: V,
}

impl<V, P> Implied<V, P> {
    pub fn always(value: V) -> Self
    where
        P: PreconditionConstruct,
    {
        Self {
            by: P::always(),
            value,
        }
    }

    pub fn by_unknown(value: V) -> Self
    where
        P: PreconditionConstruct,
    {
        Self {
            by: P::unknown(),
            value,
        }
    }

    pub fn map_value<VTo>(self, f: impl FnOnce(V) -> VTo) -> Implied<VTo, P> {
        Implied {
            by: self.by,
            value: f(self.value),
        }
    }

    pub fn into_tuple(self) -> (P, V) {
        (self.by, self.value)
    }

    pub fn add_antecedents(
        &mut self,
        antecedents: Cow<Antecedents>,
        whole_size: impl FnOnce() -> TypeSize,
    ) where
        P: PreconditionConstruct,
    {
        self.by = core::mem::take(&mut self.by).add(antecedents, whole_size);
    }
}

impl<V, P> ImpliedValue for Implied<V, P>
where
    P: PreconditionConstruct + PreconditionQuery,
{
    type Precondition = P;
}

mod enabled {

    use super::*;

    #[derive(Debug, Clone, Default, dm::From)]
    pub(crate) enum Precondition {
        #[default]
        NoneOrUnknown,
        #[from(forward)]
        Constraints(PreconditionConstraints),
    }

    impl PreconditionConstruct for Precondition {
        fn always() -> Self {
            Precondition::NoneOrUnknown
        }

        fn unknown() -> Self {
            Precondition::NoneOrUnknown
        }

        fn new(constraints: Option<impl Into<PreconditionConstraints>>) -> Self {
            match constraints {
                Some(antecedents) => Precondition::Constraints(antecedents.into()),
                None => Precondition::NoneOrUnknown,
            }
        }

        fn merge<'a>(preconditions: impl IntoIterator<Item = impl Borrow<Self>>) -> Self {
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

        fn add(self, antecedents: Cow<Antecedents>, whole_size: impl FnOnce() -> TypeSize) -> Self {
            match self {
                Self::NoneOrUnknown => {
                    PreconditionConstraints::Whole(antecedents.into_owned()).into()
                }
                Self::Constraints(mut constraints) => {
                    constraints.add(antecedents, whole_size);
                    Self::Constraints(constraints)
                }
            }
        }
    }

    impl PreconditionQuery for Precondition {
        fn is_some(&self) -> bool {
            match self {
                Self::NoneOrUnknown => false,
                Self::Constraints(..) => true,
            }
        }

        fn constraints(&self) -> Option<&PreconditionConstraints> {
            match self {
                Self::NoneOrUnknown => None,
                Self::Constraints(constraints) => Some(constraints),
            }
        }

        fn take_constraints(self) -> Option<PreconditionConstraints> {
            match self {
                Self::Constraints(constraints) => Some(constraints),
                _ => None,
            }
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

    impl<V> Borrow<Precondition> for Implied<V, Precondition> {
        fn borrow(&self) -> &Precondition {
            &self.by
        }
    }

    mod serdes {
        use serde::Serialize;

        use super::Precondition;

        impl Serialize for Precondition {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                match self {
                    Precondition::NoneOrUnknown => serializer.serialize_none(),
                    Precondition::Constraints(constraints) => {
                        constraints.expect_whole().serialize(serializer)
                    }
                }
            }
        }
    }
}

mod disabled {
    use super::*;

    pub(crate) type Precondition = ();

    impl PreconditionConstruct for Precondition {
        fn always() -> Self {
            ()
        }

        fn unknown() -> Self {
            ()
        }

        fn merge<'a>(_preconditions: impl IntoIterator<Item = impl Borrow<Self>>) -> Self {
            ()
        }

        fn add(
            self,
            _antecedents: Cow<Antecedents>,
            _whole_size: impl FnOnce() -> TypeSize,
        ) -> Self {
            self
        }

        fn new(_constraints: Option<impl Into<PreconditionConstraints>>) -> Self {
            ()
        }
    }

    impl PreconditionQuery for Precondition {
        fn is_some(&self) -> bool {
            false
        }

        fn constraints(&self) -> Option<&PreconditionConstraints> {
            None
        }

        fn take_constraints(self) -> Option<PreconditionConstraints> {
            None
        }
    }

    impl<V> Borrow<Precondition> for Implied<V, Precondition> {
        fn borrow(&self) -> &Precondition {
            &self.by
        }
    }
}
