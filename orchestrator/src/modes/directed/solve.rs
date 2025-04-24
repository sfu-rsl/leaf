use std::collections::{HashMap, HashSet};

use common::{
    directed::CfgEdgeDestination,
    log_debug,
    pri::BasicBlockIndex,
    types::trace::ConstraintKind,
    z3::{AstAndVars, AstNode, AstNodeSort, BVNode, WrappedSolver as Z3Solver},
};

use crate::{SwitchStep, two_level::DirectedEdge};

type Constraint<'ctx> = common::types::trace::Constraint<AstAndVars<'ctx, u32>, AstNode<'ctx>>;
type ParsedSwitchStep<'ctx> = SwitchStep<AstAndVars<'ctx, u32>, AstNode<'ctx>>;

pub(crate) struct Solver<'ctx, 'a> {
    solver: Z3Solver<'ctx, u32>,
    trace: &'a [SwitchStep],
    parsed_trace: Vec<ParsedSwitchStep<'ctx>>,
}

impl<'ctx, 'a> Solver<'ctx, 'a> {
    pub(crate) fn new(trace: &'a [SwitchStep]) -> Self {
        let solver: Z3Solver<_> = Default::default();
        let mut vars = HashMap::new();
        let parsed_trace = trace
            .iter()
            .map(|t| SwitchStep {
                decision: t.decision.as_ref().map(|d| {
                    d.as_ref().map(
                        |discr| discr.parse(solver.context(), &mut vars),
                        |case| case.parse_as_const(solver.context()).unwrap(),
                    )
                }),
                location: t.location,
            })
            .collect();
        Self {
            solver,
            trace,
            parsed_trace,
        }
    }

    /// # Remarks
    /// The metadata of the edge is the list of outgoing edges from the source switch node.
    pub(crate) fn satisfy_edge(
        &self,
        edge: &DirectedEdge<'_, &[CfgEdgeDestination]>,
    ) -> Option<impl Iterator<Item = (z3::SatResult, HashMap<u32, AstNode<'ctx>>)>> {
        log_debug!(
            "Edge to satisfy: {} toward {}, with discriminant: {}",
            &edge.src.location,
            edge.dst,
            edge.src
                .decision
                .as_ref()
                .map(|d| &d.discr)
                .map(|d| d.to_string())
                .unwrap_or("??".to_owned())
        );

        let constraints = &self.parsed_trace[..=self
            .trace
            .element_offset(edge.src)
            .expect("Inconsistent referencing")];
        let (last, pre) = constraints.split_last().unwrap();

        // If (symbolic) constraint is available
        let Some(decision) = &last.decision else {
            return None;
        };

        let d_vars = decision
            .discr
            .variables
            .iter()
            .map(|(id, _)| *id)
            .collect::<HashSet<_>>();

        // Only send constraints for the participating variables.
        let pre = pre
            .iter()
            .filter_map(|s| s.decision.as_ref())
            .filter(|d| d.discr.variables.iter().any(|(id, _)| d_vars.contains(id)))
            .collect::<Vec<_>>();

        let results = construct_constraint_kinds_to_satisfy(decision, edge.metadata, edge.dst)
            .map(|kind| Constraint {
                discr: decision.discr.clone(),
                kind,
            })
            .inspect(|c| log_debug!("Constraint to satisfy: {c}"))
            .map(move |to_satisfy| {
                self.solver.check(
                    pre.iter()
                        .map(|c| Constraint::clone(c))
                        .chain(core::iter::once(to_satisfy)),
                )
            })
            .map(move |(sat_result, mut answers)| {
                if z3::SatResult::Sat == sat_result {
                    answers.retain(|var, _| d_vars.contains(var))
                }
                (sat_result, answers)
            });

        Some(results)
    }
}

// NOTE: The CFG might merge targets, so it is possible to have multiple edges for a single target.
fn construct_constraint_kinds_to_satisfy<'ctx>(
    taken_decision: &Constraint<'ctx>,
    outgoing_edges: &[CfgEdgeDestination],
    target: BasicBlockIndex,
) -> impl Iterator<Item = ConstraintKind<AstNode<'ctx>>> {
    // The value of the discriminant to take the edge. None for o.w.
    let target_values = outgoing_edges
        .iter()
        .filter(move |(t, _)| *t == target)
        .map(|(_, c)| c.expect("Unconstrained decision is not expected."))
        .map(|(_, value)| value.as_ref().copied());

    let is_boolean_switch = taken_decision.kind.is_boolean();

    use ConstraintKind::*;

    let case_count = outgoing_edges
        .into_iter()
        .filter(|(_, c)| c.is_some_and(|(_, v)| v.is_some()))
        .count();

    let kinds = target_values
        .map(move |value| {
            if is_boolean_switch {
                let to_kind = |v| {
                    if v == 0 { False } else { True }
                };
                if let Some(value) = value {
                    to_kind(value)
                } else {
                    debug_assert_eq!(outgoing_edges.len(), 2);
                    // Although we know that otherwise case for boolean switches is True,
                    // let's not rely on the assumption.
                    let not_value = outgoing_edges
                        .iter()
                        .find_map(|(_, v)| v.as_ref().and_then(|(_, v)| *v))
                        .unwrap();
                    to_kind(not_value).not()
                }
            } else {
                if let Some(value) = value {
                    OneOf(vec![value])
                } else {
                    OneOf(
                        outgoing_edges
                            .iter()
                            .filter_map(|(_, v)| v.as_ref().and_then(|(_, v)| *v))
                            .collect(),
                    )
                    .not()
                }
            }
        })
        /* NOTE: Reducing or not changes the soundness.
         * Reducing results in weaker constraints, different case values will be explored in separate executions calls.
         * Soundness is dependent on the exclusion of previously seen answers.
         * Not reducing generates more inputs per execution but possible to make no difference. */
        .try_reduce(|acc, k| acc.or(&k, || case_count))
        .expect("At least one edge toward the target is expected.");

    let kinds = kinds.into_iter().map(|k| {
        k.map(|case| {
            let sort = taken_decision.discr.sort();
            let AstNodeSort::BitVector(sort) = sort else {
                unreachable!("Unexpected sort for a non-bool discriminant: {:?}", sort)
            };
            AstNode::BitVector(BVNode(
                z3::ast::BV::from_str(
                    taken_decision.discr.ast().get_ctx(),
                    taken_decision.discr.as_bit_vector().get_size(),
                    &case.to_string(),
                )
                .unwrap(),
                sort,
            ))
        })
    });

    let not_taken = taken_decision.kind.clone().not();

    /* Ensure we find something different.
     * This particularly makes difference when the CFG is merged
     * (we may execute different statements, although the paths merge later). */
    kinds.filter_map(move |k| k.and(&not_taken, || case_count))
}
