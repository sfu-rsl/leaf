use derive_more as dm;

use core::fmt::Debug;

use common::pri::BasicBlockLocation;

use crate::backends::basic as backend;
use crate::utils::alias::RRef;

mod constraints;
pub(crate) use constraints::create_trace_manager;

mod record;
pub(crate) use record::{BasicExeTraceRecorder, create_trace_recorder};

mod query;
pub(crate) use query::default_trace_querier;

// FIXME: Rename
#[derive(
    PartialEq,
    Eq,
    Hash,
    Clone,
    Copy,
    Debug,
    Default,
    dm::Deref,
    dm::From,
    dm::Into,
    dm::Display,
    serde::Serialize,
    serde::Deserialize,
)]
pub(crate) struct Step(BasicBlockLocation);
