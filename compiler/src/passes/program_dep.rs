use std::collections::HashMap;

use rustc_abi::VariantIdx;
use rustc_data_structures::graph::dominators::{Dominators, dominators};
use rustc_middle::{
    mir::{
        AggregateKind, BasicBlock, BasicBlocks, Body, Local, Location, ProjectionElem, Rvalue,
        StatementKind, TerminatorKind,
    },
    ty::TyCtxt,
};

use common::{
    log_debug,
    program_dep::{AssignmentsInfo, ControlDependencyGraph, PlainProgramDependenceMap},
};

use super::{
    CompilationPass, OverrideFlags, Storage, StorageExt, instr::assignment_ids_split_agnostic,
};
use crate::utils::{control_dependence::PostDominators, file::TyCtxtFileExt, mir::InstanceKindExt};

#[derive(Default)]
pub(crate) struct ProgramDependenceMapExporter;

const KEY_MAP: &str = "program_dep";

impl CompilationPass for ProgramDependenceMapExporter {
    fn override_flags() -> super::OverrideFlags {
        OverrideFlags::OPTIMIZED_MIR
            | OverrideFlags::EXTERN_OPTIMIZED_MIR
            | OverrideFlags::MIR_SHIMS
            | OverrideFlags::MAKE_CODEGEN_BACKEND
    }

    fn visit_mir_body_before<'tcx>(
        tcx: TyCtxt<'tcx>,
        body: &Body<'tcx>,
        storage: &mut dyn Storage,
    ) {
        let mut pdm = storage.get_or_default::<PlainProgramDependenceMap>(KEY_MAP.to_owned());
        visit_and_add(&mut pdm, tcx, body);
    }

    fn visit_tcx_at_codegen_after(&mut self, tcx: TyCtxt, storage: &mut dyn Storage) {
        let mut pdm = storage.get_or_default::<PlainProgramDependenceMap>(KEY_MAP.to_owned());

        tcx.collect_and_partition_mono_items(())
            .codegen_units
            .iter()
            .flat_map(|unit| unit.items())
            .flat_map(|(item, _)| match item {
                rustc_middle::mir::mono::MonoItem::Fn(instance) => Some(instance.def),
                _ => None,
            })
            .for_each(|instance_kind| {
                // Fetching `instance_mir` should cause a call to `visit_mir_body_before`, but anyway.
                visit_and_add(&mut pdm, tcx, tcx.instance_mir(instance_kind));
            });

        common::program_dep::rw::write_program_dep_map(&pdm, tcx.output_dir())
            .expect("Failed to write program dependence map");
    }
}

fn visit_and_add<'tcx>(pdm: &mut PlainProgramDependenceMap, tcx: TyCtxt<'tcx>, body: &Body<'tcx>) {
    let instance_kind = body.source.instance;
    let key = instance_kind.to_plain_id();

    pdm.control_dep
        .entry(key)
        .or_insert_with(|| calc_control_dep(tcx, body));
    pdm.assignments
        .entry(key)
        .or_insert_with(|| make_assignment_info(body));
}

fn calc_control_dep<'tcx>(_tcx: TyCtxt<'tcx>, body: &Body<'tcx>) -> ControlDependencyGraph {
    let cdg = crate::utils::control_dependence::ControlDependencies::build_many(
        &body.basic_blocks,
        exit_points(&body.basic_blocks),
    );

    body.basic_blocks
        .iter_enumerated()
        .flat_map(|(block, _)| {
            let nodes = cdg
                .dependent_on(block)
                .iter()
                .flat_map(|s| s.iter())
                .map(|b| b.as_u32())
                .collect::<Vec<_>>();
            (!nodes.is_empty()).then_some((block.as_u32(), nodes))
        })
        .collect()
}

type DirectAssignmentClass = (Local, Option<VariantIdx>);

enum LValue {
    Direct(DirectAssignmentClass),
    Deref,
}

fn make_assignment_info<'tcx>(body: &Body<'tcx>) -> AssignmentsInfo {
    let dom = dominators(&body.basic_blocks);
    let post_doms: Vec<_> = exit_points(&body.basic_blocks)
        .map(|e| PostDominators::build(&body.basic_blocks, e))
        .collect();

    let mut bb_map = Vec::new();

    let mut assignments_grouped: HashMap<DirectAssignmentClass, Vec<Location>> = HashMap::new();

    let mut left_values = Vec::new();
    let mut observed_variants: HashMap<Local, Vec<VariantIdx>> = HashMap::new();

    for (loc, lhs, _) in assignment_ids_split_agnostic(body) {
        bb_map.push(loc.block.as_u32());

        let left_val = if lhs
            .iter_projections()
            .last()
            .is_some_and(|p| matches!(p.1, ProjectionElem::Deref))
        {
            LValue::Deref
        } else {
            // Conservatively overapproximate over the local
            // FIXME: This may cause large imprecisions if large objects (with many fields) are used.
            let class = (lhs.local, variant_index_set_at(body, loc));
            if let Some(variant_index) = class.1 {
                observed_variants
                    .entry(class.0)
                    .or_default()
                    .push(variant_index);
            }
            assignments_grouped.entry(class).or_default().push(loc);
            LValue::Direct(class)
        };
        left_values.push((loc, left_val));
    }

    log_debug!("In body: {:?}", body.source.def_id());
    let has_alternatives = check_alternatives(
        dom,
        post_doms,
        assignments_grouped,
        left_values,
        observed_variants,
    );

    AssignmentsInfo {
        bb_map,
        has_alternatives,
    }
}

