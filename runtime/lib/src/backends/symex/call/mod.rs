use std::cell::RefMut;

use crate::{
    abs::{
        AssignmentId, BasicBlockIndex, CalleeDef, Constant, FuncDef, Local,
        backend::PhasedCallTraceRecorder, utils::BasicBlockLocationExt,
    },
    call::{
        CallControlFlowManager, CallDataFlowManager, CallFlowManager, CallShadowMemory,
        DefaultCallFlowManager, SignaturePlaces, tupling::ArgsTuplingInfo,
    },
    pri::fluent::backend::{ArgsTupling, CallHandler, DropHandler},
    utils::InPlaceSelfHierarchical,
};

use crate::backends::symex as backend;
use backend::{
    GenericVariablesState, Implied, PlaceValueRef, SymExBackend, SymExValue, SymExVariablesState,
    TypeDatabase, Value, config::CallConfig, expr::prelude::DeterPlaceValueRef,
};

pub(super) type SymExCallFlowManager =
    DefaultCallFlowManager<DeterPlaceValueRef, SymExValue, breakage::SymExBreakageCallback>;

pub(crate) fn default_flow_manager(config: CallConfig) -> SymExCallFlowManager
where
    SymExCallFlowManager: CallControlFlowManager
        + CallDataFlowManager<Place = DeterPlaceValueRef, Value = SymExValue>,
{
    DefaultCallFlowManager::new(breakage::SymExBreakageCallback {
        strategy: config.external_call,
    })
}

pub(crate) struct SymExCallHandler<'a> {
    flow_manager: &'a mut SymExCallFlowManager,
    variables_state: &'a mut SymExVariablesState,
    variables_state_factory: &'a dyn Fn() -> SymExVariablesState,
    type_manager: &'a dyn TypeDatabase,
    #[cfg(feature = "implicit_flow")]
    implication_investigator: &'a dyn super::ImplicationInvestigator,
    trace_recorder: RefMut<'a, dyn PhasedCallTraceRecorder>,
}

impl<'a> SymExCallHandler<'a> {
    pub(super) fn new(backend: &'a mut SymExBackend) -> Self {
        Self {
            flow_manager: &mut backend.call_flow_manager,
            variables_state: &mut backend.vars_state,
            variables_state_factory: &backend.vars_state_factory,
            type_manager: backend.type_manager.as_ref(),
            #[cfg(feature = "implicit_flow")]
            implication_investigator: backend.implication_investigator.as_ref(),
            trace_recorder: backend.trace_recorder.borrow_mut(),
        }
    }

    fn current_func(&self) -> FuncDef {
        self.flow_manager.current_func()
    }
}

impl<'a> CallHandler for SymExCallHandler<'a> {
    type Place = PlaceValueRef;
    type Operand = SymExValue;
    type MetadataHandler = ();

    fn before_call(mut self, def: CalleeDef, call_site: BasicBlockIndex) {
        let call_site = self.current_func().at_basic_block(call_site);
        self.trace_recorder.start_call(call_site);
        self.flow_manager.prepare_for_calling(def);
    }

