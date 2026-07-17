use leaf_runtime::{
    abs::{
        AssignmentId, BasicBlockIndex, CalleeDef, FuncDef, SwitchCaseIndex,
        backend::PhasedCallTraceRecorder, utils::BasicBlockLocationExt,
    },
    call::{CallControlFlowManager, CallFlowManager, DefaultCallFlowManager},
    pri::fluent::backend::{ArgsTupling, CallHandler, DropHandler, RuntimeBackend},
};

use super::{CftBackend, NullOperand, NullPlace, record::Recorder};

pub(super) type CftCallFlowManager = DefaultCallFlowManager<
    <super::CftBackend as RuntimeBackend>::Place,
    <super::CftBackend as RuntimeBackend>::Operand,
    (),
>;

pub(crate) struct CftCallHandler<'a> {
    flow_manager: &'a mut CftCallFlowManager,
    recorder: &'a mut Recorder<SwitchCaseIndex>,
}

impl<'a> CftCallHandler<'a> {
    pub(super) fn new(backend: &'a mut CftBackend) -> Self {
        Self {
            flow_manager: &mut backend.call_flow_manager,
            recorder: &mut backend.recorder,
        }
    }
}

impl CallHandler for CftCallHandler<'_> {
    type Place = NullPlace;
    type Operand = NullOperand;

    fn before_call(self, def: CalleeDef, call_site: BasicBlockIndex) {
        self.flow_manager.prepare_for_calling(def);
        self.recorder
            .start_call(self.flow_manager.current_func().at_basic_block(call_site));
    }

    fn before_call_some(self) {
        self.flow_manager.prepare_for_call();
        self.recorder
            .start_call(self.flow_manager.current_func().at_basic_block(0));
    }

    fn take_data_before_call(
        self,
        _func: Self::Operand,
        _args: impl IntoIterator<Item = Self::Operand>,
        _are_args_tupled: bool,
    ) {
    }

    fn enter(self, def: FuncDef) {
        let sanity = self.flow_manager.enter(def);
        self.recorder.finish_call(def, sanity.is_broken());
    }

    fn emplace_arguments(
        self,
        _arg_places: Vec<Self::Place>,
        _ret_val_place: Self::Place,
        _tupling: ArgsTupling,
    ) {
    }

    fn override_return_value(self, _value: Self::Operand) {}

    fn ret(self, ret_point: BasicBlockIndex) {
        self.recorder
            .start_return(self.flow_manager.current_func().at_basic_block(ret_point));

        self.flow_manager.start_return();
    }

    fn after_call(self, _assignment_id: AssignmentId, _result_dest: Self::Place) {
        let token = self.flow_manager.finalize_call();
        self.recorder
            .finish_return(token.sanity().is_broken().unwrap());
    }
}

impl DropHandler for CftCallHandler<'_> {
    type Place = NullPlace;
    type Operand = NullOperand;

    fn before_drop(self, def: CalleeDef, call_site: BasicBlockIndex) {
        <Self as CallHandler>::before_call(self, def, call_site);
    }

    fn before_drop_some(self) {
        <Self as CallHandler>::before_call_some(self);
    }

    fn take_data_before_drop(self, _func: Self::Operand, _arg: Self::Operand, _place: Self::Place) {
    }

    fn after_drop(self) {
        let token = self.flow_manager.finalize_call();
        self.recorder
            .finish_return(token.sanity().is_broken().unwrap());
    }
}
