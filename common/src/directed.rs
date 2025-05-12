use core::hash::Hash;
use std::{collections::HashMap, path::Path, string::String, vec::Vec};

use serde::{Deserialize, Serialize};

pub use super::types::{BasicBlockIndex, DefId};
use super::utils::MessagedError;

type AdjListGraph<V, E> = HashMap<V, Vec<E>>;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum CfgConstraint {
    Case(u128),
    Otherwise,
}

pub type CfgEdgeDestination = (BasicBlockIndex, Option<CfgConstraint>); // Target, Possible constraint
pub type ControlFlowGraph = AdjListGraph<BasicBlockIndex, CfgEdgeDestination>;
pub type CallGraphEdgeDestination<I> = (BasicBlockIndex, I);
pub type CallGraph<I = DefId> = AdjListGraph<I, CallGraphEdgeDestination<I>>;

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct DebugInfo<I: Eq + Hash> {
    pub func_names: HashMap<I, String>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct ProgramMap<I: Eq + Hash = DefId> {
    pub cfgs: HashMap<I, ControlFlowGraph>,
    pub ret_points: HashMap<I, Vec<BasicBlockIndex>>,
    pub call_graph: CallGraph<I>,
    pub entry_points: Vec<I>,
    pub debug_info: DebugInfo<I>,
}

impl ProgramMap {
    pub fn read(path: &Path) -> Result<Self, MessagedError> {
        let file =
            std::fs::OpenOptions::new()
                .read(true)
                .open(path)
                .map_err(MessagedError::with(
                    "Failed to open file for reading program map",
                ))?;

        let result = serde_json::from_reader(file).map_err(MessagedError::with(
            "Failed to parse program map from file.",
        ))?;
        Ok(result)
    }

    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), MessagedError> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .map_err(MessagedError::with(
                "Failed to open file for writing program map",
            ))?;
        let mut serializer = serde_json::Serializer::pretty(file);
        let serializable = self.clone();
        serializable
            .serialize(&mut serializer)
            .map_err(MessagedError::with(
                "Failed to serialize program map to file.",
            ))
    }
}