    fn before_call_some(mut self) {
        let call_site = self.current_func().at_basic_block(Default::default());
        self.trace_recorder.start_call(call_site);
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
            args.into_iter().collect(),
            are_args_tupled,
        );
    }

    fn enter(mut self, def: FuncDef) {
        let sanity = self.flow_manager.enter(def);
        self.trace_recorder.finish_call(def, sanity.is_broken());
    }

    fn emplace_arguments(
        self,
        arg_places: Vec<Self::Place>,
        ret_val_place: Self::Place,
        tupling: ArgsTupling,
    ) {
        let arg_types = Self::collect_arg_types_if_tupled(tupling, &arg_places);

        fn ensure_deter_place(place: PlaceValueRef) -> DeterPlaceValueRef {
            debug_assert!(!place.is_symbolic());
            DeterPlaceValueRef::new(place)
        }

        let tupling_info = Self::make_lazy_tupling_info(
            tupling,
            arg_types,
            self.type_manager,
            self.variables_state_factory,
        );
        self.variables_state.add_layer();

        self.flow_manager.emplace_args(
            SignaturePlaces {
                args: arg_places.into_iter().map(ensure_deter_place).collect(),
                return_val: ensure_deter_place(ret_val_place),
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
    fn ret(mut self, ret_point: BasicBlockIndex) {
        self.trace_recorder
            .start_return(self.flow_manager.current_func().at_basic_block(ret_point));
        let token = self.flow_manager.start_return();
        self.flow_manager
            .grab_return_value(token, self.variables_state);
        self.variables_state.drop_layer();
    }

    #[cfg_attr(not(feature = "implicit_flow"), allow(unused))]
    fn after_call(mut self, assignment_id: AssignmentId, result_dest: Self::Place) {
        debug_assert!(!result_dest.is_symbolic());

        let token = self.flow_manager.finalize_call();
        let caller = self
            .trace_recorder
            .finish_return(token.sanity().is_broken().unwrap());
        debug_assert_eq!(caller, self.current_func());

        let mut return_val = self.flow_manager.give_return_value(token);

        #[cfg(feature = "implicit_flow")]
        super::assignment::precondition::add_antecedent(
            self.implication_investigator,
            || result_dest.type_info().get_size(self.type_manager).unwrap(),
            (caller.body_id, assignment_id),
            &mut return_val,
        );

        CallShadowMemory::set_place(
            self.variables_state,
            &DeterPlaceValueRef::new(result_dest),
            return_val,
        );
    }

    fn metadata(self) -> Self::MetadataHandler {
        Default::default()
    }
}

// Currently, we have no special mechanism for dropping beyond calling the (possible) glue
impl DropHandler for SymExCallHandler<'_> {
    type Place = PlaceValueRef;
    type Operand = SymExValue;

    fn before_drop(self, def: CalleeDef, call_site: BasicBlockIndex) {
        <Self as CallHandler>::before_call(self, def, call_site);
    }

    fn before_drop_some(self) {
        <Self as CallHandler>::before_call_some(self);
    }

    fn take_data_before_drop(self, func: Self::Operand, arg: Self::Operand, _place: Self::Place) {
        <Self as CallHandler>::take_data_before_call(self, func, vec![arg], false);
    }

    fn after_drop(mut self) {
        let token = self.flow_manager.finalize_call();
        let caller = self
            .trace_recorder
            .finish_return(token.sanity().is_broken().unwrap());
        debug_assert_eq!(caller, self.current_func());

        let _ = self.flow_manager.give_return_value(token);
    }
}

mod tupling {
    use delegate::delegate;

    use common::type_info::{FieldsShapeInfo, StructShape, TypeInfo};

    use crate::{
        abs::{FieldIndex, RawAddress},
        backends::symex::expr::LazyTypeInfo,
        call::tupling::TuplingHelper,
        type_info::{FieldsShapeInfoExt, TypeInfoExt},
    };

    use super::*;
    use backend::{TypeDatabase, expr::prelude::DeterministicPlaceValue};

    pub(crate) struct TuplingHelperImpl<'a> {
        pub(crate) type_manager: &'a dyn TypeDatabase,
        pub(crate) tuple_type: LazyTypeInfo,
        pub(crate) fields_info: Option<StructShape>,
        pub(crate) temp_vars_state: SymExVariablesState,
    }

    impl<'a> CallShadowMemory<DeterPlaceValueRef> for TuplingHelperImpl<'a> {
        type Value = SymExValue;

        delegate! {
            #[through(CallShadowMemory::<DeterPlaceValueRef>)]
            to &mut self.temp_vars_state {
                fn take_place(&mut self, place: &DeterPlaceValueRef) -> Self::Value;
                fn set_place(&mut self, place: &DeterPlaceValueRef, value: Self::Value);
            }
        }
    }

    impl TuplingHelper<DeterPlaceValueRef, SymExValue> for TuplingHelperImpl<'_> {
        fn make_tupled_arg_pseudo_place(&mut self) -> DeterPlaceValueRef {
            DeterPlaceValueRef::new(
                DeterministicPlaceValue::from_addr_type_info(
                    RawAddress::default(),
                    self.tuple_type.clone(),
                )
                .to_value_ref(),
            )
        }

        fn num_fields(&mut self) -> FieldIndex {
            self.type_info()
                .expect_single_variant()
                .fields
                .as_struct()
                .unwrap()
                .fields()
                .len() as FieldIndex
        }

        fn field_place(
            &mut self,
            base: &DeterPlaceValueRef,
            field: FieldIndex,
        ) -> DeterPlaceValueRef {
            let field_info = &self.fields_info().fields()[field as usize];
            DeterPlaceValueRef::new(
                DeterministicPlaceValue::from_addr_type(
                    base.address().wrapping_byte_add(field_info.offset as usize),
                    field_info.ty,
                )
                .to_value_ref(),
            )
        }
    }

    impl<'a> TuplingHelperImpl<'a> {
        pub(crate) fn new(
            type_manager: &'a dyn TypeDatabase,
            tuple_type: LazyTypeInfo,
            temp_vars_state: SymExVariablesState,
        ) -> Self {
            Self {
                type_manager,
                tuple_type,
                fields_info: None,
                temp_vars_state,
            }
        }

        pub(crate) fn type_info(&mut self) -> &TypeInfo {
            self.tuple_type.fetch(self.type_manager)
        }

        pub(crate) fn fields_info(&mut self) -> &StructShape {
            if self.fields_info.is_none() {
                let type_info = self.type_info();
                let info = match type_info.expect_single_variant().fields {
                    FieldsShapeInfo::Struct(ref shape) => shape.clone(),
                    _ => panic!("Expected tuple type info, got: {:?}", type_info),
                };
                self.fields_info = Some(info);
            }
            self.fields_info.as_ref().unwrap()
        }
    }

    impl<'a> SymExCallHandler<'a> {
        pub(super) fn collect_arg_types_if_tupled(
            tupling: ArgsTupling,
            arg_places: &[<Self as CallHandler>::Place],
        ) -> Option<Vec<LazyTypeInfo>> {
            matches!(tupling, ArgsTupling::Tupled).then(|| {
                arg_places
                    .iter()
                    .map(|place| place.type_info().clone())
                    .collect::<Vec<_>>()
            })
        }

        pub(super) fn make_lazy_tupling_info(
            tupling: ArgsTupling,
            arg_types: Option<Vec<backend::expr::LazyTypeInfo>>,
            type_manager: &'a dyn TypeDatabase,
            variables_state_factory: &'a dyn Fn() -> SymExVariablesState,
        ) -> impl FnOnce() -> ArgsTuplingInfo<'a, 'a, DeterPlaceValueRef, SymExValue> {
            move || match tupling {
                ArgsTupling::Untupled {
                    tupled_arg_index,
                    tuple_type,
                } => {
                    core::hint::cold_path();
                    let Local::Argument(tupled_arg_index) = tupled_arg_index else {
                        unreachable!()
                    };
                    ArgsTuplingInfo::Untupled {
                        tupled_arg_index,
                        tupling_helper: Box::new(move || {
                            Box::new(tupling::TuplingHelperImpl::new(
                                type_manager,
                                tuple_type.into(),
                                variables_state_factory(),
                            ))
                        }),
                    }
                }
                ArgsTupling::Tupled => {
                    core::hint::cold_path();
                    let (first_arg_type, mut rest_args_types) = {
                        let mut arg_types = arg_types.unwrap();
                        let rest_args_types = arg_types.split_off(1);
                        let first_arg_type = arg_types.remove(0);
                        (first_arg_type, rest_args_types)
                    };
                    ArgsTuplingInfo::Tupled {
                        head_args: Box::new(move || {
                            vec![{
                                debug_assert_eq!(
                                    first_arg_type.get_size(type_manager),
                                    Some(0),
                                    "Expected to happen only in FnOnce implementation of a non-capturing closure",
                                );
                                Implied::always(Value::from(Constant::Zst).to_value_ref())
                            }]
                        }),
                        tupling_helper: Box::new(move || {
                            Box::new(tupling::TuplingHelperImpl::new(
                                type_manager,
                                rest_args_types.remove(0),
                                variables_state_factory(),
                            ))
                        }),
                    }
                }
                ArgsTupling::Normal => ArgsTuplingInfo::Normal,
            }
        }
    }
}

