use core::hash::Hash;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::types::InstanceKindId;
pub use super::types::{AssignmentId, AssignmentIdMap, BasicBlockIndex, trace::RawCaseValue};
use super::utils::MessagedError;

mod cfg {
    use core::range::RangeInclusive;
    use std::{collections::HashMap, vec::Vec};

    use serde::{Deserialize, Serialize};

    use super::{BasicBlockIndex, RawCaseValue};

    #[derive(Clone, Copy, Debug, Serialize, Deserialize)]
    pub enum CfgConstraint {
        Case(RawCaseValue),
        Otherwise,
    }

    pub type CfgEdge = (BasicBlockIndex, Option<CfgConstraint>); // Target node ID, possible constraint

    type CfgNodeRange = RangeInclusive<BasicBlockIndex>;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct CfgNode {
        #[serde(with = "serdes::cfg_node_range")]
        block_range: CfgNodeRange,
        outgoing_edges: Vec<CfgEdge>,
    }

    #[derive(Clone, Debug, Default)]
    pub struct ControlFlowGraph {
        nodes: HashMap<BasicBlockIndex, CfgNode>,
        node_by_block: HashMap<BasicBlockIndex, BasicBlockIndex>,
    }

    impl ControlFlowGraph {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn insert_range(
            &mut self,
            block_range: RangeInclusive<BasicBlockIndex>,
            outgoing_edges: Vec<CfgEdge>,
        ) -> Result<Option<Vec<CfgEdge>>, &'static str> {
            let start = block_range.start;
            let end_inclusive = block_range.last;
            if start > end_inclusive {
                return Err("CFG node range must not be empty");
            }

            for block in start..=end_inclusive {
                if self
                    .node_by_block
                    .get(&block)
                    .is_some_and(|existing_node| *existing_node != start)
                {
                    return Err("CFG node ranges must not overlap");
                }
            }

            let previous = self.nodes.remove(&start);
            if let Some(previous) = &previous {
                for block in previous.block_range {
                    self.node_by_block.remove(&block);
                }
            }

            let node = CfgNode {
                block_range,
                outgoing_edges,
            };
            for block in block_range {
                self.node_by_block.insert(block, start);
            }
            self.nodes.insert(start, node);

            Ok(previous.map(|node| node.outgoing_edges))
        }

        pub fn node_containing(&self, block: BasicBlockIndex) -> Option<BasicBlockIndex> {
            self.node_by_block.get(&block).copied()
        }

        pub fn get(&self, node_id: BasicBlockIndex) -> Option<&Vec<CfgEdge>> {
            self.nodes.get(&node_id).map(|node| &node.outgoing_edges)
        }

