use rustc_middle::{mir::Body, ty::TyCtxt};

use common::{
    log_info,
    program_map::{ProgramMap, rw::write_program_map_to},
};

use crate::utils::{file::InfoArtifact, mir::InstanceKindExt};

use super::super::{CompilationPass, OverrideFlags, Storage, StorageExt};

#[derive(Default)]
pub(crate) struct ProgramMapExporter {
    output_dir: Option<std::path::PathBuf>,
}

impl ProgramMapExporter {
    pub(crate) fn new(output_dir: Option<std::path::PathBuf>) -> Self {
        Self { output_dir }
    }
}

const KEY_MAP: &str = "program_map";

const FILENAME_MAP: &str = "program_map.json";
const FILENAME_STABLE_PREFIX: &str = "program_map-";

const EXTENSION_MAP: &str = "json";

impl CompilationPass for ProgramMapExporter {
    fn override_flags() -> OverrideFlags {
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
        visit_and_add(&mut p_map, tcx, body);
    }

    fn visit_tcx_at_codegen_after(&mut self, tcx: TyCtxt, storage: &mut dyn Storage) {
        log_info!("Exporting program map");

        let mut p_map = storage.get_or_default::<ProgramMap>(KEY_MAP.to_owned());

        tcx.collect_and_partition_mono_items(())
            .codegen_units
            .iter()
            .flat_map(|unit| unit.items())
            .flat_map(|(item, _)| match item {
                rustc_middle::mono::MonoItem::Fn(instance) => Some(instance.def),
                _ => None,
            })
            .for_each(|instance| {
                visit_and_add(&mut p_map, tcx, tcx.instance_mir(instance));
            });

        entry_points::add_to(tcx, &mut p_map);

        let artifact = InfoArtifact::new(
            tcx,
            self.output_dir.as_deref(),
            FILENAME_STABLE_PREFIX,
            EXTENSION_MAP,
        );

        write_program_map_to(&p_map, &artifact.out_file).expect("Failed to write program map");

        artifact.write_compatibility_copy(FILENAME_MAP, "program map");
    }
}

fn visit_and_add<'tcx>(p_map: &mut ProgramMap, tcx: TyCtxt<'tcx>, body: &Body<'tcx>) {
    let key = body.source.instance.to_plain_id(tcx);
    if p_map.cfgs.contains_key(&key) {
        return;
    }

    p_map.cfgs.insert(key, cfg::from_body(body));
    p_map.ret_points.insert(key, return_points::from_body(body));
    p_map
        .call_graph
        .insert(key, call_graph::from_body(tcx, body));
    p_map
        .debug_info
        .func_names
        .insert(key, debug_info::function_name(tcx, body));
}

mod cfg {
    use rustc_middle::mir::{BasicBlock, Body};

    use core::range::RangeInclusive;

    use common::program_map::{BasicBlockIndex, CfgConstraint, CfgEdge, ControlFlowGraph};

    type BlockRangeAndEdges = (RangeInclusive<BasicBlockIndex>, Vec<CfgEdge>);

    pub(super) fn from_body(body: &Body<'_>) -> ControlFlowGraph {
        let predecessors_map = body.basic_blocks.predecessors();
        let blocks_successors = body.basic_blocks.iter_enumerated().map(|(index, _)| {
            (
                index.as_u32(),
                terminator_edges_cleanup_agnostic(body, index),
            )
        });
        let nodes = group_block_strings(blocks_successors, |block| {
            let block: BasicBlock = block.into();
            predecessors_map[block].len() == 1
        });
        build_cfg(nodes)
    }

