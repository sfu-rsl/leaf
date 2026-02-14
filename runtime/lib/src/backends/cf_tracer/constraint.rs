use common::types::trace::ConstraintKind;

use crate::{
    abs::{BasicBlockLocation, Constant, backend::DecisionTraceRecorder},
    pri::fluent::backend::{ConstraintHandler, SwitchHandler},
};

use super::{CftBackend, NullOperand, Recorder};

pub(crate) struct CftConstraintHandler<'a> {
    recorder: &'a mut Recorder,
    node_location: BasicBlockLocation,
}

impl<'a> CftConstraintHandler<'a> {
    pub(crate) fn new(backend: &'a mut CftBackend, node_location: BasicBlockLocation) -> Self {
        Self {
            recorder: &mut backend.recorder,
            node_location,
        }
    }
}

pub(crate) struct CftSwitchHandler<'a> {
    recorder: &'a mut Recorder,
    node_location: BasicBlockLocation,
}

impl<'a> ConstraintHandler for CftConstraintHandler<'a> {
    type Operand = NullOperand;

    type SwitchHandler = CftSwitchHandler<'a>;

    fn switch(self, _discriminant: Self::Operand) -> Self::SwitchHandler {
        CftSwitchHandler {
            node_location: self.node_location,
            recorder: self.recorder,
        }
    }

    fn assert(
        self,
        _cond: Self::Operand,
        expected: bool,
        _assert_kind: crate::abs::AssertKind<Self::Operand>,
    ) {
        self.recorder.notify_decision(
            self.node_location,
            &if expected {
                ConstraintKind::True
            } else {
                ConstraintKind::False
            },
        );
    }
}

impl<'a> SwitchHandler for CftSwitchHandler<'a> {
    fn take(self, value: Constant) {
        self.recorder
            .notify_decision(self.node_location, &ConstraintKind::OneOf(vec![value]));
    }

    fn take_otherwise(self, non_values: Vec<Constant>) {
        self.recorder
            .notify_decision(self.node_location, &ConstraintKind::NoneOf(non_values));
    }
}
