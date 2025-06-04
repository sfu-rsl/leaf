use core::{
    borrow::Borrow,
    num::NonZero,
    ops::{DerefMut, FromResidual},
};
use std::{borrow::Cow, collections::BTreeSet, rc::Rc};

use common::program_dep::{
    ControlDependency, ProgramDepAssignmentQuery, ProgramDependenceMap,
    rw::{LoadedProgramDepMap, read_program_dep_map},
};
use derive_more as dm;

use crate::abs::{BasicBlockLocation, PointerOffset, TypeSize};

use super::{
    AssignmentId, BasicConstraint, EnumAntecedentsResult, ImplicationInvestigator, InstanceKindId,
    alias::TraceQuerier, trace::BasicExeTraceRecorder,
};

pub(super) type BasicProgramDependenceMap = LoadedProgramDepMap;

pub(crate) fn default_program_dependence_map() -> BasicProgramDependenceMap {
    read_program_dep_map().expect("Failed to read program dependence map")
}

type ConstraintId = usize; // Step index

pub(super) type Antecedents = NonEmptyConstraintSet;
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

    #[derive(Debug, Clone, dm::Deref, dm::DerefMut)]
    pub(crate) struct NonEmptyRefinedConstraints(
        Vec<(PointerOffset, NonZero<TypeSize>, NonEmptyConstraintSet)>,
    );

    impl NonEmptyRefinedConstraints {
        pub fn new(
            constraints: Vec<(PointerOffset, NonZero<TypeSize>, NonEmptyConstraintSet)>,
        ) -> Option<Self> {
            if constraints.is_empty() {
                None
            } else {
                Some(Self(constraints))
            }
        }

        pub fn from_iter(
            iter: impl IntoIterator<Item = (PointerOffset, NonZero<TypeSize>, NonEmptyConstraintSet)>,
        ) -> Option<Self> {
            Self::new(iter.into_iter().collect())
        }

        pub fn get(self) -> Vec<(PointerOffset, NonZero<TypeSize>, NonEmptyConstraintSet)> {
            self.0
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

type RefinedConstraints = Vec<(PointerOffset, NonZero<TypeSize>, NonEmptyConstraintSet)>;

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

    pub fn at_loc(self, at: PointerOffset, size: NonZero<TypeSize>) -> RefinedConstraints {
        match self {
            Self::Whole(antecedents) => vec![(at, size, antecedents)],
            Self::Refined(refined) => refined
                .get()
                .into_iter()
                .map(|(sub_offset, sub_size, antecedents)| {
                    debug_assert!(sub_offset + sub_size.get() <= size.into());
                    (sub_offset + at, sub_size, antecedents)
                })
                .collect::<Vec<_>>(),
        }
    }
}

impl<R> FromResidual<R> for Precondition {
    fn from_residual(_residual: R) -> Self {
        Precondition::NoneOrUnknown
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

struct BasicImplicationInvestigator<Q> {
    program_dep_map: BasicProgramDependenceMap,
    trace_querier: Rc<Q>,
}

pub(super) fn default_implication_investigator<Q: TraceQuerier>(
    trace_querier: Rc<Q>,
) -> impl ImplicationInvestigator {
    BasicImplicationInvestigator {
        program_dep_map: default_program_dependence_map(),
        trace_querier,
    }
}

type Record = <BasicExeTraceRecorder as super::ExeTraceStorage>::Record;
type AssignmentLocation = (InstanceKindId, AssignmentId);

impl<Q: TraceQuerier> ImplicationInvestigator for BasicImplicationInvestigator<Q> {
    #[tracing::instrument(level = "debug", skip(self), ret)]
    fn antecedent_of_latest_assignment(
        &self,
        (body, assignment_id): AssignmentLocation,
    ) -> Option<Antecedents> {
        let assignments_info = self.program_dep_map.assignments(body).unwrap();

        if !assignments_info.alternatives_may_exist(assignment_id) {
            return None;
        }

        let bb_index = assignments_info.basic_block_index(assignment_id);

        self.control_dep_latest_at(BasicBlockLocation {
            body,
            index: bb_index,
        })
    }

    #[tracing::instrument(level = "debug", skip(self), ret)]
    fn antecedent_of_latest_enum_assignment(
        &self,
        (body, assignment_id): (InstanceKindId, AssignmentId),
    ) -> Option<EnumAntecedentsResult> {
        let assignments_info = self.program_dep_map.assignments(body).unwrap();
        let bb_index = assignments_info.basic_block_index(assignment_id);

        /* NOTE: Why don't we check alternatives for the tag?
         * Hypothesis: It is always true.
         * First, note that this function is meant to be used when the Rvalue is an enum.
         * (so does not apply to cases like returning the result of a function call).
         * Now assume that the tag has no alternatives, and there is a controller.
         * Then, the program would look like:
         * ```
         * if condition1 {
         *     VariantX(data)
         * } else if condition2 {
         *     VariantX(data)
         * } else if ... {
         *     VariantX(data)
         * }
         * ```
         * which we hypothesize is not common (using the same variant in all branches).
         */
        let tag = {
            self.control_dep_latest_at(BasicBlockLocation {
                body,
                index: bb_index,
            })
        }?;

        // Alternatives are supposed to be specialized for the enums to mean the fields of the enum.
        let fields = assignments_info
            .alternatives_may_exist(assignment_id)
            .then(|| tag.clone());

        Some(EnumAntecedentsResult { tag, fields })
    }
}

impl<Q: TraceQuerier> BasicImplicationInvestigator<Q> {
    #[tracing::instrument(level = "debug", skip(self), ret)]
    fn control_dep_latest_at(&self, loc: BasicBlockLocation) -> Option<Antecedents> {
        let cdg = self.program_dep_map.control_dependency(loc.body)?;
        let get_controllers = |block| {
            Some(cdg.controllers(block).into_iter().collect::<Vec<_>>()).filter(|cs| !cs.is_empty())
        };

        let mut controllers = get_controllers(loc.index)?;
        let (controller_step, found) =
            self.trace_querier
                .find_map_in_latest_call_of(loc.body, move |r, c| {
                    let block = r.location().index;
                    if controllers.contains(&block) {
                        if c.discr.is_symbolic() || c.discr.by.is_some() {
                            Some(true)
                        } else {
                            // Recurse
                            if let Some(t_controllers) = get_controllers(block) {
                                controllers = t_controllers;
                                None // Continue
                            } else {
                                Some(false)
                            }
                        }
                    } else {
                        None
                    }
                })?;

        found.then(|| {
            let constraint: &BasicConstraint = controller_step.as_ref();
            if constraint.discr.is_symbolic() {
                let record: &Record = controller_step.as_ref();
                Antecedents::from_constraint(record.index)
            } else if let Precondition::Constraints(constraints) = &constraint.discr.by {
                // Discriminant is always a primitive, so it won't be refined.
                constraints.expect_whole().clone()
            } else {
                unreachable!()
            }
        })
    }
}
