use std::{collections::HashMap, rc::Rc};

use common::{
    directed::{CfgConstraint, CfgEdgeDestination, ProgramMap},
    log_debug, log_warn,
    pri::{BasicBlockIndex, BasicBlockLocation},
    types::trace::ConstraintKind,
    z3::{AstAndVars, AstNode, AstNodeSort, BVExt, BVNode, BVSort, WrappedSolver as Z3Solver},
};
use disjoint::DisjointSet;

use crate::{ProgramReachability, QSet, ReachabilityBiMap, SwitchStep, two_level::DirectedEdge};

type Constraint<'ctx> = common::types::trace::Constraint<AstAndVars<'ctx, u32>, AstNode<'ctx>>;
type ParsedSwitchStep<'ctx> = SwitchStep<AstAndVars<'ctx, u32>, AstNode<'ctx>>;
type VariablesForest = Rc<DisjointSet>;

pub(crate) struct Solver<'ctx, 'a> {
    solver: Z3Solver<'ctx, u32>,
    trace: &'a [SwitchStep],
    parsed_trace: Vec<ParsedSwitchStep<'ctx>>,
    weakened_trace: Vec<ParsedSwitchStep<'ctx>>,
    vars_forests: Vec<Option<VariablesForest>>,
}

impl<'ctx, 'a> Solver<'ctx, 'a> {
    pub(crate) fn new(
        trace: &'a [SwitchStep],
        current_input: &[u8],
        p_map: &ProgramMap,
        reachability: &impl ProgramReachability,
    ) -> Self {
        let mut solver: Z3Solver<_> = Default::default();
        let (vars, parsed_trace) = parse_trace(trace, solver.context());
        suggest_current_input_as_answer(&vars, current_input, &mut solver);

        let mut weakened_trace = parsed_trace.clone();
        weaken_steps(trace, p_map, reachability, weakened_trace.as_mut_slice());

        let vars_forests = make_vars_forests(&parsed_trace);

        Self {
            solver,
            trace,
            parsed_trace,
            weakened_trace,
            vars_forests,
        }
    }

    /// # Remarks
    /// The metadata of the edge is the list of outgoing edges from the source switch node.
    #[tracing::instrument(level = "debug", skip_all)]
    pub(crate) fn satisfy_edge(
        &self,
        edge: &DirectedEdge<'_, &[CfgEdgeDestination]>,
    ) -> Option<impl Iterator<Item = (z3::SatResult, HashMap<u32, AstNode<'ctx>>)>> {
        log_debug!(
            discr = edge
                .src
                .decision
                .as_ref()
                .map(|d| &d.discr)
                .map(|d| d.to_string()),
            "Edge to satisfy: {} toward {}",
            &edge.src.location,
            edge.dst,
        );

        let constraints = &self.parsed_trace[..=self
            .trace
            .element_offset(edge.src)
            .expect("Inconsistent referencing")];
        let (last, prefix) = constraints.split_last().unwrap();

        // If (symbolic) constraint is available
        let Some(decision) = &last.decision else {
            return None;
        };

        Some(self.satisfy_decision_toward(prefix, decision, edge.dst, edge.metadata))
    }

