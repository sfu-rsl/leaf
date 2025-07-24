use derive_more as dm;
use serde::Deserialize;

use std::{collections::HashMap, num::NonZero};

use common::{log_debug, log_warn};

use crate::utils::{alias::check_sym_value_loss, file::FileGenConfig};

impl TryFrom<::config::Config> for BasicBackendConfig {
    type Error = ::config::ConfigError;

    fn try_from(value: ::config::Config) -> Result<Self, Self::Error> {
        let config: BasicBackendConfig = value.try_deserialize()?;
        log_debug!("Loaded configurations: {:?}", config);

        use ExternalCallStrategy::*;
        if !check_sym_value_loss!()
            && matches!(
                config.call.external_call,
                OptimisticConcretization | OverApproximation
            )
        {
            log_warn!(
                concat!(
                    "Using {} for external calls handling requires symbolic value loss checks to be enabled, ",
                    "which is disabled in this build of the library.",
                ),
                config.call.external_call,
            );
        }

        Ok(config)
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct BasicBackendConfig {
    #[serde(default)]
    pub call: CallConfig,

    #[serde(default)]
    pub sym_place: SymbolicPlaceConfig,

    #[serde(default)]
    pub outputs: Vec<OutputConfig>,

    #[serde(default)]
    pub exe_trace: ExecutionTraceConfig,

    #[serde(default)]
    pub solver: SolverImpl,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct CallConfig {
    #[serde(default)]
    pub external_call: ExternalCallStrategy,
}

/* NOTE: Aliases don't work at the moment. */
#[derive(
    Debug, dm::Display, Default, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExternalCallStrategy {
    #[serde(alias = "panic")]
    Panic,
    #[default]
    #[serde(alias = "conc", alias = "concretize", alias = "underapprox")]
    Concretization,
    #[serde(alias = "overapprox", alias = "overapproximate")]
    OverApproximation,
    #[serde(alias = "opt_conc")]
    OptimisticConcretization,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct SymbolicPlaceConfig {
    #[serde(default)]
    pub read: SymbolicPlaceStrategy,
    #[serde(default)]
    pub write: SymbolicPlaceStrategy,
}

#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SymbolicPlaceStrategy {
    /// Panics if the place is symbolic.
    /// # Remarks
    /// This is useful for sanity checking purposes because it is not surprising to
    /// face symbolic pointers (e.g., in printing) in real programs even they do not
    /// explicitly access the memory symbolically.
    Panic,
    /// Builds an expression with multiple possible values.
    #[serde(alias = "proj", alias = "expr")]
    ProjExpression,
    /// Uses the concrete value of place for memory reading/writing.
    #[serde(alias = "conc", alias = "concretize", alias = "underapprox")]
    Concretization,
    /// Uses the concrete value of place for memory reading/writing then adds
    /// a constraint for the symbolic place to be equal to its concrete value.
    /// For example, if the symbolic place is generated because of a symbolic
    /// index `i` which currently has the value of `2`, then the slice is
    /// accessed at index `2` and the constraint of `i == 2` is added.
    #[default]
    #[serde(alias = "stamp")]
    Stamping,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutputConfig {
    File(FileGenConfig),
}

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct ExecutionTraceConfig {
    #[serde(default = "default_trace_inspectors")]
    pub inspectors: Vec<TraceInspectorType>,

    #[serde(default = "default_constraint_filters")]
    pub constraint_filters: Vec<ConstraintFilterType>,

    #[serde(default)]
    pub control_flow_dump: Option<OutputConfig>,

    #[serde(default)]
    pub constraints_dump: Option<OutputConfig>,

    #[serde(default)]
    pub preconditions_dump: Option<OutputConfig>,

    /// The time interval between dumping inspectors' data (e.g., snapshots) during the execution
    /// and not merely in the end.
    /// # Remarks
    /// - This is useful for executions that possibly take long to finish and may get stopped
    /// prematurely.
    /// - This works as a global config for any dumping happening in the trace management.
    #[serde(default)]
    pub dump_interval: Option<NonZero<u64>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub(crate) enum TraceInspectorType {
    SanityChecker {
        #[serde(default)]
        level: ConstraintSanityCheckLevel,
        #[serde(default)]
        output: Option<OutputConfig>,
    },
    DivergingInput {
        #[serde(default = "default_diverging_input_check_optimistic")]
        check_optimistic: bool,
        #[serde(default)]
        filters: Vec<DivergenceFilterType>,
    },
    BranchCoverage {
        #[serde(default)]
        output: Option<OutputConfig>,
    },
}

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConstraintSanityCheckLevel {
    #[default]
    Warn,
    Panic,
}

fn default_diverging_input_check_optimistic() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub(crate) enum DivergenceFilterType {
    Tags {
        exclude_any_of: Vec<String>,
    },
    BranchDepthDistance {
        #[serde(default = "default_branch_depth_distance_factor")]
        distance_threshold_factor: f32,
        #[serde(default)]
        persistence: Option<OutputConfig>,
    },
}

fn default_branch_depth_distance_factor() -> f32 {
    2.0
}

fn default_trace_inspectors() -> Vec<TraceInspectorType> {
    vec![
        #[cfg(debug_assertions)]
        TraceInspectorType::SanityChecker {
            level: ConstraintSanityCheckLevel::Panic,
            output: None,
        },
        TraceInspectorType::DivergingInput {
            check_optimistic: default_diverging_input_check_optimistic(),
            filters: vec![],
        },
    ]
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConstraintFilterType {
    SanityChecker { output: Option<OutputConfig> },
}

fn default_constraint_filters() -> Vec<ConstraintFilterType> {
    vec![
        #[cfg(debug_assertions)]
        ConstraintFilterType::SanityChecker { output: None },
    ]
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub(crate) enum SolverImpl {
    Z3 {
        #[serde(flatten)]
        config: Z3Config,
    },
}

impl Default for SolverImpl {
    fn default() -> Self {
        Self::Z3 {
            config: Default::default(),
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct Z3Config {
    #[serde(default)]
    pub global_params: HashMap<String, ParamValue>,
}

#[derive(Debug, Clone, Deserialize, dm::Display)]
#[serde(untagged)]
#[display("{_0}")]
pub(crate) enum ParamValue {
    Bool(bool),
    Uint(u32),
    Double(f64),
    String(String),
}
