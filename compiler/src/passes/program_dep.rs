use rustc_middle::{
    mir::{Body, TerminatorKind},
    ty::TyCtxt,
};

use common::{
    log_warn,
    program_dep::{ControlDependencyGraph, PlainProgramDependenceMap},
    types::BasicBlockIndex,
};

use super::{
    CompilationPass, OverrideFlags, Storage, StorageExt, instr::assignment_ids_split_agnostic,
};
use crate::utils::{
    file::TyCtxtFileExt,
    mir::{InstanceKindExt, TyCtxtExt},
};

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

    if key.to_string().contains("2:50359") {
        log_warn!("The body: {}", tcx.pretty_mir(body));
    }

    pdm.control_dep
        .entry(key)
        .or_insert_with(|| calc_control_dep(tcx, body));
    pdm.assignment_bb_map
        .entry(key)
        .or_insert_with(|| make_assignment_index_to_bb(body));
}

fn calc_control_dep<'tcx>(_tcx: TyCtxt<'tcx>, body: &Body<'tcx>) -> ControlDependencyGraph {
    let cdg = crate::utils::control_dependence::ControlDependencies::build_many(
        &body.basic_blocks,
        body.basic_blocks
            .iter_enumerated()
            .filter_map(|(index, block)| match block.terminator().kind {
                TerminatorKind::Return => Some(index),
                _ => None,
            }),
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

fn make_assignment_index_to_bb<'tcx>(body: &Body<'tcx>) -> Vec<BasicBlockIndex> {
    assignment_ids_split_agnostic(body)
        .map(|(loc, _)| loc.block.as_u32())
        .collect()
}
