use std::collections::HashMap;

use common::{
    directed::CfgEdgeDestination,
    log_debug, log_info,
    types::trace::{Constraint, ConstraintKind},
    z3::{AstAndVars, AstNode, AstNodeSort, BVNode, WrappedSolver as Z3Solver},
};

use crate::{SwitchStep, two_level::DirectedEdge};

type ParsedConstraint<'ctx> = Constraint<AstAndVars<'ctx, u32>, AstNode<'ctx>>;
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
    ) -> Option<(z3::SatResult, HashMap<u32, AstNode<'ctx>>)> {
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

        let constraints = &self.parsed_trace
            [..=index_of(self.trace, edge.src).expect("Inconsistent referencing")];
        let (last, pre) = constraints.split_last().unwrap();

        // If (symbolic) constraint is available
        let Some(decision) = &last.decision else {
            return None;
        };

        let to_satisfy = construct_constraint_to_satisfy(edge, decision);
        log_debug!("Constraint to satisfy: {to_satisfy}");

        Some(
            self.solver.check(
                pre.iter()
                    .filter_map(|s| s.decision.clone())
                    .chain(core::iter::once(to_satisfy)),
            ),
        )
    }
}

fn construct_constraint_to_satisfy<'ctx>(
    edge: &DirectedEdge<'_, &[CfgEdgeDestination]>,
    decision: &ParsedConstraint<'ctx>,
) -> ParsedConstraint<'ctx> {
    // The value of the discriminant to take the edge. None for o.w.
    let value = edge
        .metadata
        .iter()
        .find(|(bb, _)| *bb == edge.dst)
        .and_then(|(_, c)| c.as_ref())
        .map(|(_, value)| value)
        .copied()
        .expect("Inconsistent CFG information");

    use ConstraintKind::*;
    let kind = if decision.kind.is_boolean() {
        let to_kind = |v| {
            if v == 0 { False } else { True }
        };
        if let Some(value) = value {
            to_kind(value)
        } else {
            debug_assert_eq!(edge.metadata.len(), 2);
            let not_value = edge
                .metadata
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
                edge.metadata
                    .iter()
                    .filter_map(|(_, v)| v.as_ref().and_then(|(_, v)| *v))
                    .collect(),
            )
            .not()
        }
    };
    let kind = kind.map(|case| {
        let sort = decision.discr.sort();
        let AstNodeSort::BitVector(sort) = sort else {
            unreachable!("Unexpected sort for a non-bool discriminant: {:?}", sort)
        };
        AstNode::BitVector(BVNode(
            z3::ast::BV::from_str(
                decision.discr.ast().get_ctx(),
                decision.discr.as_bit_vector().get_size(),
                &case.to_string(),
            )
            .unwrap(),
            sort,
        ))
    });

    Constraint {
        discr: decision.discr.clone(),
        kind,
    }
}

fn index_of<T>(slice: &[T], item: &T) -> Option<usize> {
    let range = slice.as_ptr_range();
    let item_ptr = item as *const T;

    if range.contains(&item_ptr) {
        Some(unsafe { item_ptr.offset_from(range.start) as usize })
    } else {
        None
    }
}