mod breakage {
    use const_format::concatcp;

    use crate::abs::{CalleeDef, Constant, FuncDef};
    use crate::call::CallFlowBreakageCallback;
    use crate::utils::alias::check_sym_value_loss;

    use super::backend;
    use backend::{ConcreteValue, Implied, SymExValue, config::ExternalCallStrategy};
    use common::{log_debug, log_warn};

    const TAG: &str = concatcp!(crate::call::TAG, "::breakage");

    pub(crate) struct SymExBreakageCallback {
        pub(super) strategy: ExternalCallStrategy,
    }

    impl SymExBreakageCallback {
        /// # Remarks
        /// Returns an empty vector if symbolic value loss checks are disabled.
        fn inspect_external_call_info<'a>(
            &self,
            current_func: FuncDef,
            arg_values: &'a [SymExValue],
        ) -> Vec<(usize, &'a SymExValue)> {
            if !check_sym_value_loss!() {
                return vec![];
            }

            let symbolic_args: Vec<_> = arg_values
                .iter()
                .enumerate()
                .filter(|(_, v)| v.is_symbolic())
                .collect();
            if !symbolic_args.is_empty() {
                log_warn!(
                    target: TAG,
                    concat!(
                        "Possible loss of symbolic arguments in external function call, ",
                        "current internal function: {:?}",
                    ),
                    current_func,
                );
                log_debug!(
                    target: TAG,
                    "Symbolic arguments passed to the function: {:?}",
                    symbolic_args,
                );
            }
            symbolic_args
        }

