mod stack;
use std::{borrow::Cow, cell::RefMut};

use derive_more as dm;

use crate::abs::{
    AssignmentId, BasicBlockIndex, CalleeDef, Constant, FuncDef, Local, LocalIndex, TypeId,
    backend::{ArgsTupling, CallHandler, PhasedCallTraceRecorder},
    utils::BasicBlockLocationExt,
};

use crate::backends::basic as backend;
use backend::{
    BasicBackend, BasicValue, BasicVariablesState, CallStackInfo, GenericVariablesState, Implied,
    PlaceValueRef, TypeDatabase, Value, expr::prelude::DeterPlaceValueRef,
};

pub(super) use self::stack::BasicCallStackManager;
use self::stack::EntranceToken;

enum CallFlowSanity {
    Expected,
    /// The stack is broken (e.g., external function in between)
    Broken,
}

#[derive(dm::Debug)]
enum ArgsTuplingInfo<'h, 'a> {
    Normal,
    Untupled {
        #[debug(ignore)]
        tupled_arg_index: LocalIndex,
        #[debug(ignore)]
        tupling_helper: LazyTuplingHelper<'h, 'a>,
    },
    Tupled {
        #[debug(ignore)]
        head_args: Box<dyn FnOnce() -> Vec<BasicValue> + 'a>,
        #[debug(ignore)]
        tupling_helper: LazyTuplingHelper<'h, 'a>,
    },
}

type LazyTuplingHelper<'h, 'a> =
    Box<dyn FnOnce() -> Box<dyn tupling::BasicUntupleHelper + 'h> + 'a>;

trait GenericCallStackManager: CallStackInfo {
    type Place = <Self::VariablesState as GenericVariablesState>::PlaceValue;
    type Value = <Self::VariablesState as GenericVariablesState>::Value;

    /* NOTE: Why `are_args_tupled` are passed? Isn't `try_untuple_argument` enough?
     * First, arguments are tupled at the call site, which also calls this function.
     * Second, when untupling, we should make sure that the arguments were tupled.
     * If closures are converted to a function pointer, then the arguments are not tupled.
     */
    fn prepare_for_call(
        &mut self,
        def: CalleeDef,
        func: Self::Value,
        args: Vec<Self::Value>,
        are_args_tupled: bool,
    );

    fn set_places(&mut self, arg_places: Vec<Self::Place>, ret_val_place: Self::Place);

    fn start_enter(&mut self, current_func: FuncDef) -> EntranceToken<Self::Value>;

    fn finalize_enter<'a, 'h>(
        &'a mut self,
        token: EntranceToken<Self::Value>,
        tupling: ArgsTuplingInfo,
    ) -> CallFlowSanity;

    fn pop_stack_frame(&mut self);

    fn override_return_value(&mut self, value: Self::Value);

    fn finalize_call(&mut self) -> (Self::Value, CallFlowSanity);
}

trait CallStackManager:
    GenericCallStackManager<
        VariablesState = BasicVariablesState,
        Place = DeterPlaceValueRef,
        Value = <BasicVariablesState as GenericVariablesState>::Value,
    >
{
}
impl<T> CallStackManager for T where
    T: GenericCallStackManager<
            VariablesState = BasicVariablesState,
            Place = DeterPlaceValueRef,
            Value = <BasicVariablesState as GenericVariablesState>::Value,
        >
{
}

pub(crate) struct BasicCallHandler<'a> {
    call_stack_manager: &'a mut dyn CallStackManager,
    type_manager: &'a dyn TypeDatabase,
    implication_investigator: &'a dyn ImplicationInvestigator,
    trace_recorder: RefMut<'a, dyn PhasedCallTraceRecorder>,
}

impl<'a> BasicCallHandler<'a> {
    pub(super) fn new(backend: &'a mut BasicBackend) -> Self {
        Self {
            call_stack_manager: &mut backend.call_stack_manager,
            type_manager: backend.type_manager.as_ref(),
            implication_investigator: backend.implication_investigator.as_ref(),
            trace_recorder: backend.trace_recorder.borrow_mut(),
        }
    }

    fn current_func(&self) -> FuncDef {
        self.call_stack_manager.current_func()
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
        self.call_stack_manager.prepare_for_call(
            def,
            func,
            args.into_iter().collect(),
            are_args_tupled,
        );
    }