    fn satisfy_decision_toward<'b, 'c>(
        &'b self,
        prefix: &'b [ParsedSwitchStep<'ctx>],
        decision: &'b Constraint<'ctx>,
        dst: BasicBlockIndex,
        outgoing_edges: &'c [CfgEdgeDestination],
    ) -> impl Iterator<Item = (z3::SatResult, HashMap<u32, AstNode<'ctx>>)> + use<'b, 'c, 'ctx>
    {
        // Only send constraints for the participating variables.
        let vars_forest = self.vars_forests[prefix.len()].as_ref().unwrap();
        let vars_set_root = vars_forest.root_of(
            decision
                .discr
                .variables
                .iter()
                .map(|(id, _)| *id as usize)
                .next()
                .unwrap(),
        );

        const USE_WEAKENED_TRACE: bool = false;
        let prefix = if USE_WEAKENED_TRACE {
            &self.weakened_trace[..prefix.len()]
        } else {
            prefix
        };
        let prefix = prefix
            .iter()
            .filter_map(|s| s.decision.as_ref())
            .filter(|d| {
                d.discr
                    .variables
                    .iter()
                    .any(|(id, _)| vars_forest.is_joined(vars_set_root, *id as usize))
            })
            .collect::<Vec<_>>();

        log_debug!("Found {} relevant constraints in the prefix", prefix.len());

        let to_satisfy = construct_constraint_kinds_to_satisfy(decision, outgoing_edges, dst)
            .map(|kind| Constraint {
                discr: decision.discr.clone(),
                kind,
            })
            .inspect(|c| log_debug!("Constraint to satisfy: {c}"));

        to_satisfy.map(move |to_satisfy| {
            self.solver.check(
                prefix
                    .iter()
                    .map(|c| Constraint::clone(c))
                    .chain(core::iter::once(to_satisfy)),
            )
        })
    }
}

fn suggest_current_input_as_answer<'ctx>(
    vars: &HashMap<u32, AstNode<'ctx>>,
    current_input: &[u8],
    solver: &mut Z3Solver<'ctx, u32>,
) {
    if vars.len() != current_input.len() {
        log_warn!("Current input does not align with the found symbolic variables");
        return;
    }

    for (id, var) in vars.iter() {
        solver.consider_possible_answer(
            var.clone(),
            AstNode::BitVector(BVNode(
                z3::ast::BV::from_u64(
                    solver.context(),
                    current_input[*id as usize - 1] as u64,
                    u8::BITS,
                ),
                BVSort { is_signed: false },
            )),
        );
    }
}

fn parse_trace<'ctx>(
    trace: &[SwitchStep],
    context: &'ctx z3::Context,
) -> (HashMap<u32, AstNode<'ctx>>, Vec<ParsedSwitchStep<'ctx>>) {
    let mut vars = HashMap::new();
    let parsed_trace = trace
        .iter()
        .map(|t| SwitchStep {
            decision: t.decision.as_ref().map(|d| {
                d.as_ref().map(
                    |discr| discr.parse(context, &mut vars),
                    |case| case.parse_as_const(context).unwrap(),
                )
            }),
            location: t.location,
        })
        .collect::<Vec<_>>();
    (vars, parsed_trace)
}

/// Calculates related variables forests from the beginning up to each step in the trace.
/// Variables are related if they appear in a constraint together.
fn make_vars_forests(parsed_trace: &[ParsedSwitchStep]) -> Vec<Option<VariablesForest>> {
    let num_vars = parsed_trace
        .iter()
        .filter_map(|s| s.decision.as_ref())
        .flat_map(|d| d.discr.variables.iter().map(|(id, _)| id))
        .max()
        .copied()
        .unwrap_or(0);

    let mut current = VariablesForest::new(DisjointSet::with_len(num_vars as usize + 1));
    parsed_trace
        .into_iter()
        .map(|s| s.decision.as_ref())
        .map(|d| {
            d.map(|d| {
                d.discr
                    .variables
                    .iter()
                    .map(|(id, _)| *id as usize)
                    .map_windows::<_, _, 2>(|[a, b]| {
                        if !current.is_joined(*a, *b) {
                            VariablesForest::make_mut(&mut current).join(*a, *b);
                        }
                    })
                    .count();
                current.clone()
            })
        })
        .collect()
}

