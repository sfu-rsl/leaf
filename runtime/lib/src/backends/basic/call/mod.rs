use std::cell::RefMut;

use crate::{
    abs::{
        AssignmentId, BasicBlockIndex, CalleeDef, Constant, FuncDef, Local,
        backend::{ArgsTupling, CallHandler, PhasedCallTraceRecorder},
        utils::BasicBlockLocationExt,
    },
    call::{
        CallFlowManager, CallShadowMemory, DefaultCallFlowManager, SignaturePlaces,
        tupling::ArgsTuplingInfo,
    },
    utils::InPlaceSelfHierarchical,
};

use crate::backends::basic as backend;
use backend::{
    BasicBackend, BasicValue, BasicVariablesState, GenericVariablesState, Implied, PlaceValueRef,
    TypeDatabase, Value, config::CallConfig, expr::prelude::DeterPlaceValueRef,
};

pub(super) type BasicCallFlowManager =
    DefaultCallFlowManager<DeterPlaceValueRef, BasicValue, breakage::BasicBreakageCallback>;

pub(crate) fn default_flow_manager(config: CallConfig) -> BasicCallFlowManager
where
    BasicCallFlowManager: CallFlowManager<Place = DeterPlaceValueRef, Value = BasicValue>,
{
    DefaultCallFlowManager::new(breakage::BasicBreakageCallback {
        strategy: config.external_call,
    })
}

pub(crate) struct BasicCallHandler<'a> {
    flow_manager: &'a mut BasicCallFlowManager,
    variables_state: &'a mut BasicVariablesState,
    variables_state_factory: &'a dyn Fn() -> BasicVariablesState,
    type_manager: &'a dyn TypeDatabase,
    #[cfg(feature = "implicit_flow")]
    implication_investigator: &'a dyn super::ImplicationInvestigator,
    trace_recorder: RefMut<'a, dyn PhasedCallTraceRecorder>,
}