    fn terminator_edges_cleanup_agnostic(body: &Body<'_>, block: BasicBlock) -> Vec<CfgEdge> {
        use rustc_middle::mir::{CallReturnPlaces, TerminatorEdges};

        match body.basic_blocks[block].terminator().edges() {
            TerminatorEdges::None => Vec::new(),
            TerminatorEdges::Single(target) | TerminatorEdges::Double(target, _) => {
                vec![(target.as_u32(), None)]
            }
            TerminatorEdges::AssignOnReturn { return_, place, .. } => match place {
                CallReturnPlaces::Call(_) | CallReturnPlaces::Yield(_) => return_
                    .first()
                    .map(|target| (target.as_u32(), None))
                    .into_iter()
                    .collect(),
                CallReturnPlaces::InlineAsm(_) => return_
                    .iter()
                    .map(|target| (target.as_u32(), None))
                    .collect(),
            },
            TerminatorEdges::SwitchInt { targets, .. } => {
                let mut successors = Vec::new();
                for (value, target) in targets.iter() {
                    successors.push((target.as_u32(), Some(CfgConstraint::Case(value))));
                }
                if !body.basic_blocks[targets.otherwise()].is_empty_unreachable() {
                    successors.push((targets.otherwise().as_u32(), Some(CfgConstraint::Otherwise)));
                }
                successors
            }
        }
    }

    fn group_block_strings(
        blocks: impl IntoIterator<Item = (BasicBlockIndex, Vec<CfgEdge>)>,
        has_single_predecessor: impl Fn(BasicBlockIndex) -> bool,
    ) -> Vec<BlockRangeAndEdges> {
        let mut nodes: Vec<BlockRangeAndEdges> = Vec::new();

        for (block, successors) in blocks {
            let continues_previous = nodes.last().is_some_and(|(block_range, outgoing_edges)| {
                block_range.last.checked_add(1) == Some(block)
                    && outgoing_edges.len() == 1
                    && outgoing_edges[0].0 == block
                    && outgoing_edges[0].1.is_none()
                    && has_single_predecessor(block)
            });

            if continues_previous {
                let previous = nodes.last_mut().unwrap();
                previous.0.last = block;
                previous.1 = successors;
            } else {
                nodes.push((RangeInclusive::from(block..=block), successors));
            }
        }

        nodes
    }

    fn build_cfg(nodes: Vec<BlockRangeAndEdges>) -> ControlFlowGraph {
        let mut cfg = ControlFlowGraph::new();
        for (block_range, _) in &nodes {
            let _ = cfg
                .insert_range(*block_range, Vec::new())
                .expect("CFG node ranges must be valid and non-overlapping");
        }

        for (block_range, outgoing_edges) in nodes {
            let outgoing_edges = outgoing_edges
                .into_iter()
                .map(|(target, constraint)| {
                    let target_node = cfg
                        .node_containing(target)
                        .expect("MIR successor must belong to a CFG node");
                    (target_node, constraint)
                })
                .collect();
            let _ = cfg
                .insert_range(block_range, outgoing_edges)
                .expect("CFG node ranges must be valid and non-overlapping");
        }

        cfg
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn node_ranges(nodes: &[BlockRangeAndEdges]) -> Vec<(BasicBlockIndex, BasicBlockIndex)> {
            nodes
                .iter()
                .map(|(range, _)| (range.start, range.last))
                .collect()
        }

        #[test]
        fn groups_linear_blocks_into_one_range() {
            let nodes = group_block_strings(
                vec![(0, vec![(1, None)]), (1, vec![(2, None)]), (2, Vec::new())],
                |_| true,
            );

            assert_eq!(node_ranges(&nodes), vec![(0, 2)]);
            assert!(nodes[0].1.is_empty());
        }

        #[test]
        fn keeps_non_index_ordered_linear_chain_in_separate_ranges() {
            let nodes = group_block_strings(
                vec![(0, vec![(2, None)]), (1, Vec::new()), (2, vec![(1, None)])],
                |_| true,
            );

            assert_eq!(node_ranges(&nodes), vec![(0, 0), (1, 1), (2, 2)]);
        }

        #[test]
        fn keeps_branch_and_join_targets_as_separate_nodes() {
            let nodes = group_block_strings(
                vec![
                    (0, vec![(1, None), (2, None)]),
                    (1, vec![(3, None)]),
                    (2, vec![(3, None)]),
                    (3, vec![(4, None)]),
                    (4, Vec::new()),
                ],
                |block| block != 3,
            );

            assert_eq!(node_ranges(&nodes), vec![(0, 0), (1, 1), (2, 2), (3, 4)]);
        }

        #[test]
        fn call_normal_continuation_is_sequential() {
            let nodes = group_block_strings(
                vec![(0, vec![(1, None)]), (1, vec![(2, None)]), (2, Vec::new())],
                |_| true,
            );

            assert_eq!(node_ranges(&nodes), vec![(0, 2)]);
        }
    }
}

