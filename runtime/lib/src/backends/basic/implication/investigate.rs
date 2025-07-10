use std::rc::Rc;

use common::program_dep::{
    ControlDependency, ProgramDepAssignmentQuery, ProgramDependenceMap,
    rw::{LoadedProgramDepMap, read_program_dep_map},
};

use crate::{abs::BasicBlockLocation, utils::HasIndex};

use crate::backends::basic as backend;
use backend::{
    AssignmentId, BasicConstraint, EnumAntecedentsResult, ImplicationInvestigator, InstanceKindId,
    TraceQuerier,
};

use super::{Antecedents, Precondition};

type BasicProgramDependenceMap = LoadedProgramDepMap;

fn default_program_dependence_map() -> BasicProgramDependenceMap {
    read_program_dep_map().expect("Failed to read program dependence map")
}

struct BasicImplicationInvestigator<Q> {
    program_dep_map: BasicProgramDependenceMap,
    trace_querier: Rc<Q>,
}

pub(crate) fn default_implication_investigator<Q: TraceQuerier>(
    trace_querier: Rc<Q>,
) -> impl ImplicationInvestigator {
    BasicImplicationInvestigator {
        program_dep_map: default_program_dependence_map(),
        trace_querier,
    }
}

type AssignmentLocation = (InstanceKindId, AssignmentId);

impl<Q: TraceQuerier> ImplicationInvestigator for BasicImplicationInvestigator<Q> {
    #[tracing::instrument(level = "debug", skip(self), ret)]
    fn antecedent_of_latest_assignment(
        &self,
        (body, assignment_id): AssignmentLocation,
    ) -> Option<Antecedents> {
        let assignments_info = self.program_dep_map.assignments(body)?;

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
        let assignments_info = self.program_dep_map.assignments(body)?;
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

        if !self
            .trace_querier
            .any_sym_dependent_in_current_call(loc.body)
        {
            return None;
        }

        let get_controllers = |block| {
            Some(cdg.controllers(block).into_iter().collect::<Vec<_>>()).filter(|cs| !cs.is_empty())
        };

        let mut controllers = get_controllers(loc.index)?;
        let (controller_step, found) =
            self.trace_querier
                .find_map_in_current_func(loc.body, move |block, c| {
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
                Antecedents::from_constraint(controller_step.index())
            } else if let Precondition::Constraints(constraints) = &constraint.discr.by {
                // Discriminant is always a primitive, so it won't be refined.
                constraints.expect_whole().clone()
            } else {
                unreachable!()
            }
        })
    }
}
