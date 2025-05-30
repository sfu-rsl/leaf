// Source: https://github.com/cognitive-engineering-lab/rustc_plugin/blob/main/crates/rustc_utils/src/mir/control_dependencies.rs

//! An algorithm to compute control-dependencies between MIR blocks.
//!
//! In a function $f$, a block $Y$ is control-dependent on a block $X$ if the execution of $Y$
//! is conditional on $X$, e.g. if $X$ is a conditional branch and $Y$ is one of the branches.
//!
//! See Section 3.1 of "The Program Dependence Graph and Its Use in Optimization" (Ferrante et al. 1987)
//! for more on how to define and analyze control-dependence.

use std::fmt;

use rustc_data_structures::graph::{
    ControlFlowGraph, DirectedGraph, Predecessors, StartNode, Successors, dominators,
    dominators::Dominators, iterate, vec_graph::VecGraph,
};
use rustc_index::{
    Idx,
    bit_set::{DenseBitSet, MixedBitSet, SparseBitMatrix},
};
use smallvec::SmallVec;

struct ReversedGraph<'a, G: ControlFlowGraph> {
    graph: &'a G,
    exit: G::Node,
    unreachable: MixedBitSet<G::Node>,
}

impl<G: ControlFlowGraph> DirectedGraph for ReversedGraph<'_, G> {
    type Node = G::Node;

    fn num_nodes(&self) -> usize {
        self.graph.num_nodes()
    }
}

impl<G: ControlFlowGraph> StartNode for ReversedGraph<'_, G> {
    fn start_node(&self) -> Self::Node {
        self.exit
    }
}

impl<G: ControlFlowGraph> Successors for ReversedGraph<'_, G> {
    fn successors(&self, node: Self::Node) -> impl Iterator<Item = Self::Node> {
        self.graph
            .predecessors(node)
            .filter(|bb| !self.unreachable.contains(*bb))
            // We have to collect -> immediately into_iter because we need to return
            // an iterator type that doesn't describe closures, which aren't nameable
            // in the GraphSuccessors trait implementation.
            .collect::<SmallVec<[G::Node; 4]>>()
            .into_iter()
    }
}

impl<G: ControlFlowGraph> Predecessors for ReversedGraph<'_, G> {
    fn predecessors(&self, node: Self::Node) -> impl Iterator<Item = Self::Node> {
        self.graph
            .successors(node)
            .filter(|bb| !self.unreachable.contains(*bb))
            .collect::<SmallVec<[G::Node; 4]>>()
            .into_iter()
    }
}

/// Represents the post-dominators of a graph's nodes with respect to a particular exit.
pub struct PostDominators<Node: Idx> {
    dominators: Dominators<Node>,
    num_nodes: usize,
}

impl<Node: Idx> PostDominators<Node> {
    /// Constructs the post-dominators by computing the dominators on a reversed graph.
    pub fn build<G: ControlFlowGraph<Node = Node>>(graph: &G, exit: Node) -> Self {
        let num_nodes = graph.num_nodes();
        let mut reversed = ReversedGraph {
            graph,
            exit,
            unreachable: MixedBitSet::new_empty(num_nodes),
        };

        let reachable = iterate::post_order_from(&reversed, exit);
        reversed.unreachable.insert_all();
        for n in &reachable {
            reversed.unreachable.remove(*n);
        }

        let dominators = dominators::dominators(&reversed);
        PostDominators {
            dominators,
            num_nodes,
        }
    }

    /// Gets the node that immediately post-dominators `node`, if one exists.
    pub fn immediate_post_dominator(&self, node: Node) -> Option<Node> {
        self.dominators.immediate_dominator(node)
    }

    pub fn is_reachable(&self, node: Node) -> bool {
        self.dominators.is_reachable(node)
    }

    pub fn post_dominates(&self, a: Node, b: Node) -> bool {
        self.dominators.dominates(a, b)
    }

    /// Gets all nodes that post-dominate `node`, if they exist.
    pub fn post_dominators(&self, node: Node) -> Option<impl Iterator<Item = Node> + '_> {
        let reachable = self.dominators.is_reachable(node);
        reachable.then(move || {
            (0..self.num_nodes)
                .map(Node::new)
                .filter(move |other| self.post_dominates(*other, node))
        })
    }
}

/// Represents the control dependencies between all pairs of nodes of a graph.
pub struct ControlDependencies<Node: Idx>(SparseBitMatrix<Node, Node>);

impl<Node: Idx> fmt::Debug for ControlDependencies<Node> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, (bb, bbs)) in self
            .0
            .rows()
            .enumerate()
            .filter_map(|(i, bb)| self.0.row(bb).map(move |bbs| (i, (bb, bbs))))
        {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{bb:?}: {{")?;
            for (j, bb2) in bbs.iter().enumerate() {
                if j > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{bb2:?}")?;
            }
            write!(f, "}}")?;
        }
        Ok(())
    }
}

impl<Node: Idx + Ord> ControlDependencies<Node> {
    /// Compute control dependencies from post-dominator frontier.
    ///
    /// Frontier algorithm from "An Efficient Method of Computing Single Static Assignment Form", Cytron et al. 89
    fn build<G: ControlFlowGraph<Node = Node>>(graph: &G, exit: Node) -> Self {
        let post_dominators = PostDominators::build(graph, exit);
        let idom = |node| post_dominators.immediate_post_dominator(node);

        let n = graph.num_nodes();
        let edges = (0..n)
            .filter_map(|i| {
                let node = Node::new(i);
                Some((idom(node)?, node))
            })
            .collect::<Vec<_>>();

        let dominator_tree: VecGraph<Node, true> = VecGraph::new(n, edges);

        let traversal = iterate::post_order_from(&dominator_tree, exit);

        // Only use size = n b/c exit node shouldn't ever have a dominance frontier
        let mut df = SparseBitMatrix::new(n);
        for x in traversal {
            let local = graph.predecessors(x);
            let up = dominator_tree
                .successors(x)
                .iter()
                .flat_map(|z| df.row(*z).into_iter().flat_map(|set| set.iter()));
            let frontier = local
                .chain(up)
                .filter(|y| idom(*y).map(|yd| yd != x).unwrap_or(false))
                .collect::<Vec<_>>();

            for y in frontier {
                df.insert(x, y);
            }
        }

        ControlDependencies(df)
    }

    /// Compute the union of control dependencies from multiple exits.
    pub fn build_many<G: ControlFlowGraph<Node = Node>>(
        graph: &G,
        exits: impl IntoIterator<Item = Node>,
    ) -> Self {
        let mut all_deps = SparseBitMatrix::new(graph.num_nodes());
        for exit in exits {
            let deps = ControlDependencies::build(graph, exit);
            for node in deps.0.rows() {
                if let Some(set) = deps.0.row(node) {
                    all_deps.union_row(node, set);
                }
            }
        }
        ControlDependencies(all_deps)
    }

    /// Returns the set of all node that are control-dependent on the given `node`.
    pub fn dependent_on(&self, node: Node) -> Option<&DenseBitSet<Node>> {
        self.0.row(node)
    }
}
