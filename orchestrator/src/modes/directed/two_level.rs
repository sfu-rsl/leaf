use std::{borrow::Borrow, collections::HashMap};

use derive_more as dm;
use serde::{Deserialize, Serialize};

use common::{
    directed::{CallGraph, ControlFlowGraph, ProgramMap},
    log_trace,
    pri::{BasicBlockIndex, BasicBlockLocation},
    types::DefId,
};

use crate::{Reachability, SwitchStep, utils::from_pinned_coroutine};

type Function = DefId;
type BasicBlock = BasicBlockIndex;

#[derive(Serialize, Deserialize)]
pub(crate) struct ProgramReachability {
    pub call: Reachability<Function, Function>,
    pub cfgs: HashMap<Function, Reachability<BasicBlock, BasicBlock>>,
}

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
        reachability: &ProgramReachability,
        target: &BasicBlockLocation,
    ) -> impl Iterator<Item = DirectedEdge<'a>> {
        let mut trace: &'a [SwitchStep] = self.trace;
        from_pinned_coroutine(
            #[coroutine]
            static move || {
                let mut fn_trace: Vec<Function> = get_fn_trace(trace);
                let target_fn = target.body;

                let next_fn_finder = make_next_fn_finder(p_map, reachability);

                while !fn_trace.is_empty() {
                    let current_fn = *fn_trace.last().unwrap();
                    log_trace!("Current function: {}", current_fn);
                    let intra_trace = get_tailing_intra_trace(trace);

                    let next_bb_finder = make_next_bb_finder(p_map, reachability, current_fn);

                    if current_fn == target_fn {
                        for edge in search_intra(&next_bb_finder, intra_trace, &target.index) {
                            yield edge;
                        }
                    }

                    // We'll do it anyway because of recursive functions.
                    for next_fn in next_fn_finder.next_step(&fn_trace, &target_fn) {
                        log_trace!("Next function: {}", next_fn);

                        for call_site in get_call_sites(&p_map.call_graph, &current_fn, &next_fn) {
                            for edge in search_intra(&next_bb_finder, intra_trace, &call_site) {
                                yield edge;
                            }
                        }
                    }

                    fn_trace.pop();
                    trace = &trace[..trace.len() - intra_trace.len()];
                }
            },
        )
    }
}

fn search_intra<'a>(
    next_bb_finder: &impl NextStepFinder<Node = BasicBlock>,
    mut trace: &'a [SwitchStep],
    target: &u32,
) -> impl Iterator<Item = DirectedEdge<'a>> {
    core::iter::from_coroutine(
        #[coroutine]
        move || {
            let mut back_taken_step: Option<BasicBlock> = None;
            while let Some(current) = trace.last() {
                log_trace!("Current bb: {}", current.location.index);

                for next_bb in next_bb_finder.next_step(&trace, target) {
                    if back_taken_step.is_some_and(|s| s == next_bb || s == *target) {
                        log_trace!("Skipping back taken step");
                        continue;
                    }

                    let edge = (current, next_bb);
                    yield edge.into();
                }

                let Some((last, rest)) = trace.split_last() else {
                    unreachable!();
                };
                back_taken_step = Some(*last.borrow());
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
                .filter(|(_, c)| c.eq(callee))
                .map(|(bb, _)| bb)
                .copied();
            for call_site in result {
                yield call_site;
            }
        },
    )
}

mod next {
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

    struct CallGraphView<'a, 'b> {
        graph: &'a CallGraph,
        reachability: &'b Reachability<Function>,
    }

    struct ControlFlowGraphView<'a, 'b> {
        graph: &'a ControlFlowGraph,
        reachability: &'b Reachability<BasicBlock>,
    }

    impl NextStepFinder for CallGraphView<'_, '_> {
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
                    log_trace!("Reachers: {:?}", reachers);
                    let result = callees
                        .iter()
                        .map(|(_, c)| c)
                        .copied()
                        .filter(move |c| c == target || reachers.contains(c));
                    for callee in result {
                        yield callee;
                    }
                },
            )
        }
    }

    impl NextStepFinder for ControlFlowGraphView<'_, '_> {
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
                    let Some(last_bb) = trace.last() else {
                        return;
                    };

                    let Some(successors) = self.graph.get(last_bb.borrow()) else {
                        return;
                    };

                    let reachers = self.reachability.reachers(target);

                    log_trace!("Target: {}", target);
                    log_trace!("Successors: {:?}", successors);
                    log_trace!("Reachers: {:?}", reachers);
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
        reachability: &'b ProgramReachability,
    ) -> impl NextStepFinder<Node = Function> {
        CallGraphView {
            graph: &p_map.call_graph,
            reachability: &reachability.call,
        }
    }

    pub(super) fn make_next_bb_finder<'a, 'b>(
        p_map: &'a ProgramMap,
        reachability: &'b ProgramReachability,
        current_fn: DefId,
    ) -> impl NextStepFinder<Node = BasicBlock> {
        ControlFlowGraphView {
            graph: p_map
                .cfgs
                .get(&current_fn)
                .expect("Could not find cfg in program map"),
            reachability: reachability
                .cfgs
                .get(&current_fn)
                .expect("Could not find cfg reachability"),
        }
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