        fn inspect_returned_value<'a>(
            &self,
            callee: FuncDef,
            current_func: FuncDef,
            returned_value: &SymExValue,
        ) {
            if !check_sym_value_loss!() {
                return;
            }

            if returned_value.is_symbolic() {
                log_warn!(
                    target: TAG,
                    concat!(
                        "Possible loss of symbolic returned value from {:?}, ",
                        "current internal function: {:?}",
                    ),
                    callee,
                    current_func,
                );
                log_debug!(
                    target: TAG,
                    "Symbolic returned value from a function: {:?}",
                    returned_value,
                );
            }
        }
    }

    fn unknown_value() -> SymExValue {
        Implied::by_unknown(ConcreteValue::from(Constant::Some).to_value_ref())
    }

    impl<P> CallFlowBreakageCallback<P, SymExValue> for SymExBreakageCallback {
        fn after_return_with_args(
            &mut self,
            _callee: Option<CalleeDef>,
            current: FuncDef,
            unconsumed_args: Vec<SymExValue>,
        ) -> SymExValue {
            let symbolic_args = self.inspect_external_call_info(current, &unconsumed_args);

            enum Action {
                Concretize,
                OverApproximate,
            }
            use Action::*;

            let action = match self.strategy {
                ExternalCallStrategy::Panic => panic!("External function call detected."),
                ExternalCallStrategy::Concretization => Concretize,
                ExternalCallStrategy::OverApproximation => OverApproximate,
                ExternalCallStrategy::OptimisticConcretization => {
                    /* NOTE: What is optimistic here?
                     * It correspond to the optimistic assumption that the callee has been a
                     * pure function and no symbolic input results in no symbolic output. */
                    /* FIXME: With the current implementation, references to symbolic values
                     * skip this check. */
                    if !symbolic_args.is_empty() {
                        Concretize
                    } else {
                        OverApproximate
                    }
                }
            };
            match action {
                Concretize => unknown_value(),
                OverApproximate => {
                    todo!("#306: Over-approximated symbolic values are not supported.")
                }
            }
        }

        fn at_enter(
            &mut self,
            _caller: FuncDef,
            _expected_callee: CalleeDef,
            current: FuncDef,
            unconsumed_args: Vec<SymExValue>,
            current_arg_places: &[P],
        ) -> Vec<SymExValue> {
            self.inspect_external_call_info(current, &unconsumed_args);
            self.at_enter_with_no_caller(current, current_arg_places)
        }

        fn at_enter_with_return_val(
            &mut self,
            callee: FuncDef,
            current: FuncDef,
            unconsumed_return_value: SymExValue,
        ) {
            self.inspect_returned_value(callee, current, &unconsumed_return_value);
        }

        fn at_enter_with_no_caller(
            &mut self,
            _current: FuncDef,
            current_arg_places: &[P],
        ) -> Vec<SymExValue> {
            core::iter::repeat_n(unknown_value(), current_arg_places.len()).collect()
        }

        fn after_return_with_return_val(
            &mut self,
            callee: FuncDef,
            current: FuncDef,
            unconsumed_return_value: SymExValue,
        ) -> SymExValue {
            self.inspect_returned_value(callee, current, &unconsumed_return_value);
            unknown_value()
        }

        fn at_return_with_return_val(
            &mut self,
            current: FuncDef,
            unconsumed_return_value: SymExValue,
        ) {
            self.inspect_returned_value(current, current, &unconsumed_return_value);
        }
    }
}

impl<P> CallShadowMemory<P> for SymExVariablesState
where
    P: AsRef<<SymExVariablesState as GenericVariablesState>::PlaceValue>,
{
    type Value = SymExValue;

    fn take_place(&mut self, place: &P) -> Self::Value {
        GenericVariablesState::take_place(self, place.as_ref())
    }

    fn set_place(&mut self, place: &P, value: Self::Value) {
        GenericVariablesState::set_place(self, place.as_ref(), value)
    }
}
