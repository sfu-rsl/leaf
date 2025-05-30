use std::collections::HashMap;

use rustc_data_structures::graph::dominators::{Dominators, dominators};
use rustc_middle::{
    mir::{BasicBlock, BasicBlocks, Body, Local, Location, TerminatorKind},
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

fn make_assignment_info<'tcx>(body: &Body<'tcx>) -> AssignmentsInfo {
    let dom = dominators(&body.basic_blocks);
    let post_doms: Vec<_> = exit_points(&body.basic_blocks)
        .map(|e| PostDominators::build(&body.basic_blocks, e))
        .collect();

    let mut assignments_by_local: HashMap<Local, Vec<Location>> = HashMap::new();
    let mut dest_locals = Vec::new();
    let mut bb_map = Vec::new();

    for (loc, dest, _) in assignment_ids_split_agnostic(body) {
        bb_map.push(loc.block.as_u32());
        // Conservatively overapproximate over the local
        assignments_by_local
            .entry(dest.local)
            .or_default()
            .push(loc);
        dest_locals.push((loc, dest.local));
    }

    log_debug!("In body: {:?}", body.source.def_id());
    let alternatives = dest_locals
        .into_iter()
        .map(|(loc, local)| {
            log_debug!("Checking for alternatives of {:?} @ {:?}", local, loc);
            there_are_alternatives_for(loc, &assignments_by_local[&local], &dom, &post_doms)
        })
        .collect();

    AssignmentsInfo {
        bb_map,
        alternatives,
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

#[tracing::instrument(level = "debug", skip(dom, post_doms), ret)]
fn there_are_alternatives_for(
    this: Location,
    all_assignments: &[Location],
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

    all_assignments.iter().any(|a| is_a_sign(a.block))
}
