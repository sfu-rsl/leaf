use rustc_middle::{
    mir::{Body, HasLocalDecls},
    ty::TyCtxt,
};

use common::directed::{BasicBlockIndex, ControlFlowGraph, DefId, ProgramMap};

use super::{CompilationPass, Storage, StorageExt};
use crate::utils::file::TyCtxtFileExt;

type Calls = Vec<(BasicBlockIndex, DefId)>;
type InterestingBlocks = Vec<u32>;

#[derive(Default)]
pub(crate) struct ProgramMapExporter;

const KEY_MAP: &str = "program_map";

const FILE_OUTPUT: &str = "program_map.json";

impl CompilationPass for ProgramMapExporter {
    fn override_flags() -> super::OverrideFlags {
        super::OverrideFlags::MAKE_CODEGEN_BACKEND
    }

    fn visit_mir_body_before<'tcx>(
        tcx: TyCtxt<'tcx>,
        body: &Body<'tcx>,
        storage: &mut dyn Storage,
    ) {
        let mut p_map = storage.get_or_default::<ProgramMap>(KEY_MAP.to_owned());
        visit_and_add(&mut p_map, tcx, body.source.def_id(), body);
    }

    fn visit_tcx_at_codegen_after(&mut self, tcx: TyCtxt, storage: &mut dyn Storage) {
        let mut p_map = storage.get_or_default::<ProgramMap>(KEY_MAP.to_owned());

        tcx.collect_and_partition_mono_items(())
            .codegen_units
            .iter()
            .flat_map(|unit| unit.items())
            .flat_map(|(item, _)| match item {
                rustc_middle::mir::mono::MonoItem::Fn(instance) => Some(instance.def),
                _ => None,
            })
            .map(|instance| (instance.def_id(), tcx.instance_mir(instance)))
            .for_each(|(def_id, body)| {
                visit_and_add(&mut p_map, tcx, def_id, body);
            });

        p_map.entry_points.extend(
            tcx.entry_fn(())
                .iter()
                .map(|(def_id, _)| map_def_id(*def_id)),
        );

        p_map
            .write(tcx.output_dir().join(FILE_OUTPUT))
            .expect("Failed to write program map");
    }
}

fn visit_and_add<'tcx>(
    p_map: &mut ProgramMap,
    tcx: TyCtxt<'tcx>,
    def_id: rustc_hir::def_id::DefId,
    body: &Body<'tcx>,
) {
    let key = map_def_id(def_id);
    if p_map.cfgs.contains_key(&key) {
        return;
    }

    let data = visit_body(tcx, &body);
    p_map.cfgs.insert(key, data.0);
    p_map.call_graph.insert(key, data.1);
    p_map
        .debug_info
        .func_names
        .insert(key, tcx.def_path_str(def_id));
}

fn visit_body<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
) -> (ControlFlowGraph, Calls, InterestingBlocks) {
    let mut cfg = ControlFlowGraph::new();
    let mut calls = Calls::new();
    let mut interesting_blocks = InterestingBlocks::new();

    for (index, block) in body.basic_blocks.iter_enumerated() {
        use rustc_middle::mir::TerminatorKind::*;
        match &block.terminator().kind {
            FalseEdge {
                real_target: target,
                imaginary_target: _,
            }
            | FalseUnwind {
                real_target: target,
                unwind: _,
            }
            | Goto { target } => {
                cfg.insert(index.as_u32(), vec![(target.as_u32(), None)]);
            }
            SwitchInt { discr: _, targets } => {
                let mut successors: Vec<(u32, Option<(usize, Option<u128>)>)> = Vec::new();
                for (target_index, target) in targets.iter().enumerate() {
                    successors.push((target.1.as_u32(), Some((target_index, Some(target.0)))));
                }
                if !body.basic_blocks[targets.otherwise()].is_empty_unreachable() {
                    successors.push((targets.otherwise().as_u32(), Some((usize::MAX, None))));
                }
                cfg.insert(index.as_u32(), successors);
            }
            UnwindResume | UnwindTerminate(_) | Return | Unreachable | CoroutineDrop => {}
            Drop {
                target, unwind: _, ..
            }
            | Assert {
                target, unwind: _, ..
            } => {
                // TODO: Handle unwind
                cfg.insert(index.as_u32(), vec![(target.as_u32(), None)]);
            }
            Call {
                func,
                destination,
                target,
                ..
            } => {
                let is_never = destination.ty(body, tcx).ty.is_never();
                use rustc_type_ir::TyKind::*;
                match func.ty(body.local_decls(), tcx).kind() {
                    FnDef(def_id, ..)
                    | Closure(def_id, ..)
                    | Coroutine(def_id, ..)
                    | CoroutineClosure(def_id, ..) => {
                        calls.push((index.as_u32(), map_def_id(def_id.clone())));
                    }
                    FnPtr(..) => {
                        // TODO
                    }
                    _ => {}
                }
                if let Some(target) = target {
                    cfg.insert(index.as_u32(), vec![(target.as_u32(), None)]);
                }
                if is_never || target.is_none() {
                    interesting_blocks.push(index.as_u32())
                }
            }
            TailCall { func, .. } => {
                // let is_never = destination.ty(body, tcx).ty.is_never();
                let is_never = false; // TODO
                use rustc_type_ir::TyKind::*;
                match func.ty(body.local_decls(), tcx).kind() {
                    FnDef(def_id, ..)
                    | Closure(def_id, ..)
                    | Coroutine(def_id, ..)
                    | CoroutineClosure(def_id, ..) => {
                        calls.push((index.as_u32(), map_def_id(def_id.clone())));
                    }
                    FnPtr(..) => {
                        // TODO
                    }
                    _ => {}
                }
                if is_never {
                    interesting_blocks.push(index.as_u32())
                }
            }
            Yield { resume, .. } => {
                cfg.insert(index.as_u32(), vec![(resume.as_u32(), None)]);
            }

            InlineAsm { targets, .. } => {
                // TODO: Handle unwind
                for target in targets.iter() {
                    cfg.insert(index.as_u32(), vec![(target.as_u32(), None)]);
                }
            }
        }
    }
    (cfg, calls, interesting_blocks)
}

fn map_def_id(def_id: rustc_hir::def_id::DefId) -> DefId {
    DefId(def_id.krate.as_u32(), def_id.index.as_u32())
}
