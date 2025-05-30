use core::borrow::Borrow;
use std::{collections::BTreeSet, ops::FromResidual, rc::Rc};

use common::{
    program_dep::{
        ControlDependency, ProgramDepAssignmentQuery, ProgramDependenceMap,
        rw::{LoadedProgramDepMap, read_program_dep_map},
    },
    types::{BasicBlockIndex, BasicBlockLocation},
};
use constraint_set::NonEmptyConstraintSet;
use derive_more as dm;

use super::{
    AssignmentId, BasicConstraint, ImplicationInvestigator, InstanceKindId, alias::TraceQuerier,
    trace::BasicExeTraceRecorder,
};

pub(super) type BasicProgramDependenceMap = LoadedProgramDepMap;

pub(crate) fn default_program_dependence_map() -> BasicProgramDependenceMap {
    read_program_dep_map().expect("Failed to read program dependence map")
}

type ConstraintId = usize; // Step index

mod constraint_set {
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
pub(crate) enum Precondition {
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

impl<R> FromResidual<R> for Precondition {
    fn from_residual(_residual: R) -> Self {
        Precondition::None
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
    fn antecedent_of_latest_assignment(
        &self,
        (body, assignment_id): AssignmentLocation,
    ) -> Precondition {
        let assignments_info = self.program_dep_map.assignments(body).unwrap();

        if !assignments_info.alternatives_may_exist(assignment_id) {
            return Precondition::None;
        }

        let bb_index = assignments_info.basic_block_index(assignment_id);

        self.control_dep_latest_at(BasicBlockLocation {
            body,
            index: bb_index,
        })
    }
}

impl<Q: TraceQuerier> BasicImplicationInvestigator<Q> {
    fn control_dep_latest_at(&self, loc: BasicBlockLocation) -> Precondition {
        let controllers = self.controllers(loc).filter(|cs| !cs.is_empty())?;

        let controller_step = self
            .trace_querier
            .find_in_latest_call_of(loc.body, move |r, _| {
                controllers.contains(&r.location().index)
            })?;

        let constraint: &BasicConstraint = controller_step.as_ref();

        if constraint.discr.is_symbolic() {
            let record: &Record = controller_step.as_ref();
            Precondition::from_constraint(record.index)
        } else if matches!(constraint.discr.by, Precondition::Constraints(..)) {
            constraint.discr.by.clone()
        } else {
            Precondition::None
        }
    }

    fn controllers(&self, loc: BasicBlockLocation) -> Option<Vec<BasicBlockIndex>> {
        let cdg = self.program_dep_map.control_dependency(loc.body)?;
        Some(cdg.controllers(loc.index).into_iter().collect::<Vec<_>>())
    }
}