fn exit_points<'a, 'tcx>(
    basic_blocks: &'a BasicBlocks<'tcx>,
) -> impl Iterator<Item = BasicBlock> + use<'a, 'tcx> {
    basic_blocks
        .iter_enumerated()
        .filter_map(|(index, block)| match block.terminator().kind {
            TerminatorKind::Return => Some(index),
            _ => None,
        })
}

fn check_alternatives(
    dom: Dominators<BasicBlock>,
    post_doms: Vec<PostDominators<BasicBlock>>,
    assignments_grouped: HashMap<(Local, Option<VariantIdx>), Vec<Location>>,
    left_values: Vec<(Location, LValue)>,
    observed_variants: HashMap<Local, Vec<VariantIdx>>,
) -> Vec<bool> {
    left_values
        .into_iter()
        .map(|(loc, left_val)| {
            match left_val {
                LValue::Deref => {
                    // Deref assignments are always considered to have alternatives.
                    // Look at the first case in `there_are_alternatives_for`.
                    return true;
                }
                LValue::Direct(class) => {
                    log_debug!("Checking for alternatives of {:?} @ {:?}", class, loc);
                    let assignments_in_class = assignments_grouped[&class].iter().cloned();

                    // The variant of a non-aggregate assignment (like return values) can be anything.
                    let other_assignments: Box<dyn Iterator<Item = Location>> = if class.1.is_some()
                    {
                        Box::new(
                            assignments_grouped
                                .get(&(class.0, None))
                                .map(|v| v.iter().cloned())
                                .into_iter()
                                .flatten(),
                        )
                    } else if let Some(variants) = observed_variants.get(&class.0) {
                        Box::new(variants.iter().flat_map(|v| {
                            assignments_grouped
                                .get(&(class.0, Some(*v)))
                                .map(|v| v.iter().cloned())
                                .into_iter()
                                .flatten()
                        }))
                    } else {
                        Box::new(core::iter::empty())
                    };
                    there_are_alternatives_for(
                        loc,
                        assignments_in_class.chain(other_assignments),
                        &dom,
                        &post_doms,
                    )
                }
            }
        })
        .collect()
}

#[tracing::instrument(level = "debug", skip(related_assignments, dom, post_doms), ret)]
fn there_are_alternatives_for(
    this: Location,
    mut related_assignments: impl Iterator<Item = Location>,
    dom: &Dominators<BasicBlock>,
    post_doms: &[PostDominators<BasicBlock>],
) -> bool {
    let this = this.block;

    let dominates = |a, b| dom.is_reachable(b) && dom.dominates(a, b);
    let post_dominates = |a, b| {
        post_doms
            .iter()
            .all(|p| p.is_reachable(b) && p.post_dominates(a, b))
    };

    let is_a_sign = |other| {
        /* NOTE: How reliable is it?
         * First, we don't want to have false negatives so some false positives
         * are possible.
         * Second, we want to decide it at the point of this assignment (not for the destination everywhere).
         * If two assignments are part of an SESE (they are guaranteed to run consecutively),
         * then that does not count for sure. Alternatives only happen when this is not holding.
         */

        /* (Counts)
         * let a = 0;
         * if x {
         *     a += 2;
         * }
         */
        if dominates(other, this) {
            // If this doesn't post-dominate the other, then presence of it is affecting the value.
            return !post_dominates(this, other);
        }

        /* (Counts)
         * if x {
         *     a = 2;
         * } else {
         *     a = 1;
         * }
         */
        /* (Doesn't Count)
         * if x {
         *     a = 1;
         * }
         * a = 2;
         */
        /* NOTE: How about this case?
         * if x {
         *     a = 1;
         * }
         * a += 1;
         * Rust requires `a` to be assigned before being read.
         * So there is an initial assignment, and it falls under the first check.
         */
        if !dominates(this, other) {
            return !post_dominates(other, this);
        }

        // This is dominating the other, so the other always has a constant image of this.
        false
    };

    related_assignments.any(|a| is_a_sign(a.block))
}

/// Returns the variant index of the assignment at the given location, if known.
fn variant_index_set_at<'tcx>(body: &Body<'tcx>, assignment_loc: Location) -> Option<VariantIdx> {
    body.stmt_at(assignment_loc)
        .left()
        .and_then(|stmt| match stmt.kind {
            StatementKind::Assign(box (
                _,
                Rvalue::Aggregate(box AggregateKind::Adt(_, variant_index, _, _, _), _),
            )) => Some(variant_index),
            StatementKind::SetDiscriminant { variant_index, .. } => Some(variant_index),
            _ => None,
        })
}
