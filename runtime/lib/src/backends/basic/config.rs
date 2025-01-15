use derive_more as dm;
use serde::Deserialize;

use std::collections::HashMap;

use common::log_debug;

impl TryFrom<::config::Config> for super::BasicBackend {
    type Error = ::config::ConfigError;

    fn try_from(value: ::config::Config) -> Result<Self, Self::Error> {
        let config = value.try_deserialize()?;
        log_debug!("Loaded configurations: {:?}", config);
        Ok(Self::new(config))
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
#[derive(Debug, Default, Clone, Deserialize, serde::Serialize)]
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
    File(FileOutputConfig),
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutputFileFormat {
    #[default]
    Json,
    /// # Remarks
    /// Requires all symbolic values to be byte (u8).
    Binary,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct FileOutputConfig {
    /// The folder to write file outputs to.
    pub directory: std::path::PathBuf,
    /// The format to write the file outputs in.
    pub format: OutputFileFormat,
    /// The prefix to use for the name of the output files.
    pub prefix: Option<String>,
    /// The extension to use for the name of the output files.
    pub extension: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct ExecutionTraceConfig {
    #[serde(default = "default_trace_inspectors")]
    pub inspectors: Vec<TraceInspectorType>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub(crate) enum TraceInspectorType {
    SanityChecker {
        level: ConstraintSanityCheckLevel,
    },
    DivergingInput {
        #[serde(default = "default_diverging_input_check_optimistic")]
        check_optimistic: bool,
    },
    BranchCoverage {
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

fn default_trace_inspectors() -> Vec<TraceInspectorType> {
    vec![
        TraceInspectorType::SanityChecker {
            level: ConstraintSanityCheckLevel::Panic,
        },
        TraceInspectorType::DivergingInput {
            check_optimistic: default_diverging_input_check_optimistic(),
        },
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
