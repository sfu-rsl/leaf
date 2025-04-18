use crate::abs::{Constraint, backend::TraceManager};

mod adapt;
mod agg;
mod coverage;
pub(crate) mod divergence;
mod filter;
pub(crate) mod inspect;
mod log;
pub(crate) mod sanity_check;

pub(crate) use adapt::TraceManagerExt as AdapterTraceManagerExt;
pub(crate) use agg::AggregatorTraceManager;
pub(crate) use coverage::BranchCoverageStepInspector;
pub(crate) use filter::StepInspectorExt as FilterStepInspectorExt;
pub(crate) use inspect::{
    StepInspector, TraceInspector, TraceManagerExt as InspectionTraceManagerExt,
};
pub(crate) use log::TraceManagerExt as LoggerTraceManagerExt;