        pub fn iter(&self) -> impl Iterator<Item = (&BasicBlockIndex, &Vec<CfgEdge>)> {
            self.nodes
                .iter()
                .map(|(node_id, node)| (node_id, &node.outgoing_edges))
        }
    }

    mod serdes {
        use std::collections::HashMap;

        use serde::{Deserialize, Deserializer, Serialize, Serializer};

        use super::{BasicBlockIndex, CfgNode, ControlFlowGraph, RangeInclusive};

        impl Serialize for ControlFlowGraph {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                self.nodes.serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for ControlFlowGraph {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                use serde::de::Error as _;

                let nodes = HashMap::<BasicBlockIndex, CfgNode>::deserialize(deserializer)?;
                let mut cfg = Self::new();
                for (node_id, node) in nodes {
                    if node_id != node.block_range.start {
                        return Err(D::Error::custom(
                            "CFG node ID must match the start of its block range",
                        ));
                    }
                    cfg.insert_range(
                        RangeInclusive::from(node_id..=node.block_range.last),
                        node.outgoing_edges,
                    )
                    .map_err(D::Error::custom)?;
                }
                Ok(cfg)
            }
        }

        pub(super) mod cfg_node_range {
            use super::*;

            use super::super::CfgNodeRange;

            pub fn serialize<S>(range: &CfgNodeRange, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                core::ops::RangeInclusive::new(range.start, range.last).serialize(serializer)
            }

            pub fn deserialize<'de, D>(deserializer: D) -> Result<CfgNodeRange, D::Error>
            where
                D: Deserializer<'de>,
            {
                let range =
                    core::ops::RangeInclusive::<BasicBlockIndex>::deserialize(deserializer)?;
                Ok(range.into())
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::{vec, vec::Vec};

        use super::*;

        fn branching_cfg() -> ControlFlowGraph {
            let mut cfg = ControlFlowGraph::new();
            cfg.insert_range(
                core::range::RangeInclusive::from(2..=4),
                vec![(6, None), (9, None)],
            )
            .unwrap();
            cfg.insert_range(core::range::RangeInclusive::from(6..=7), vec![(12, None)])
                .unwrap();
            cfg.insert_range(core::range::RangeInclusive::from(9..=10), vec![(12, None)])
                .unwrap();
            cfg.insert_range(core::range::RangeInclusive::from(12..=13), Vec::new())
                .unwrap();
            cfg
        }

        #[test]
        fn node_lookup_resolves_ranges_and_branch_join_targets() {
            let cfg = branching_cfg();

            for block in 2..=4 {
                assert_eq!(cfg.node_containing(block), Some(2));
            }
            for block in 6..=7 {
                assert_eq!(cfg.node_containing(block), Some(6));
            }
            for block in 9..=10 {
                assert_eq!(cfg.node_containing(block), Some(9));
            }
            for block in 12..=13 {
                assert_eq!(cfg.node_containing(block), Some(12));
            }
            assert_eq!(cfg.node_containing(99), None);

            let branch_edges = cfg.get(cfg.node_containing(3).unwrap()).unwrap();
            assert_eq!(branch_edges[0].0, 6);
            assert_eq!(branch_edges[1].0, 9);
        }

        #[test]
        fn deserialization_rebuilds_block_lookup() {
            let cfg = branching_cfg();
            let encoded = serde_json::to_string(&cfg).unwrap();
            let decoded: ControlFlowGraph = serde_json::from_str(&encoded).unwrap();

            assert_eq!(decoded.node_containing(3), Some(2));
            assert_eq!(decoded.node_containing(13), Some(12));
            assert_eq!(decoded.node_containing(99), None);
            assert_eq!(decoded.iter().count(), 4);
        }
    }
}

pub use cfg::{CfgConstraint, CfgEdge, ControlFlowGraph};

mod call_graph {
    use serde::{Deserialize, Serialize};

    use super::BasicBlockIndex;
    use crate::types::{AdjListGraph, InstanceKindId};
    use std::string::String;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct CallDebugInfo {
        pub location: String,
    }

    pub type CallGraphEdgeDestination<I = InstanceKindId> = (BasicBlockIndex, I, CallDebugInfo);
    pub type CallGraph<I = InstanceKindId> = AdjListGraph<I, CallGraphEdgeDestination<I>>;
}

pub use call_graph::{CallDebugInfo, CallGraph, CallGraphEdgeDestination};

mod return_points {
    use super::BasicBlockIndex;
    use std::vec::Vec;

    pub type ReturnPoints = Vec<BasicBlockIndex>;
}

pub use return_points::ReturnPoints;

mod entry_points {
    use std::vec::Vec;

    pub type EntryPoints<I> = Vec<I>;
}

pub use entry_points::EntryPoints;

mod debug_info {
    use core::hash::Hash;
    use std::{collections::HashMap, string::String};

    use serde::{Deserialize, Serialize};

    #[derive(Default, Clone, Serialize, Deserialize)]
    pub struct DebugInfo<I: Eq + Hash> {
        pub func_names: HashMap<I, String>,
    }
}

pub use debug_info::DebugInfo;

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct ProgramMap<I: Eq + Hash = InstanceKindId> {
    pub cfgs: HashMap<I, ControlFlowGraph>,
    #[serde(default)]
    pub assignment_id_maps: HashMap<I, AssignmentIdMap>,
    pub ret_points: HashMap<I, ReturnPoints>,
    pub call_graph: CallGraph<I>,
    pub entry_points: EntryPoints<I>,
    pub debug_info: DebugInfo<I>,
}

pub mod rw {
    use core::hash::Hash;
    use std::{
        fs::OpenOptions,
        path::{Path, PathBuf},
    };

    use serde::{Serialize, de::DeserializeOwned};

    use super::{MessagedError, ProgramMap};

    pub const FILENAME_MAP: &str = "program_map.json";
    pub const EXTENSION_MAP: &str = "json";

    pub fn read_program_map_from<I>(path: impl AsRef<Path>) -> Result<ProgramMap<I>, MessagedError>
    where
        I: Eq + Hash + Default + DeserializeOwned,
    {
        let file =
            OpenOptions::new()
                .read(true)
                .open(path.as_ref())
                .map_err(MessagedError::with(
                    "Failed to open file for reading program map",
                ))?;

        serde_json::from_reader(file).map_err(MessagedError::with(
            "Failed to parse program map from file.",
        ))
    }

    pub fn write_program_map_to<I>(
        program_map: &ProgramMap<I>,
        out_file: impl AsRef<Path>,
    ) -> Result<PathBuf, MessagedError>
    where
        I: Eq + Hash + Serialize,
    {
        let path = out_file.as_ref();
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .map_err(MessagedError::with(
                "Failed to open file for writing program map",
            ))?;

        let mut serializer = serde_json::Serializer::pretty(file);
        program_map
            .serialize(&mut serializer)
            .map(|_| path.to_path_buf())
            .map_err(MessagedError::with(
                "Failed to serialize program map to file.",
            ))
    }
}
