use delegate::delegate;

use crate::{
    abs::{
        AssignmentId, BasicBlockIndex, CalleeDef, Constant, FuncDef, Local,
        backend::PhasedCallTraceRecorder, utils::BasicBlockLocationExt,
    },
    backends::mdsan::{MdMemoryState, state::MdState},
    call::{
        CallControlFlowManager, CallDataFlowManager, CallFlowManager, CallShadowMemory,
        DefaultCallFlowManager, SignaturePlaces, tupling::ArgsTuplingInfo,
    },
    pri::fluent::backend::{ArgsTupling, CallHandler, DropHandler},
    utils::InPlaceSelfHierarchical,
};

use super::alias::backend;
use backend::{MdSanBackend, MdSanPlaceValue, MdSanValue, MdSanVariablesState, TypeDatabase};
use common::log_info;

#[derive(Default)]
pub(super) struct StackData {
    latest_dropped_place: Option<MdSanPlaceValue>,
}

pub(super) type MdSanCallFlowManager =
    DefaultCallFlowManager<MdSanPlaceValue, MdSanValue, breakage::MdSanBreakageCallback, StackData>;

pub(crate) fn default_flow_manager() -> MdSanCallFlowManager
where
    MdSanCallFlowManager: CallControlFlowManager
        + CallDataFlowManager<Place = MdSanPlaceValue, Value = MdSanValue, StackStorage = StackData>,
{
    DefaultCallFlowManager::new(breakage::MdSanBreakageCallback {})
}

pub(crate) struct MdSanCallHandler<'a> {
    flow_manager: &'a mut MdSanCallFlowManager,
    variables_state: &'a mut MdSanVariablesState,
    variables_state_factory: &'a dyn Fn() -> MdSanVariablesState,
    type_manager: &'a dyn TypeDatabase,
}

impl<'a> MdSanCallHandler<'a> {
    pub(super) fn new(backend: &'a mut MdSanBackend) -> Self {
        Self {
            flow_manager: &mut backend.call_flow_manager,
            variables_state: &mut backend.vars_state,
            variables_state_factory: &backend.vars_state_factory,
            type_manager: backend.type_manager.as_ref(),
        }
    }

    fn current_func(&self) -> FuncDef {
        self.flow_manager.current_func()
    }
}

impl<'a> CallHandler for MdSanCallHandler<'a> {
    type Place = MdSanPlaceValue;
    type Operand = MdSanValue;
    type MetadataHandler = ();

    fn before_call(mut self, def: CalleeDef, call_site: BasicBlockIndex) {
        self.flow_manager.prepare_for_calling(def);
    }

    fn before_call_some(mut self) {
        self.flow_manager.prepare_for_call();
    }

    fn take_data_before_call(
        self,
        func: Self::Operand,
        args: impl IntoIterator<Item = Self::Operand>,
        are_args_tupled: bool,
    ) {
        self.flow_manager.prepare_for_call_with_values(
            func,
            args.into_iter().collect::<Vec<_>>(),
            are_args_tupled,
        );
    }

    fn enter(mut self, def: FuncDef) {
        let _sanity = self.flow_manager.enter(def);
    }

    fn emplace_arguments(
        self,
        arg_places: Vec<Self::Place>,
        ret_val_place: Self::Place,
        tupling: ArgsTupling,
    ) {
        let tupling_info = Self::make_lazy_tupling_info(
            tupling,
            &arg_places,
            self.type_manager,
            self.variables_state_factory,
        );

        self.flow_manager.emplace_args(
            SignaturePlaces {
                args: arg_places.into_iter().collect(),
                return_val: ret_val_place,
            },
            tupling_info,
            self.variables_state,
        );
    }

    #[inline]
    fn override_return_value(self, value: Self::Operand) {
        self.flow_manager.override_return_value(value)
    }

    #[inline]
    fn ret(self, _ret_point: BasicBlockIndex) {
        if self.flow_manager.current_func().body_id.1 == common::pri::DefId(2, 2225) {}

        let token = self.flow_manager.start_return();
        self.flow_manager
            .grab_return_value(token, self.variables_state);
    }

    fn after_call(self, _assignment_id: AssignmentId, result_dest: Self::Place) {
        let token = self.flow_manager.finalize_call();
        let return_val = self.flow_manager.give_return_value(token);

        CallShadowMemory::set_place(self.variables_state, &result_dest, return_val);
    }

    fn metadata(self) -> Self::MetadataHandler {
        Default::default()
    }
}

