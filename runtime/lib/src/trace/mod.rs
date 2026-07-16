// FIXME: Might be too specific to symex backend.

use crate::abs::{Constraint, backend::TraceManager};

mod adapt;
mod agg;
mod coverage;
mod divergence;
mod dump;
mod filter;
pub mod inspect;
mod log;
pub mod sanity_check;

pub use adapt::TraceManagerExt as AdapterTraceManagerExt;
pub use agg::{AggregatorStepInspector, AggregatorTraceManager};
pub use coverage::BranchCoverageStepInspector;
pub use divergence::{
    BranchCoverageDepthDivergenceFilter, DepthProvider, DivergenceFilter,
    ImmediateDivergingAnswerFinder, filter::all as divergence_filter_all,
};
pub use dump::StreamDumperStepInspector;
pub use filter::{
    StepInspectorExt as FilterStepInspectorExt, TraceManagerExt as FilterTraceManagerExt,
};
pub use inspect::{StepInspector, TraceInspector, TraceManagerExt as InspectionTraceManagerExt};
pub use log::TraceManagerExt as LoggerTraceManagerExt;
