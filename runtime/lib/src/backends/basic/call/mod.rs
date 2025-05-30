mod stack;
use std::cell::RefMut;

pub(super) use stack::BasicCallStackManager;

use crate::abs::{
    AssignmentId, BasicBlockIndex, CalleeDef, FuncDef, Local, LocalIndex, TypeId,
    backend::{CallHandler, PhasedCallTraceRecorder},
    utils::BasicBlockLocationExt,
};

use crate::backends::basic as backend;
use backend::{
    BasicBackend, BasicVariablesState, CallStackInfo, GenericVariablesState, Implied,
    PlaceValueRef, TypeDatabase, ValueRef, expr::prelude::DeterPlaceValueRef,
};

enum CallFlowSanity {
    Expected,
    /// The stack is broken (e.g., external function in between)
    Broken,
}

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

    fn try_untuple_argument<'a, 'b>(
        &'a mut self,
        arg_index: LocalIndex,
        untuple_helper: &dyn Fn() -> Box<dyn untuple::BasicUntupleHelper + 'b>,
    );

    fn notify_enter(&mut self, current_func: FuncDef) -> CallFlowSanity;

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
    type Operand = Implied<ValueRef>;
    type MetadataHandler = ();

    #[inline]
    fn before_call(
        mut self,
        def: CalleeDef,
        call_site: BasicBlockIndex,
        func: Self::Operand,
        args: impl Iterator<Item = Self::Arg>,
        are_args_tupled: bool,
    ) {
        let call_site = self.current_func().at_basic_block(call_site);
        self.trace_recorder.start_call(call_site);
        self.call_stack_manager
            .prepare_for_call(def, func, args.collect(), are_args_tupled);
    }

    fn enter(
        mut self,
        def: FuncDef,
        arg_places: impl Iterator<Item = Self::Place>,
        ret_val_place: Self::Place,
        tupled_arg: Option<(Local, TypeId)>,
    ) {
        fn ensure_deter_place(place: PlaceValueRef) -> DeterPlaceValueRef {
            debug_assert!(!place.is_symbolic());
            DeterPlaceValueRef::new(place)
        }
        self.call_stack_manager.set_places(
            arg_places.map(ensure_deter_place).collect(),
            ensure_deter_place(ret_val_place),
        );
        if let Some((arg_index, tuple_type_id)) = tupled_arg {
            let Local::Argument(arg_index) = arg_index else {
                unreachable!()
            };
            self.call_stack_manager
                .try_untuple_argument(arg_index, &|| {
                    Box::new(untuple::UntupleHelperImpl::new(
                        self.type_manager,
                        tuple_type_id,
                    ))
                });
        }

        let sanity = self.call_stack_manager.notify_enter(def);
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
        return_val.by.add_info(&antecedent);
        self.call_stack_manager
            .top()
            .set_place(DeterPlaceValueRef::new(result_dest).as_ref(), return_val);
    }

    fn metadata(self) -> Self::MetadataHandler {
        Default::default()
    }
}

mod untuple {
    use common::type_info::{FieldsShapeInfo, StructShape, TypeInfo};

    use crate::{
        abs::{FieldIndex, RawAddress, TypeId},
        type_info::{FieldsShapeInfoExt, TypeInfoExt},
    };

    use super::{DeterPlaceValueRef, backend};
    use backend::{BasicPlaceInfo, TypeDatabase, expr::prelude::DeterministicPlaceValue};

    pub(super) trait UntupleHelper {
        type PlaceInfo;
        type Place;

        fn make_tupled_arg_pseudo_place(&mut self, addr: RawAddress) -> Self::Place;

        fn num_fields(&mut self) -> FieldIndex;

        /// Takes a place and returns a place with projection to the field.
        /// It should make a valid place with full information needed for the state.
        fn field_place(&mut self, base: Self::Place, field: FieldIndex) -> Self::Place;
    }

    pub(super) trait BasicUntupleHelper:
        UntupleHelper<PlaceInfo = BasicPlaceInfo, Place = DeterPlaceValueRef>
    {
    }
    impl<T> BasicUntupleHelper for T where
        T: UntupleHelper<PlaceInfo = BasicPlaceInfo, Place = DeterPlaceValueRef>
    {
    }

    pub(crate) struct UntupleHelperImpl<'a> {
        pub(crate) type_manager: &'a dyn TypeDatabase,
        pub(crate) tuple_type_id: TypeId,
        pub(crate) type_info: Option<&'static TypeInfo>,
        pub(crate) fields_info: Option<&'static StructShape>,
    }

    impl UntupleHelper for UntupleHelperImpl<'_> {
        type PlaceInfo = BasicPlaceInfo;
        type Place = DeterPlaceValueRef;

        fn make_tupled_arg_pseudo_place(&mut self, addr: RawAddress) -> Self::Place {
            DeterPlaceValueRef::new(
                DeterministicPlaceValue::from_addr_type(addr, self.tuple_type_id).to_value_ref(),
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

    impl<'a> UntupleHelperImpl<'a> {
        pub(crate) fn new(type_manager: &'a dyn TypeDatabase, tuple_type_id: TypeId) -> Self {
            Self {
                type_manager,
                tuple_type_id,
                type_info: None,
                fields_info: None,
            }
        }

        #[inline]
        pub(crate) fn get_type(&self, type_id: TypeId) -> &'static TypeInfo {
            self.type_manager.get_type(&type_id)
        }

        pub(crate) fn type_info(&mut self) -> &'static TypeInfo {
            if self.type_info.is_none() {
                self.type_info = Some(self.get_type(self.tuple_type_id));
            }
            self.type_info.unwrap()
        }

        pub(crate) fn fields_info(&mut self) -> &'static StructShape {
            let type_info = self.type_info();
            self.fields_info
                .get_or_insert_with(|| match type_info.expect_single_variant().fields {
                    FieldsShapeInfo::Struct(ref shape) => shape,
                    _ => panic!("Expected tuple type info, got: {:?}", type_info),
                })
        }
    }
}
use untuple::BasicUntupleHelper;

use super::ImplicationInvestigator;