impl DropHandler for MdSanCallHandler<'_> {
    type Place = MdSanPlaceValue;
    type Operand = MdSanValue;

    fn before_drop(self, def: CalleeDef, call_site: BasicBlockIndex) {
        <Self as CallHandler>::before_call(self, def, call_site);
    }

    fn before_drop_some(self) {
        <Self as CallHandler>::before_call_some(self);
    }

    fn take_data_before_drop(self, func: Self::Operand, arg: Self::Operand, place: Self::Place) {
        self.flow_manager.current_storage().latest_dropped_place = Some(place);
        <Self as CallHandler>::take_data_before_call(self, func, vec![arg], false);
    }

    fn after_drop(self) {
        let token = self.flow_manager.finalize_call();

        let _ = self.flow_manager.give_return_value(token);

        let dropped_place = self
            .flow_manager
            .current_storage()
            .latest_dropped_place
            .take()
            .expect("Inconsistent instrumentation.");
        self.variables_state.mark_place_dropped(&dropped_place);
    }
}

mod tupling {

    use crate::{
        abs::{FieldIndex, Local, PlaceUsage, RawAddress, TypeId, place::HasMetadata},
        backends::mdsan::MdSanPlaceInfo,
        call::{
            tupling::TuplingHelper,
            tupling_utils::{TuplingHelperTypeUtils, make_lazy_tupling_info},
        },
    };

    use super::*;
    use backend::TypeDatabase;

    pub(crate) struct TuplingHelperImpl<'a> {
        type_manager: &'a dyn TypeDatabase,
        temp_vars_state: MdSanVariablesState,
        type_utils: TuplingHelperTypeUtils<'a, TypeId>,
    }

    impl<'a> CallShadowMemory<MdSanPlaceValue> for TuplingHelperImpl<'a> {
        type Value = MdSanValue;

        delegate! {
            #[through(CallShadowMemory::<MdSanPlaceValue>)]
            to &mut self.temp_vars_state {
                fn take_place(&mut self, place: &MdSanPlaceValue) -> Self::Value;
                fn set_place(&mut self, place: &MdSanPlaceValue, value: Self::Value);
            }
        }
    }

    impl TuplingHelper<MdSanPlaceValue> for TuplingHelperImpl<'_> {
        fn make_tupled_arg_pseudo_place(&mut self, usage: PlaceUsage) -> MdSanPlaceValue {
            self.temp_vars_state.ref_place(
                {
                    let mut place_info = MdSanPlaceInfo::from(Local::Argument(0));
                    place_info.metadata_mut().set_address(1 as RawAddress);
                    place_info
                        .metadata_mut()
                        .set_type_id(self.type_utils.type_holder);
                    place_info
                },
                usage,
            )
        }

        fn num_fields(&mut self) -> FieldIndex {
            self.type_utils.num_fields()
        }

        fn field_place(
            &mut self,
            base: &MdSanPlaceValue,
            field: FieldIndex,
            _usage: PlaceUsage,
        ) -> MdSanPlaceValue {
            let field_info = self.type_utils.field_info(field);
            let field_ty = field_info.ty;
            base.project_field(
                field_info.offset,
                || self.type_manager.get_size(&field_ty).unwrap(),
                || field_ty,
            )
        }
    }

    impl<'a> TuplingHelperImpl<'a> {
        pub(crate) fn new(
            type_manager: &'a dyn TypeDatabase,
            tuple_type: TypeId,
            temp_vars_state: MdSanVariablesState,
        ) -> Self {
            Self {
                type_manager,
                temp_vars_state,
                type_utils: TuplingHelperTypeUtils::new(
                    tuple_type,
                    Box::new(|type_id| type_manager.get_type(type_id)),
                ),
            }
        }
    }

    impl<'a> MdSanCallHandler<'a> {
        pub(super) fn make_lazy_tupling_info(
            tupling: ArgsTupling,
            arg_places: &[MdSanPlaceValue],
            type_manager: &'a dyn TypeDatabase,
            variables_state_factory: &'a dyn Fn() -> MdSanVariablesState,
        ) -> impl FnOnce() -> ArgsTuplingInfo<'a, 'a, MdSanPlaceValue, MdSanValue> {
            make_lazy_tupling_info(
                tupling,
                arg_places,
                |p| {
                    p.type_id(type_manager)
                        .expect("Expected type info for a tupled argument")
                },
                |t| {
                    Box::new(tupling::TuplingHelperImpl::new(
                        type_manager,
                        t,
                        variables_state_factory(),
                    ))
                },
                |places| core::iter::repeat_n(MdSanValue::non_rel(), places.len()).collect(),
            )
        }
    }
}

