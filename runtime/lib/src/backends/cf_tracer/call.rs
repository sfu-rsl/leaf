use crate::{
    abs::{
        AssignmentId, BasicBlockIndex, CalleeDef, FuncDef, backend::PhasedCallTraceRecorder,
        utils::BasicBlockLocationExt,
    },
    backends::cf_tracer::{CftBackend, record::Recorder},
    call::{
        CallFlowManager, CallFlowSanity, CallShadowMemory, DefaultCallFlowManager,
        NoOpCallFlowBreakageCallback, SignaturePlaces,
    },
    pri::fluent::backend::{ArgsTupling, CallHandler, RuntimeBackend},
};

use super::{NullOperand, NullPlace};

pub(super) type BCallback = NoOpCallFlowBreakageCallback<fn() -> super::NullOperand>;

pub(super) type CftCallFlowManager = DefaultCallFlowManager<
    <super::CftBackend as RuntimeBackend>::Place,
    super::NullOperand,
    BCallback,
>;

pub(super) struct NoOpCallShadowMemory;

impl CallShadowMemory<super::NullPlace> for NoOpCallShadowMemory {
    type Value = super::NullOperand;

    fn take_place(&mut self, _place: &super::NullPlace) -> Self::Value {
        super::NullOperand::default()
    }

    fn set_place(&mut self, _place: &super::NullPlace, _value: Self::Value) {}

    fn set_args(&mut self, _places: &[super::NullPlace], _values: Vec<Self::Value>) {}
}

pub(crate) struct CftCallHandler<'a> {
    flow_manager: &'a mut CftCallFlowManager,
    recorder: &'a mut Recorder,
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
    type Arg = Self::Operand;

    type MetadataHandler = ();

    fn before_call(
        self,
        def: CalleeDef,
        call_site: BasicBlockIndex,
        func: Self::Operand,
        args: impl IntoIterator<Item = Self::Arg>,
        are_args_tupled: bool,
    ) {
        self.flow_manager
            .prepare_for_call(def, func, args.into_iter().collect(), are_args_tupled);
        self.recorder
            .start_call(self.flow_manager.current_func().at_basic_block(call_site));
    }

    fn enter(
        self,
        def: FuncDef,
        arg_places: Vec<Self::Place>,
        ret_val_place: Self::Place,
        _tupling: ArgsTupling,
    ) {
        let token = self.flow_manager.start_enter(
            def,
            SignaturePlaces {
                arg_places,
                return_val_place: ret_val_place,
            },
        );
        let sanity = self.flow_manager.finalize_enter(
            token,
            crate::call::NoOpArgsTuplingInfoProvider,
            &mut NoOpCallShadowMemory,
        );

        self.recorder
            .finish_call(def, matches!(sanity, CallFlowSanity::Broken));
    }

    fn override_return_value(self, value: Self::Operand) {
        self.flow_manager.override_return_value(value);
    }

    fn ret(self, ret_point: BasicBlockIndex) {
        self.recorder
            .start_return(self.flow_manager.current_func().at_basic_block(ret_point));

        self.flow_manager.start_return(&mut NoOpCallShadowMemory);
    }

    fn after_call(self, _assignment_id: AssignmentId, _result_dest: Self::Place) {
        let (_, sanity) = self.flow_manager.finalize_call();

        self.recorder
            .finish_return(matches!(sanity, CallFlowSanity::Broken));
    }

    fn metadata(self) -> Self::MetadataHandler {
        Default::default()
    }
}
