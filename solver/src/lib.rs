pub mod backend;

pub use crate::backend::z3::{AstAndVars, AstNode, BVNode, Z3Solver, set_global_params};
pub use crate::solver::{Model, SolveResult, Solver};
pub use common::types::trace::{Constraint, ConstraintKind};

#[cfg(feature = "serde")]
pub use crate::format::{ModelFormat, OutputFormat};

// Re-export essential Z3 types for standalone usage
pub use z3::ast::{self, Ast};
pub use z3::{Config, Context};

pub mod solver {
    use common::types::trace::Constraint;
    use std::collections::HashMap;

    /// Core solver trait that all backend implementations must provide
    pub trait Solver {
        type Value;
        type Case;
        type Model;

        /// Check satisfiability of the given constraints
        fn check(
            &mut self,
            constraints: impl Iterator<Item = Constraint<Self::Value, Self::Case>>,
        ) -> SolveResult<Self::Model>;
    }

    pub type Model<I, V> = HashMap<I, V>;

    /// The result of the checking performed by [`Solver`]
    pub enum SolveResult<M> {
        Sat(M),
        Unsat,
        Unknown,
    }
}

// Serialization support for binary interface
#[cfg(feature = "serde")]
pub mod format {
    use crate::backend::z3::serdes::SmtLibExpr;
    use common::types::trace::Constraint;
    use serde::{Deserialize, Serialize};

    /// Input format for JSONL constraint files
    #[derive(Debug, Serialize, Deserialize)]
    pub struct ConstraintEntry {
        pub step: StepInfo,
        pub constraint: Constraint<SmtLibExpr, SmtLibExpr>,
    }

    /// Step information from the execution trace
    #[derive(Debug, Serialize, Deserialize)]
    pub struct StepInfo {
        pub value: String,
        pub index: u32,
    }

    /// Output format configuration
    #[derive(Debug, Clone, Copy, clap::ValueEnum)]
    pub enum OutputFormat {
        /// Standard format with SmtLibExpr values
        Standard,
        /// Raw bytes format - variables mapped to byte values
        Bytes,
    }

    /// Model representation based on output format
    #[derive(Debug, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum ModelFormat {
        Standard(std::collections::HashMap<String, SmtLibExpr>),
        Bytes(std::collections::HashMap<String, u8>),
    }

    /// Output format for solver results
    #[derive(Debug, Serialize, Deserialize)]
    pub struct SolverOutput {
        pub result: String, // "sat", "unsat", or "unknown"
        pub model: Option<ModelFormat>,
    }
}