mod breakage {
    use const_format::concatcp;

    use crate::abs::{CalleeDef, FuncDef};
    use crate::call::CallFlowBreakageCallback;
    use crate::utils::alias::check_sym_value_loss;

    use super::backend;
    use backend::MdSanValue;
    use common::{log_debug, log_warn};

    const TAG: &str = concatcp!(crate::call::TAG, "::breakage");

    pub(crate) struct MdSanBreakageCallback {}

    impl MdSanBreakageCallback {
        /// # Remarks
        /// Returns an empty vector if symbolic value loss checks are disabled.
        fn inspect_external_call_info<'a>(
            &self,
            current_func: FuncDef,
            arg_values: &'a [MdSanValue],
        ) -> Vec<(usize, &'a MdSanValue)> {
            if !check_sym_value_loss!() {
                return vec![];
            }

            let relevant_args: Vec<_> = arg_values
                .iter()
                .enumerate()
                .filter(|(_, v)| v.is_rel())
                .collect();

            if !relevant_args.is_empty() {
                log_warn!(
                    target: TAG,
                    concat!(
                        "Possible loss of MD-relevant arguments in external function call, ",
                        "current internal function: {:?}",
                    ),
                    current_func,
                );
                log_debug!(
                    target: TAG,
                    "MD-relevant arguments passed to the function: {:?}",
                    relevant_args,
                );
            }
            relevant_args
        }

        fn inspect_returned_value<'a>(
            &self,
            callee: FuncDef,
            current_func: FuncDef,
            returned_value: &MdSanValue,
        ) {
            if !check_sym_value_loss!() {
                return;
            }

            if returned_value.is_rel() {
                log_warn!(
                    target: TAG,
                    concat!(
                        "Possible loss of MD-relevant returned value from {:?}, ",
                        "current internal function: {:?}",
                    ),
                    callee,
                    current_func,
                );
                log_debug!(
                    target: TAG,
                    "MD-relevant returned value from a function: {:?}",
                    returned_value,
                );
            }
        }
    }

    fn unknown_value() -> MdSanValue {
        MdSanValue::non_rel()
    }

    impl<P> CallFlowBreakageCallback<P, MdSanValue> for MdSanBreakageCallback {
        fn after_return_with_args(
            &mut self,
            _callee: Option<CalleeDef>,
            current: FuncDef,
            unconsumed_args: Vec<MdSanValue>,
        ) -> MdSanValue {
            let _ = self.inspect_external_call_info(current, &unconsumed_args);
            unknown_value()
        }

        fn at_enter(
            &mut self,
            _caller: FuncDef,
            _expected_callee: CalleeDef,
            current: FuncDef,
            unconsumed_args: Vec<MdSanValue>,
            current_arg_places: &[P],
        ) -> Vec<MdSanValue> {
            let _ = self.inspect_external_call_info(current, &unconsumed_args);
            self.at_enter_with_no_caller(current, current_arg_places)
        }

        fn at_enter_with_return_val(
            &mut self,
            callee: FuncDef,
            current: FuncDef,
            unconsumed_return_value: MdSanValue,
        ) {
            self.inspect_returned_value(callee, current, &unconsumed_return_value);
        }

        fn at_enter_with_no_caller(
            &mut self,
            _current: FuncDef,
            current_arg_places: &[P],
        ) -> Vec<MdSanValue> {
            core::iter::repeat_with(unknown_value)
                .take(current_arg_places.len())
                .collect()
        }

        fn after_return_with_return_val(
            &mut self,
            callee: FuncDef,
            current: FuncDef,
            unconsumed_return_value: MdSanValue,
        ) -> MdSanValue {
            self.inspect_returned_value(callee, current, &unconsumed_return_value);
            unknown_value()
        }

        fn at_return_with_return_val(
            &mut self,
            current: FuncDef,
            unconsumed_return_value: MdSanValue,
        ) {
            self.inspect_returned_value(current, current, &unconsumed_return_value);
        }
    }
}

impl CallShadowMemory<MdSanPlaceValue> for MdSanVariablesState {
    type Value = MdSanValue;

    delegate! {
        #[through(MdMemoryState)]
        to self {
            fn take_place(&mut self, place: &MdSanPlaceValue) -> Self::Value;
            fn set_place(&mut self, place: &MdSanPlaceValue, value: Self::Value);
        }
    }
}
