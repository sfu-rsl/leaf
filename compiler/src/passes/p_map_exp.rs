use rustc_middle::{
    mir::{BasicBlock, Body, HasLocalDecls},
    ty::TyCtxt,
};

use common::directed::{BasicBlockIndex, CfgEdgeDestination, ControlFlowGraph, DefId, ProgramMap};

use super::{CompilationPass, OverrideFlags, Storage, StorageExt};
use crate::utils::file::TyCtxtFileExt;

type Calls = Vec<(BasicBlockIndex, DefId)>;
type ReturnPoints = Vec<BasicBlockIndex>;

#[derive(Default)]
pub(crate) struct ProgramMapExporter;

const KEY_MAP: &str = "program_map";

const FILE_OUTPUT: &str = "program_map.json";

impl CompilationPass for ProgramMapExporter {
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
    p_map.ret_points.insert(key, data.1);
    p_map.call_graph.insert(key, data.2);
    p_map
        .debug_info
        .func_names
        .insert(key, tcx.def_path_str(def_id));
}

fn visit_body<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
) -> (ControlFlowGraph, ReturnPoints, Calls) {
    let mut cfg = ControlFlowGraph::new();
    let mut ret_points = Vec::new();
    let mut calls = Calls::new();

    let first_cfg_affecting_starting = |bb: BasicBlock| {
        let mut current = bb;
        while let Some(next) = body.basic_blocks[current]
            .terminator
            .as_ref()
            .and_then(|t| t.kind.as_goto())
        {
            current = next;
        }
        current
    };

    for (index, block) in body.basic_blocks.iter_enumerated() {
        use rustc_middle::mir::TerminatorKind::*;

        let mut insert_to_cfg = |targets: Vec<CfgEdgeDestination>| {
            cfg.insert(
                index.as_u32(),
                targets
                    .into_iter()
                    .map(|(bb, c)| (first_cfg_affecting_starting(bb.into()).as_u32(), c))
                    .collect(),
            );
        };

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
                insert_to_cfg(vec![(target.as_u32(), None)]);
            }
            SwitchInt { discr: _, targets } => {
                let mut successors: Vec<(u32, Option<(usize, Option<u128>)>)> = Vec::new();
                for (target_index, target) in targets.iter().enumerate() {
                    successors.push((target.1.as_u32(), Some((target_index, Some(target.0)))));
                }
                if !body.basic_blocks[targets.otherwise()].is_empty_unreachable() {
                    successors.push((targets.otherwise().as_u32(), Some((usize::MAX, None))));
                }
                insert_to_cfg(successors);
            }
            Return => ret_points.push(index.as_u32()),
            UnwindResume | UnwindTerminate(_) | Unreachable | CoroutineDrop => {}
            Drop {
                target, unwind: _, ..
            }
            | Assert {
                target, unwind: _, ..
            } => {
                // TODO: Handle unwind
                insert_to_cfg(vec![(target.as_u32(), None)]);
            }
            Call { func, target, .. } => {
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
                    insert_to_cfg(vec![(target.as_u32(), None)]);
                }
            }
            TailCall { func, .. } => {
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
            }
            Yield { resume, .. } => {
                insert_to_cfg(vec![(resume.as_u32(), None)]);
            }

            InlineAsm { targets, .. } => {
                // TODO: Handle unwind
                for target in targets.iter() {
                    insert_to_cfg(vec![(target.as_u32(), None)]);
                }
            }
        }
    }
    (cfg, ret_points, calls)
}

fn map_def_id(def_id: rustc_hir::def_id::DefId) -> DefId {
    DefId(def_id.krate.as_u32(), def_id.index.as_u32())
}