impl<'a> BasicCallHandler<'a> {
    pub(super) fn new(backend: &'a mut BasicBackend) -> Self {
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

impl<'a> CallHandler for BasicCallHandler<'a> {
    type Place = PlaceValueRef;
    type Operand = BasicValue;
    type MetadataHandler = ();

    #[inline]
    fn before_call(
        mut self,
        def: CalleeDef,
        call_site: BasicBlockIndex,
        func: Self::Operand,
        args: impl IntoIterator<Item = Self::Arg>,
        are_args_tupled: bool,
    ) {
        let call_site = self.current_func().at_basic_block(call_site);
        self.trace_recorder.start_call(call_site);
        self.flow_manager
            .prepare_for_call(def, func, args.into_iter().collect(), are_args_tupled);
    }

    fn enter(
        mut self,
        def: FuncDef,
        arg_places: Vec<Self::Place>,
        ret_val_place: Self::Place,
        tupling: ArgsTupling,
    ) {
        let arg_types = Self::collect_arg_types_if_tupled(tupling, &arg_places);

        fn ensure_deter_place(place: PlaceValueRef) -> DeterPlaceValueRef {
            debug_assert!(!place.is_symbolic());
            DeterPlaceValueRef::new(place)
        }

        let token = self.flow_manager.start_enter(
            def,
            SignaturePlaces {
                arg_places: arg_places.into_iter().map(ensure_deter_place).collect(),
                return_val_place: ensure_deter_place(ret_val_place),
            },
        );

        let tupling_info = Self::make_lazy_tupling_info(
            tupling,
            arg_types,
            self.type_manager,
            self.variables_state_factory,
        );
        self.variables_state.add_layer();
        let sanity = self
            .flow_manager
            .finalize_enter(token, tupling_info, self.variables_state);

        self.trace_recorder
            .finish_call(def, matches!(sanity, crate::call::CallFlowSanity::Broken));
    }

    #[inline]
    fn override_return_value(self, value: Self::Operand) {
        self.flow_manager.override_return_value(value)
    }

    #[inline]
    fn ret(mut self, ret_point: BasicBlockIndex) {
        self.trace_recorder
            .start_return(self.flow_manager.current_func().at_basic_block(ret_point));
        self.flow_manager.start_return(self.variables_state);
        self.variables_state.drop_layer();
    }

    #[cfg_attr(not(feature = "implicit_flow"), allow(unused))]
    fn after_call(mut self, assignment_id: AssignmentId, result_dest: Self::Place) {
        debug_assert!(!result_dest.is_symbolic());
        let (mut return_val, sanity) = self.flow_manager.finalize_call();
        let call_site = self
            .trace_recorder
            .finish_return(matches!(sanity, crate::call::CallFlowSanity::Broken));
        debug_assert_eq!(call_site.body, self.current_func());

        #[cfg(feature = "implicit_flow")]
        super::assignment::precondition::add_antecedent(
            self.implication_investigator,
            || result_dest.type_info().get_size(self.type_manager).unwrap(),
            (call_site.body.body_id, assignment_id),
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

mod tupling {
    use delegate::delegate;

    use common::type_info::{FieldsShapeInfo, StructShape, TypeInfo};

    use crate::{
        abs::{FieldIndex, RawAddress},
        backends::basic::expr::LazyTypeInfo,
        call::tupling::TuplingHelper,
        type_info::{FieldsShapeInfoExt, TypeInfoExt},
    };

    use super::*;
    use backend::{TypeDatabase, expr::prelude::DeterministicPlaceValue};

    pub(crate) struct TuplingHelperImpl<'a> {
        pub(crate) type_manager: &'a dyn TypeDatabase,
        pub(crate) tuple_type: LazyTypeInfo,
        pub(crate) fields_info: Option<StructShape>,
        pub(crate) temp_vars_state: BasicVariablesState,
    }

    impl<'a> CallShadowMemory<DeterPlaceValueRef> for TuplingHelperImpl<'a> {
        type Value = BasicValue;

        delegate! {
            #[through(CallShadowMemory::<DeterPlaceValueRef>)]
            to &mut self.temp_vars_state {
                fn take_place(&mut self, place: &DeterPlaceValueRef) -> Self::Value;
                fn set_place(&mut self, place: &DeterPlaceValueRef, value: Self::Value);
            }
        }
    }

    impl TuplingHelper<DeterPlaceValueRef, BasicValue> for TuplingHelperImpl<'_> {
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
            temp_vars_state: BasicVariablesState,
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

    impl<'a> BasicCallHandler<'a> {
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
            variables_state_factory: &'a dyn Fn() -> BasicVariablesState,
        ) -> impl FnOnce() -> ArgsTuplingInfo<'a, 'a, DeterPlaceValueRef, BasicValue> {
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
    use backend::{BasicValue, ConcreteValue, Implied, config::ExternalCallStrategy};
    use common::{log_debug, log_warn};

    const TAG: &str = concatcp!(crate::call::TAG, "::breakage");

    pub(crate) struct BasicBreakageCallback {
        pub(super) strategy: ExternalCallStrategy,
    }

    impl BasicBreakageCallback {
        /// # Remarks
        /// Returns an empty vector if symbolic value loss checks are disabled.
        fn inspect_external_call_info<'a>(
            &self,
            current_func: FuncDef,
            arg_values: &'a [BasicValue],
        ) -> Vec<(usize, &'a BasicValue)> {
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

        fn inspect_returned_value<'a>(&self, current_func: FuncDef, returned_value: &BasicValue) {
            if !check_sym_value_loss!() {
                return;
            }

            if returned_value.is_symbolic() {
                log_warn!(
                    target: TAG,
                    concat!(
                        "Possible loss of symbolic return value in external function call",
                        "current internal function: {:?}",
                    ),
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

    fn unknown_value() -> BasicValue {
        Implied::by_unknown(ConcreteValue::from(Constant::Some).to_value_ref())
    }

    impl<P> CallFlowBreakageCallback<P, BasicValue> for BasicBreakageCallback {
        fn after_return_with_args(
            &mut self,
            _callee: CalleeDef,
            current: FuncDef,
            unconsumed_args: Vec<BasicValue>,
        ) -> BasicValue {
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
            unconsumed_args: Vec<BasicValue>,
            current_arg_places: &[P],
        ) -> Vec<BasicValue> {
            self.inspect_external_call_info(current, &unconsumed_args);
            self.at_enter_with_no_caller(current, current_arg_places)
        }

        fn at_enter_with_no_caller(
            &mut self,
            _current: FuncDef,
            current_arg_places: &[P],
        ) -> Vec<BasicValue> {
            core::iter::repeat_n(unknown_value(), current_arg_places.len()).collect()
        }

        fn before_return_with_return_val(
            &mut self,
            _callee: FuncDef,
            current: FuncDef,
            unconsumed_return_value: BasicValue,
        ) {
            self.inspect_returned_value(current, &unconsumed_return_value);
        }

        fn after_return_with_return_val(
            &mut self,
            _callee: FuncDef,
            current: FuncDef,
            unconsumed_return_value: BasicValue,
        ) -> BasicValue {
            self.inspect_returned_value(current, &unconsumed_return_value);
            unknown_value()
        }
    }
}

impl<P> CallShadowMemory<P> for BasicVariablesState
where
    P: AsRef<<BasicVariablesState as GenericVariablesState>::PlaceValue>,
{
    type Value = BasicValue;

    fn take_place(&mut self, place: &P) -> Self::Value {
        GenericVariablesState::take_place(self, place.as_ref())
    }

    fn set_place(&mut self, place: &P, value: Self::Value) {
        GenericVariablesState::set_place(self, place.as_ref(), value)
    }
}
