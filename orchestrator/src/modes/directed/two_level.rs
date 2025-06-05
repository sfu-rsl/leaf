use std::borrow::Borrow;

use derive_more as dm;

use common::{
    directed::{CallGraph, ControlFlowGraph, ProgramMap},
    log_trace,
    pri::{BasicBlockIndex, BasicBlockLocation},
    types::InstanceKindId,
};

use crate::{ProgramReachability, SwitchStep, utils::from_pinned_coroutine};

type Function = InstanceKindId;
type BasicBlock = BasicBlockIndex;

#[derive(Debug, dm::From)]
pub(crate) struct DirectedEdge<'a, M = ()> {
    pub src: &'a SwitchStep,
    pub dst: BasicBlock,
    pub metadata: M,
}

impl<'a> From<(&'a SwitchStep, BasicBlock)> for DirectedEdge<'a> {
    fn from((step, toward): (&'a SwitchStep, BasicBlock)) -> Self {
        Self::from((step, toward, ()))
    }
}

pub(crate) struct Director<'a> {
    trace: &'a [SwitchStep],
}

impl<'a> Director<'a> {
    pub(crate) fn new(trace: &'a [SwitchStep]) -> Self {
        Self { trace }
    }

    pub(crate) fn find_edges_toward(
        &self,
        p_map: &ProgramMap,
        reachability: &impl ProgramReachability,
        target: &BasicBlockLocation,
    ) -> impl Iterator<Item = DirectedEdge<'a>> {
        let mut trace: &'a [SwitchStep] = self.trace;
        from_pinned_coroutine(
            #[coroutine]
            static move || {
                let mut fn_trace: Vec<Function> = get_fn_trace(trace);
                let target_fn = target.body;

                let next_fn_finder = make_next_fn_finder(p_map, reachability);
                let mut proposed_edges = Vec::new();

                while !fn_trace.is_empty() {
                    let current_fn = *fn_trace.last().unwrap();
                    log_trace!("Current function: {}", current_fn);
                    let intra_trace = get_tailing_intra_trace(trace);
                    proposed_edges.clear();

                    let next_bb_finder = make_next_bb_finder(p_map, reachability, current_fn);

                    macro_rules! yield_all_search_intra {
                        ($target: expr) => {
                            for edge in search_intra(
                                &next_bb_finder,
                                intra_trace,
                                &mut proposed_edges,
                                $target,
                            ) {
                                yield edge;
                            }
                        };
                    }

                    if current_fn == target_fn {
                        yield_all_search_intra!(target.index);
                    }
                    // We continue anyway because of recursive (target) functions.

                    for next_fn in next_fn_finder.next_step(&fn_trace, &target_fn) {
                        log_trace!("Next function: {}", next_fn);

                        for call_site in get_call_sites(&p_map.call_graph, &current_fn, &next_fn) {
                            yield_all_search_intra!(call_site);
                        }
                    }

                    /* All return points are conservatively treated as local targets.
                     * There is an implicit edge from the current function to its caller.
                     * If the caller can potentially reach the target, then the return points
                     * can also reach it. This implicit edge is transitive across all parent callers.
                     * Therefore, the condition becomes: "If one of the callers can reach the target...".
                     * The entry point is among the parent callers, so this condition always holds,
                     * and we don't need to explicitly check it.
                     * The only exception is when we have already passed the point where the target
                     * is reachable in the caller function. However, this should not represent a
                     * significant portion of the work.
                     * NOTE: This logic could be improved by considering whether there is a dependency
                     * on the return value. */
                    for ret_point in get_return_points(p_map, &current_fn) {
                        yield_all_search_intra!(ret_point);
                    }

                    fn_trace.pop();
                    trace = &trace[..trace.len() - intra_trace.len()];
                }
            },
        )
    }
}

fn search_intra<'a, 'b, 'c, F: NextStepFinder<Node = BasicBlock>>(
    next_bb_finder: &'a F,
    mut trace: &'b [SwitchStep],
    proposed_edges: &'c mut Vec<(usize, BasicBlockIndex)>,
    target: BasicBlock,
) -> impl Iterator<Item = DirectedEdge<'b>> + use<'a, 'b, 'c, F> {
    from_pinned_coroutine(
        #[coroutine]
        static move || {
            while let Some(current) = trace.last() {
                log_trace!("Current bb: {}", current.location.index);

                for next_bb in next_bb_finder.next_step(&trace, &target) {
                    let edge = (current, next_bb);

                    let key = (trace.len(), next_bb);
                    if proposed_edges.contains(&key) {
                        log_trace!("Skipping repeated edge");
                        continue;
                    }
                    proposed_edges.push(key);

                    yield edge.into();
                }

                let Some((last, rest)) = trace.split_last() else {
                    unreachable!();
                };
                trace = rest;
            }
        },
    )
}

fn get_fn_trace(trace: &[SwitchStep]) -> Vec<Function> {
    let mut result = Vec::new();
    if trace.is_empty() {
        return result;
    }

    result.push(trace[0].location.body);
    for SwitchStep {
        location: BasicBlockLocation { body: func, .. },
        ..
    } in trace
    {
        if func != result.last().unwrap() {
            result.push(*func);
        }
    }

    result
}