/// Weakens the constraints on steps based on the reachability of the next step.
/// If there are multiple branches that lead to the next step, the constraint can
/// be weakened, so it lets more values to be selected at that particular step.
#[tracing::instrument(level = "debug", skip_all)]
fn weaken_steps<'ctx>(
    trace: &[SwitchStep],
    p_map: &ProgramMap,
    reachability: &impl ProgramReachability,
    parsed_trace: &mut [ParsedSwitchStep<'ctx>],
) {
    const ENABLE_REACHABILITY: bool = false;
    parsed_trace
        .iter_mut()
        .zip(trace.iter().map_windows::<_, _, 2>(|[step, next_step]| {
            /* NOTE: A more effective approach would be looking at the next step in the full trace.
             * For simplicity, we currently only do it at intra-procedural level. */
            (next_step.location.body == step.location.body).then_some(next_step.location.index)
        }))
        .for_each(|(step, next)| {
            let Some(edges) = p_map
                .cfgs
                .get(&step.location.body)
                .and_then(|cfg| cfg.get(&step.location.index))
            else {
                return;
            };

            weaken_constraint(
                step,
                edges,
                ENABLE_REACHABILITY.then_some((reachability, next)),
            );
        });
}

/// Weakens the path constraint with respect to the next step in the trace.
/// If the next step is reachable through multiple edges, their constraints will
/// be merged (logical or).
/// # Remarks
/// You can disable weakening based on reachability by passing `None`. Then only
/// the CFG will be taken into account.
#[tracing::instrument(level = "debug", skip(outgoing_edges, reachability))]
fn weaken_constraint<'ctx>(
    step: &mut ParsedSwitchStep<'ctx>,
    outgoing_edges: &[CfgEdgeDestination],
    reachability: Option<(&impl ProgramReachability, Option<BasicBlockIndex>)>,
) {
    let Some(ref mut taken_decision) = step.decision else {
        return;
    };

    let out_going_kinds = outgoing_edges
        .iter()
        .map(|(target, c)| {
            (
                *target,
                constraint_kind_from_outgoing_edge(
                    c.expect(Some(step.location)),
                    outgoing_edges,
                    taken_decision.kind.is_boolean(),
                ),
            )
        })
        .collect::<Vec<_>>();

    let taken_kind = taken_decision
        .kind
        .as_ref()
        .map(|case| case.as_bit_vector().as_u128().unwrap());

    let next = if let Some(next) = reachability.and_then(|(_, next)| next) {
        // If provided (based on the trace)
        next
    } else {
        out_going_kinds
            .iter()
            .find_map(|(target, kind)| (kind.eq(&taken_kind)).then_some(*target))
            .unwrap_or_else(|| {
                panic!(
                    "Could not map constraint back to CFG edge based on the taken step. Decision: {} at {}",
                    taken_kind, step.location,
                )
            })
    };

    // Possible kinds: Constraint kinds (edges) that take us to the next observed step.
    let possible_kinds = out_going_kinds
        .into_iter()
        .filter(|(t, _)| {
            /* NOTE: A more effective approach would be looking at the next step in the full trace.
             * For simplicity, we currently only do it at intra-procedural level. */
            *t == next
                || reachability
                    .and_then(|(r, _)| r.cfg(step.location.body))
                    .is_some_and(|r| r.reachers(&next).contains(t))
        })
        .map(|(_, k)| k);
    let case_count = case_count(outgoing_edges);
    let weakened_kind = possible_kinds
        .into_iter()
        .try_reduce(|acc, k| acc.or(&k, || case_count))
        .map(|k| {
            k.unwrap_or_else(|| {
                panic!(
                    "Could not map constraint back to CFG edge based on target. Decision: {} at {}, Target: {}",
                    taken_kind, step.location, next,
                )
            })
        });

    if let Some(weakened_kind) = weakened_kind {
        taken_decision.kind = weakened_kind.map(ast_mapper(&taken_decision.discr));
    } else {
        log_debug!(
            "Removing constraint. The next basic block ({}), is reachable from all edges of {:?}.",
            next,
            step
        );
        step.decision = None;
    }
}