mod call_graph {
    use rustc_middle::{
        mir::{Body, HasLocalDecls, TerminatorKind},
        ty::TyCtxt,
    };

    use common::{
        program_map::{CallDebugInfo, CallGraphEdgeDestination},
        types::InstanceKindId,
    };

    use crate::utils::mir::{InstanceKindExt, TyCtxtExt};

    type Calls = Vec<CallGraphEdgeDestination>;

    pub(super) fn from_body<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>) -> Calls {
        let mut calls = Calls::new();

        for (index, block) in body.basic_blocks.iter_enumerated() {
            let (TerminatorKind::Call { func, fn_span, .. }
            | TerminatorKind::TailCall { func, fn_span, .. }) = &block.terminator().kind
            else {
                continue;
            };

            let debug_info = CallDebugInfo {
                location: tcx.sess.source_map().span_to_short_string(
                    *fn_span,
                    rustc_span::RemapPathScopeComponents::DEBUGINFO,
                ),
            };

            let callee_ty = func.ty(body.local_decls(), tcx);
            use rustc_type_ir::TyKind::*;
            match callee_ty.kind() {
                FnDef(def_id, generic_args)
                | Closure(def_id, generic_args)
                | Coroutine(def_id, generic_args)
                | CoroutineClosure(def_id, generic_args) => {
                    let instance_kind = tcx.try_resolve_instance_raw(
                        tcx.typing_env_in_body(body.source.def_id()),
                        *def_id,
                        generic_args,
                    );
                    calls.push((
                        index.as_u32(),
                        instance_kind
                            .map(|instance| instance.def.to_plain_id(tcx))
                            .unwrap_or(InstanceKindId::INVALID),
                        debug_info,
                    ));
                }
                FnPtr(..) => calls.push((index.as_u32(), InstanceKindId::INVALID, debug_info)),
                _ => unreachable!("Unexpected callee type: {:?}", callee_ty),
            }
        }

        calls
    }
}

mod return_points {
    use rustc_middle::mir::{Body, TerminatorKind};

    use common::program_map::BasicBlockIndex;

    pub(super) fn from_body(body: &Body<'_>) -> Vec<BasicBlockIndex> {
        body.basic_blocks
            .iter_enumerated()
            .filter_map(|(index, block)| {
                matches!(&block.terminator().kind, TerminatorKind::Return).then_some(index.as_u32())
            })
            .collect()
    }
}

mod debug_info {
    use rustc_middle::{mir::Body, ty::TyCtxt};

    pub(super) fn function_name(tcx: TyCtxt<'_>, body: &Body<'_>) -> String {
        tcx.def_path_str(body.source.def_id())
    }
}

mod entry_points {
    use rustc_middle::ty::{InstanceKind, TyCtxt};

    use common::program_map::ProgramMap;

    use crate::utils::mir::InstanceKindExt;

    pub(super) fn add_to<'tcx>(tcx: TyCtxt<'tcx>, p_map: &mut ProgramMap) {
        p_map.entry_points.extend(
            tcx.entry_fn(())
                .iter()
                .map(|(def_id, _)| InstanceKind::Item(*def_id).to_plain_id(tcx)),
        );
    }
}
