use core::hash::Hash;
use std::{collections::HashMap, path::Path, string::String, vec::Vec};

use serde::{Deserialize, Serialize};

pub use super::types::{BasicBlockIndex, DefId};
use super::utils::StrError;

type AdjListGraph<V, E> = HashMap<V, Vec<E>>;

pub type Constraint = (usize, Option<u128>); // Target index, Target value (None for o.w.)

pub type CfgEdgeDestination = (BasicBlockIndex, Option<Constraint>); // Target index, Possible constraint
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
    pub call_graph: CallGraph<I>,
    pub entry_points: Vec<I>,
    pub debug_info: DebugInfo<I>,
}

impl ProgramMap {
    pub fn read(path: &Path) -> Result<Self, StrError> {
        let file =
            std::fs::OpenOptions::new()
                .read(true)
                .open(path)
                .map_err(StrError::with_message(
                    "Failed to open file for reading program map",
                ))?;

        let result = serde_json::from_reader(file).map_err(StrError::with_message(
            "Failed to parse program map from file.",
        ))?;
        Ok(result)
    }

    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), StrError> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .map_err(StrError::with_message(
                "Failed to open file for writing program map",
            ))?;
        let mut serializer = serde_json::Serializer::pretty(file);
        let serializable = self.clone();
        serializable
            .serialize(&mut serializer)
            .map_err(StrError::with_message(
                "Failed to serialize program map to file.",
            ))
    }
}