// NOTE: If the CFG has merged targets, multiple edges for a single target is possible.
fn construct_constraint_kinds_to_satisfy<'ctx>(
    taken_decision: &Constraint<'ctx>,
    outgoing_edges: &[CfgEdgeDestination],
    target: BasicBlockIndex,
) -> impl Iterator<Item = ConstraintKind<AstNode<'ctx>>> {
    // The value of the discriminant to take the edge. None for o.w.
    let target_case_values = outgoing_edges
        .iter()
        .filter(move |(t, _)| *t == target)
        .map(|(_, c)| c.expect(None));
    let kinds = target_case_values.map(move |value| {
        constraint_kind_from_outgoing_edge(value, outgoing_edges, taken_decision.kind.is_boolean())
    });

    let case_count = case_count(outgoing_edges);

    /* NOTE: Reducing or not changes the soundness.
     * Reducing results in weaker constraints, different case values will be explored in separate executions calls.
     * Soundness is dependent on the exclusion of previously seen answers.
     * Not reducing generates more inputs per execution but possible to make no difference. */
    // let kinds = kinds
    //     .into_iter()
    //     .try_reduce(|acc, k| acc.or(&k, || case_count))
    //     .map(|k| k.expect("At least one edge toward the target is expected."));

    let kinds = kinds
        .into_iter()
        .map(|k| k.map(ast_mapper(&taken_decision.discr)));

    let not_taken = taken_decision.kind.clone().not();

    /* Ensure we find something different.
     * This particularly makes difference when the CFG is merged
     * (we may execute different statements, although the paths merge later). */
    kinds.filter_map(move |k| k.and(&not_taken, || case_count))
}

fn constraint_kind_from_outgoing_edge(
    edge_case: CfgConstraint,
    all_edges: &[CfgEdgeDestination],
    is_boolean_switch: bool,
) -> ConstraintKind<u128> {
    use ConstraintKind::*;
    if is_boolean_switch {
        let to_kind = |v| {
            if v == 0 { False } else { True }
        };
        match edge_case {
            CfgConstraint::Case(value) => to_kind(value),
            CfgConstraint::Otherwise => {
                debug_assert_eq!(all_edges.len(), 2);
                /* Although we know that otherwise case for boolean switches is True,
                 * let's not rely on the assumption. */
                let not_value = all_edges.iter().find_map(|(_, v)| v.flatten()).unwrap();
                to_kind(not_value).not()
            }
        }
    } else {
        match edge_case {
            CfgConstraint::Case(value) => OneOf(vec![value]),
            CfgConstraint::Otherwise => {
                OneOf(all_edges.iter().filter_map(|(_, v)| v.flatten()).collect()).not()
            }
        }
    }
}

fn ast_mapper<'ctx>(discr: &AstNode<'ctx>) -> impl FnMut(u128) -> AstNode<'ctx> {
    let sort = discr.sort();

    move |case| {
        AstNode::BitVector(BVNode(
            z3::ast::BV::from_str(
                discr.ast().get_ctx(),
                discr.as_bit_vector().get_size(),
                &case.to_string(),
            )
            .unwrap(),
            {
                let AstNodeSort::BitVector(sort) = sort else {
                    unreachable!("Unexpected sort for a non-bool discriminant: {:?}", sort)
                };
                sort
            },
        ))
    }
}

fn case_count(outgoing_edges: &[CfgEdgeDestination]) -> usize {
    outgoing_edges
        .into_iter()
        .flat_map(|(_, c)| c)
        .fold(0, |count, constraint| match constraint {
            CfgConstraint::Case(_) => count.saturating_add(1),
            /* While having otherwise, makes the universe practically infinite,
             * it should appear for exhaustive matches. */
            CfgConstraint::Otherwise => usize::MAX,
        })
}

trait CfgConstraintExt {
    fn flatten(self) -> Option<u128>;
    fn expect(&self, location: Option<BasicBlockLocation>) -> CfgConstraint;
}

impl CfgConstraintExt for Option<CfgConstraint> {
    fn flatten(self) -> Option<u128> {
        self.and_then(|c| match c {
            CfgConstraint::Case(value) => Some(value),
            CfgConstraint::Otherwise => None,
        })
    }

    fn expect(&self, location: Option<BasicBlockLocation>) -> CfgConstraint {
        self.unwrap_or_else(|| {
            unreachable!(
                "Unconstrained decision is not expected {}",
                location.map(|l| format!("at {l}")).unwrap_or("".to_owned()),
            )
        })
    }
}