fn get_tailing_intra_trace(trace: &[SwitchStep]) -> &[SwitchStep] {
    let Some(last) = trace.last() else {
        return trace;
    };
    let tailing_func = last.location.body;
    &trace[trace
        .iter()
        .rposition(|s| s.location.body != tailing_func)
        .map(|i| i + 1)
        .unwrap_or(0)..]
}

fn get_call_sites(
    graph: &CallGraph,
    caller: &Function,
    callee: &Function,
) -> impl Iterator<Item = BasicBlock> {
    core::iter::from_coroutine(
        #[coroutine]
        move || {
            let Some(call_sites) = graph.get(caller) else {
                return;
            };

            let result = call_sites
                .iter()
                .filter(|(_, c, _)| c.eq(callee))
                .map(|(bb, _, _)| bb)
                .copied();
            for call_site in result {
                yield call_site;
            }
        },
    )
}

fn get_return_points(p_map: &ProgramMap, func: &Function) -> impl Iterator<Item = BasicBlock> {
    core::iter::from_coroutine(
        #[coroutine]
        move || {
            let Some(ret_points) = p_map.ret_points.get(func) else {
                return;
            };

            for bb in ret_points {
                yield *bb;
            }
        },
    )
}

mod next {

    use common::{log_warn, utils::comma_separated};

    use crate::reachability::{QSet, ReachabilityBiMap};

    use super::*;

    pub(super) trait NextStepFinder {
        type Node;

        fn next_step<'a, S>(
            &self,
            trace: &'a [S],
            target: &Self::Node,
        ) -> impl Iterator<Item = Self::Node>
        where
            S: Borrow<Self::Node>;
    }

    struct CallGraphView<'a, 'b, R> {
        graph: &'a CallGraph,
        reachability: &'b R,
    }

    struct BodyView<'a, 'b, R> {
        graph: &'a ControlFlowGraph,
        reachability: &'b R,
    }

    impl<R: ReachabilityBiMap<Function>> NextStepFinder for CallGraphView<'_, '_, R> {
        type Node = Function;

        fn next_step<'a, S>(
            &self,
            trace: &'a [S],
            target: &Self::Node,
        ) -> impl Iterator<Item = Self::Node>
        where
            S: Borrow<Self::Node>,
        {
            from_pinned_coroutine(
                #[coroutine]
                move || {
                    let Some(last_func) = trace.last() else {
                        return;
                    };

                    let Some(callees) = self.graph.get(last_func.borrow()) else {
                        return;
                    };

                    let reachers = self.reachability.reachers(target);
                    log_trace!("Target: {}", target);
                    log_trace!("Callees: {:?}", callees);
                    log_trace!(
                        "Reachers: {:?}",
                        comma_separated(reachers.iter(|r| r.to_string()))
                    );
                    let result = callees
                        .iter()
                        .map(|(_, c, _)| c)
                        .copied()
                        .filter(move |c| c == target || reachers.contains(c));
                    for callee in result {
                        yield callee;
                    }
                },
            )
        }
    }

    impl<R: ReachabilityBiMap<BasicBlock>> NextStepFinder for Option<BodyView<'_, '_, R>> {
        type Node = BasicBlock;

        fn next_step<'a, S>(
            &self,
            trace: &'a [S],
            target: &Self::Node,
        ) -> impl Iterator<Item = Self::Node>
        where
            S: Borrow<Self::Node>,
        {
            from_pinned_coroutine(
                #[coroutine]
                move || {
                    let Some(this) = self else {
                        return;
                    };

                    let Some(last_bb) = trace.last() else {
                        return;
                    };

                    let Some(successors) = this.graph.get(last_bb.borrow()) else {
                        return;
                    };

                    let reachers = this.reachability.reachers(target);

                    log_trace!("Target: {}", target);
                    log_trace!("Successors: {:?}", successors);
                    log_trace!(
                        "Reachers: {:?}",
                        comma_separated(reachers.iter(|r| r.to_string()))
                    );
                    let result = successors
                        .iter()
                        .map(|(s, _)| s)
                        .copied()
                        .filter(move |s| s == target || reachers.contains(s));
                    for successor in result {
                        yield successor;
                    }
                },
            )
        }
    }

    pub(super) fn make_next_fn_finder<'a, 'b>(
        p_map: &'a ProgramMap,
        reachability: &'b impl ProgramReachability,
    ) -> impl NextStepFinder<Node = Function> {
        CallGraphView {
            graph: &p_map.call_graph,
            reachability: reachability.fn_call(),
        }
    }

    pub(super) fn make_next_bb_finder<'a, 'b>(
        p_map: &'a ProgramMap,
        reachability: &'b impl ProgramReachability,
        current_fn: Function,
    ) -> impl NextStepFinder<Node = BasicBlock> {
        let Some(graph) = p_map.cfgs.get(&current_fn) else {
            log_warn!("Missing CFG in program map: {current_fn}");
            return None;
        };

        let Some(reachability) = reachability.cfg(current_fn) else {
            log_warn!("Missing CFG reachability information: {current_fn}");
            return None;
        };

        Some(BodyView {
            graph,
            reachability,
        })
    }

    impl Borrow<Function> for SwitchStep {
        fn borrow(&self) -> &Function {
            &self.location.body
        }
    }

    impl Borrow<BasicBlock> for SwitchStep {
        fn borrow(&self) -> &BasicBlock {
            &self.location.index
        }
    }
}
use next::*;