    fn enter(
        mut self,
        def: FuncDef,
        arg_places: Vec<Self::Place>,
        ret_val_place: Self::Place,
        tupling: ArgsTupling,
    ) {
        let arg_types = matches!(tupling, ArgsTupling::Tupled).then(|| {
            arg_places
                .iter()
                .map(|place| place.type_info().clone())
                .collect::<Vec<_>>()
        });

        fn ensure_deter_place(place: PlaceValueRef) -> DeterPlaceValueRef {
            debug_assert!(!place.is_symbolic());
            DeterPlaceValueRef::new(place)
        }
        self.call_stack_manager.set_places(
            arg_places.into_iter().map(ensure_deter_place).collect(),
            ensure_deter_place(ret_val_place),
        );
        let token = self.call_stack_manager.start_enter(def);

        let tupling_info = match tupling {
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
                            self.type_manager,
                            tuple_type.into(),
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
                                first_arg_type.get_size(self.type_manager),
                                Some(0),
                                "Expected to happen only in FnOnce implementation of a non-capturing closure",
                            );
                            Implied::always(Value::from(Constant::Zst).to_value_ref())
                        }]
                    }),
                    tupling_helper: Box::new(move || {
                        Box::new(tupling::TuplingHelperImpl::new(
                            self.type_manager,
                            rest_args_types.remove(0),
                        ))
                    }),
                }
            }
            ArgsTupling::Normal => ArgsTuplingInfo::Normal,
        };
        let sanity = self.call_stack_manager.finalize_enter(token, tupling_info);

        self.trace_recorder
            .finish_call(def, matches!(sanity, CallFlowSanity::Broken));
    }

    #[inline]
    fn override_return_value(self, value: Self::Operand) {
        self.call_stack_manager.override_return_value(value)
    }

    #[inline]
    fn ret(mut self, ret_point: BasicBlockIndex) {
        self.trace_recorder.start_return(
            self.call_stack_manager
                .current_func()
                .at_basic_block(ret_point),
        );
        self.call_stack_manager.pop_stack_frame();
    }

    fn after_call(mut self, assignment_id: AssignmentId, result_dest: Self::Place) {
        debug_assert!(!result_dest.is_symbolic());
        let (mut return_val, sanity) = self.call_stack_manager.finalize_call();
        let call_site = self
            .trace_recorder
            .finish_return(matches!(sanity, CallFlowSanity::Broken));
        debug_assert_eq!(call_site.body, self.current_func());

        let antecedent = self
            .implication_investigator
            .antecedent_of_latest_assignment((call_site.body.body_id, assignment_id));
        if let Some(antecedent) = antecedent {
            return_val.by.add_antecedents(Cow::Owned(antecedent), || {
                result_dest.type_info().get_size(self.type_manager).unwrap()
            });
        }
        self.call_stack_manager
            .top()
            .set_place(DeterPlaceValueRef::new(result_dest).as_ref(), return_val);
    }

    fn metadata(self) -> Self::MetadataHandler {
        Default::default()
    }
}

mod tupling {
    use common::type_info::{FieldsShapeInfo, StructShape, TypeInfo};

    use crate::{
        abs::{FieldIndex, RawAddress, TypeId},
        backends::basic::expr::LazyTypeInfo,
        type_info::{FieldsShapeInfoExt, TypeInfoExt},
    };

    use super::{DeterPlaceValueRef, backend};
    use backend::{BasicPlaceInfo, TypeDatabase, expr::prelude::DeterministicPlaceValue};

    pub(super) trait TuplingHelper {
        type PlaceInfo;
        type Place;

        fn make_tupled_arg_pseudo_place(&mut self, addr: RawAddress) -> Self::Place;

        fn num_fields(&mut self) -> FieldIndex;

        /// Takes a place and returns a place with projection to the field.
        /// It should make a valid place with full information needed for the state.
        fn field_place(&mut self, base: Self::Place, field: FieldIndex) -> Self::Place;
    }

    pub(super) trait BasicUntupleHelper:
        TuplingHelper<PlaceInfo = BasicPlaceInfo, Place = DeterPlaceValueRef>
    {
    }
    impl<T> BasicUntupleHelper for T where
        T: TuplingHelper<PlaceInfo = BasicPlaceInfo, Place = DeterPlaceValueRef>
    {
    }

    pub(crate) struct TuplingHelperImpl<'a> {
        pub(crate) type_manager: &'a dyn TypeDatabase,
        pub(crate) tuple_type: LazyTypeInfo,
        pub(crate) fields_info: Option<StructShape>,
    }

    impl TuplingHelper for TuplingHelperImpl<'_> {
        type PlaceInfo = BasicPlaceInfo;
        type Place = DeterPlaceValueRef;

        fn make_tupled_arg_pseudo_place(&mut self, addr: RawAddress) -> Self::Place {
            DeterPlaceValueRef::new(
                DeterministicPlaceValue::from_addr_type_info(addr, self.tuple_type.clone())
                    .to_value_ref(),
            )
        }

        fn num_fields(&mut self) -> FieldIndex {
            self.type_info()
                .expect_single_variant()
                .fields
                .as_struct()
                .unwrap()
                .fields
                .len() as FieldIndex
        }

        fn field_place(&mut self, base: Self::Place, field: FieldIndex) -> Self::Place {
            let field_info = &self.fields_info().fields[field as usize];
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
        pub(crate) fn new(type_manager: &'a dyn TypeDatabase, tuple_type: LazyTypeInfo) -> Self {
            Self {
                type_manager,
                tuple_type,
                fields_info: None,
            }
        }

        #[inline]
        pub(crate) fn get_type(&self, type_id: TypeId) -> &'static TypeInfo {
            self.type_manager.get_type(&type_id)
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
}
use tupling::BasicUntupleHelper;

use super::ImplicationInvestigator;
