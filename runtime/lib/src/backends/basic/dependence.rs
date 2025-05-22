use core::borrow::Borrow;
use std::{collections::BTreeSet, rc::Rc};

use derive_more as dm;
use set::NonEmptyConstraintSet;

pub(super) type BasicProgramDependenceMap = common::program_dep::rw::LoadedProgramDepMap;

pub(crate) fn default_program_dependence_map() -> BasicProgramDependenceMap {
    common::program_dep::rw::read_program_dep_map().expect("Failed to read program dependence map")
}

type ConstraintId = usize; // Step index

mod set {
    use super::*;

    #[derive(Debug, Clone, dm::Deref)]
    pub(super) struct NonEmptyConstraintSet(Rc<BTreeSet<ConstraintId>>);

    impl NonEmptyConstraintSet {
        pub fn new(set: BTreeSet<ConstraintId>) -> Option<Self> {
            if set.is_empty() {
                None
            } else {
                Some(Self(Rc::new(set)))
            }
        }

        pub fn from_iter(iter: impl IntoIterator<Item = ConstraintId>) -> Option<Self> {
            let set: BTreeSet<_> = iter.into_iter().collect();
            Self::new(set)
        }

        pub fn extend_with(&mut self, other: &Self) {
            if self.is_superset(other) {
                return;
            }

            Rc::make_mut(&mut self.0).extend(other.iter().copied())
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) enum Precondition {
    None,
    #[default]
    Unknown,
    Constraints(NonEmptyConstraintSet),
}

impl Precondition {
    pub fn from_constraints(ids: impl IntoIterator<Item = ConstraintId>) -> Self {
        NonEmptyConstraintSet::from_iter(ids)
            .map(Self::Constraints)
            .unwrap_or(Self::None)
    }

    pub fn from_constraint(id: ConstraintId) -> Self {
        Self::from_constraints(core::iter::once(id))
    }

    pub fn is_some(&self) -> bool {
        use Precondition::*;
        match self {
            None | Unknown => false,
            Constraints(..) => true,
        }
    }

    pub fn merge(preconditions: impl IntoIterator<Item = impl Borrow<Self>>) -> Self {
        preconditions
            .into_iter()
            .fold(Precondition::None, |mut result, cond| {
                result.add_info(cond.borrow());
                result
            })
    }

    pub fn add_info(&mut self, other: &Self) {
        use Precondition::*;
        match (self, other) {
            (this @ (None | Unknown), _) => other.clone_into(this),
            (_, None | Unknown) => (),
            (Constraints(a), Constraints(b)) => a.extend_with(b),
        }
    }
}

#[derive(Debug, Clone, dm::Deref)]
pub(super) struct Implied<V> {
    pub by: Precondition,
    #[deref]
    pub value: V,
}

impl<V> Implied<V> {
    pub fn always(value: V) -> Self {
        Implied {
            by: Precondition::None,
            value,
        }
    }

    pub fn by_unknown(value: V) -> Self {
        Implied {
            by: Precondition::Unknown,
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
