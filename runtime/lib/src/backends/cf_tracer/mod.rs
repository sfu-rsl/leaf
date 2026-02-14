mod call;
mod constraint;
mod instance;
mod record;
mod tracing_i;

use crate::{
    abs::{backend::Shutdown, utils::BasicBlockLocationExt},
    call::CallFlowManager,
    pri::fluent::backend::{AssignmentHandler, RuntimeBackend, shared::noop::*},
};

use call::CftCallHandler;
use record::Recorder;

pub use instance::CftInstanceManager;

/// A backend meant for control flow tracing (CFT).
pub(crate) struct CftBackend {
    call_flow_manager: call::CftCallFlowManager,
    recorder: record::Recorder,
}

impl CftBackend {
    pub(crate) fn new() -> Self {
        Self {
            call_flow_manager: Default::default(),
            recorder: Default::default(),
        }
    }
}

impl RuntimeBackend for CftBackend {
    type PlaceHandler<'a>
        = NoOpPlaceHandler<Self::PlaceInfo, Self::Place>
    where
        Self: 'a;

    type OperandHandler<'a>
        = NoOpOperandHandler<Self::Place, Self::Operand>
    where
        Self: 'a;

    type AssignmentHandler<'a>
        = NoOpAssignmentHandler<Self::Place, Self::Operand>
    where
        Self: 'a;

    type MemoryHandler<'a>
        = NoOpMemoryHandler
    where
        Self: 'a;

    type RawMemoryHandler<'a>
        = NoOpRawMemoryHandler<Self::Place, Self::Operand>
    where
        Self: 'a;

    type ConstraintHandler<'a>
        = constraint::CftConstraintHandler<'a>
    where
        Self: 'a;

    type CallHandler<'a>
        = CftCallHandler<'a>
    where
        Self: 'a;

    type AnnotationHandler<'a>
        = NoOpAnnotationHandler
    where
        Self: 'a;

    type PlaceInfo = NullPlace;
    type Place = NullPlace;
    type DiscriminablePlace = NullPlace;

    type Operand = NullOperand;

    fn place(&mut self, _usage: crate::abs::PlaceUsage) -> Self::PlaceHandler<'_> {
        Default::default()
    }

    fn operand(&mut self) -> Self::OperandHandler<'_> {
        Default::default()
    }

    fn assign_to<'a>(
        &'a mut self,
        _id: common::pri::AssignmentId,
        _dest: <Self::AssignmentHandler<'a> as AssignmentHandler>::Place,
    ) -> Self::AssignmentHandler<'a> {
        Default::default()
    }

    fn memory<'a>(&'a mut self) -> Self::MemoryHandler<'a> {
        Default::default()
    }

    fn raw_memory<'a>(&'a mut self) -> Self::RawMemoryHandler<'a> {
        Default::default()
    }

    fn constraint_at(
        &mut self,
        location: common::pri::BasicBlockIndex,
    ) -> Self::ConstraintHandler<'_> {
        constraint::CftConstraintHandler::new(
            self,
            self.call_flow_manager
                .current_func()
                .body_id
                .at_basic_block(location),
        )
    }

    fn call_control(&mut self) -> Self::CallHandler<'_> {
        CftCallHandler::new(self)
    }

    fn annotate(&mut self) -> Self::AnnotationHandler<'_> {
        Default::default()
    }
}

impl Shutdown for CftBackend {
    fn shutdown(&mut self) {}
}

pub(crate) type NullPlace = ();
pub(crate) type NullOperand = ();
