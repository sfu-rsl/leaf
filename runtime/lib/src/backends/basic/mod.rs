mod alias;
mod call;
mod concrete;
mod config;
pub(crate) mod expr;
mod outgen;
mod place;
mod state;
mod sym_vars;
mod trace;
mod types;

use std::{
    cell::{RefCell, RefMut},
    iter,
    ops::DerefMut,
    rc::Rc,
};

use common::{
    log_debug, log_info,
    tyexp::{FieldsShapeInfo, StructShape, TagEncodingInfo, TagInfo, TypeInfo},
};
use sym_vars::SymVariablesManager;

use crate::{
    abs::{
        self, AssertKind, BasicBlockLocation, CalleeDef, CastKind, ConstraintKind, FieldIndex,
        FuncDef, IntType, Local, LocalIndex, PlaceUsage, SymVariable, Tag, TypeId, UnaryOp,
        VariantIndex, backend::*, place::HasMetadata,
    },
    tyexp::{FieldsShapeInfoExt, TypeInfoExt},
    utils::alias::RRef,
};

use self::{
    alias::{
        BasicExprBuilder, BasicSymExprBuilder, TraceManager, TypeManager,
        ValueRefExprBuilder as OperationalExprBuilder,
        ValueRefUnaryExprBuilder as UnaryExprBuilder,
    },
    concrete::BasicConcretizer,
    config::BasicBackendConfig,
    expr::{SymVarId, prelude::*},
    place::PlaceMetadata,
    state::{RawPointerVariableState, make_sym_place_handler},
    types::BasicTypeManager,
};

type BasicTraceManager = dyn TraceManager;

type BasicVariablesState = RawPointerVariableState<BasicSymExprBuilder>;

type BasicCallStackManager = call::BasicCallStackManager<BasicVariablesState>;

type BasicSymVariablesManager = sym_vars::BasicSymVariablesManager;

type Place = place::PlaceWithMetadata;
type Projection = place::Projection;
pub(crate) use place::BasicPlaceBuilder;

const LOG_TAG_TAGS: &str = "tags";

pub struct BasicBackend {
    call_stack_manager: BasicCallStackManager,
    trace_manager: RRef<BasicTraceManager>,
    expr_builder: RRef<BasicExprBuilder>,
    sym_values: RRef<BasicSymVariablesManager>,
    type_manager: Rc<dyn TypeManager>,
    tags: RRef<Vec<Tag>>,
}

impl BasicBackend {
    pub fn new(config: BasicBackendConfig) -> Self {
        let type_manager_ref = Rc::new(BasicTypeManager::default());
        let expr_builder_ref = Rc::new(RefCell::new(expr::builders::new_expr_builder(
            type_manager_ref.clone(),
        )));
        let expr_builder = expr_builder_ref.clone();
        let sym_var_manager = Rc::new(RefCell::new(BasicSymVariablesManager::new()));

        let tags_ref = Rc::new(RefCell::new(Vec::new()));
        let type_manager = type_manager_ref.clone();
        let trace_manager_ref = Rc::new(RefCell::new(trace::new_trace_manager(
            tags_ref.clone(),
            sym_var_manager.clone(),
            &config.exe_trace,
            &config.outputs,
            &config.solver,
        )));
        let trace_manager = trace_manager_ref.clone();

        let sym_place_handler_factory = |s| {
            Rc::new(RefCell::from(make_sym_place_handler(s, || {
                Box::new(BasicConcretizer::new(
                    expr_builder_ref.clone(),
                    trace_manager_ref.clone(),
                ))
            })))
        };
        let sym_read_handler_ref = sym_place_handler_factory(config.sym_place.read);
        let sym_write_handler_ref = sym_place_handler_factory(config.sym_place.write);

        Self {
            call_stack_manager: BasicCallStackManager::new(
                Box::new(move |_id| {
                    let vars_state = RawPointerVariableState::new(
                        type_manager_ref.clone(),
                        sym_read_handler_ref.clone(),
                        sym_write_handler_ref.clone(),
                        Rc::new(RefCell::new(expr::builders::to_sym_expr_builder(
                            expr_builder_ref.clone(),
                        ))),
                    );

                    vars_state
                }),
                &config.call,
            ),
            trace_manager,
            expr_builder,
            sym_values: sym_var_manager.clone(),
            type_manager,
            tags: tags_ref.clone(),
        }
    }
}

impl RuntimeBackend for BasicBackend {
    type PlaceHandler<'a>
        = BasicPlaceHandler<'a>
    where
        Self: 'a;

    type OperandHandler<'a>
        = BasicOperandHandler<'a>
    where
        Self: 'a;

    type AssignmentHandler<'a>
        = BasicAssignmentHandler<'a, BasicExprBuilder>
    where
        Self: 'a;

    type ConstraintHandler<'a>
        = BasicConstraintHandler<'a, BasicExprBuilder>
    where
        Self: 'a;

    type FunctionHandler<'a>
        = BasicFunctionHandler<'a>
    where
        Self: 'a;

    type AnnotationHandler<'a>
        = BasicAnnotationHandler<'a>
    where
        Self: 'a;

    type PlaceInfo = Place;
    type Place = PlaceValueRef;
    type Operand = ValueRef;

    fn place(&mut self, usage: PlaceUsage) -> Self::PlaceHandler<'_> {
        BasicPlaceHandler {
            vars_state: self.call_stack_manager.top(),
            usage,
            type_manager: self.type_manager.as_ref(),
        }
    }

    fn operand(&mut self) -> Self::OperandHandler<'_> {
        BasicOperandHandler {
            vars_state: self.call_stack_manager.top(),
            sym_values: self.sym_values.clone(),
        }
    }

    fn assign_to<'a>(
        &'a mut self,
        dest: <Self::AssignmentHandler<'a> as AssignmentHandler>::Place,
    ) -> Self::AssignmentHandler<'a> {
        BasicAssignmentHandler::new(dest, self)
    }

    fn constraint_at(&mut self, location: BasicBlockLocation) -> Self::ConstraintHandler<'_> {
        BasicConstraintHandler::new(self, location)
    }

    fn func_control(&mut self) -> Self::FunctionHandler<'_> {
        BasicFunctionHandler::new(self)
    }

    fn annotate(&mut self) -> Self::AnnotationHandler<'_> {
        BasicAnnotationHandler::new(self)
    }
}

pub(crate) struct BasicPlaceHandler<'a> {
    vars_state: &'a mut dyn VariablesState,
    usage: PlaceUsage,
    type_manager: &'a dyn TypeManager,
}

impl PlaceHandler for BasicPlaceHandler<'_> {
    type PlaceInfo<'a> = Place;
    type Place = PlaceValueRef;
    type DiscriminablePlace = DiscriminantPossiblePlace;
    type Operand = ValueRef;

    fn from_info<'a>(self, info: Self::PlaceInfo<'a>) -> Self::Place {
        self.vars_state.ref_place(&info, self.usage)
    }

    fn tag_of<'a>(self, info: Self::PlaceInfo<'a>) -> Self::DiscriminablePlace {
        let mut place = info;
        let type_manager: &dyn TypeManager = self.type_manager;
        let ty = type_manager.get_type(place.metadata().unwrap_type_id());
        let (tag_as_field, tag_encoding) = match ty.tag.as_ref() {
            Some(TagInfo::Constant { discr_bit_rep }) => {
                return DiscriminantPossiblePlace::SingleVariant {
                    discr_bit_rep: *discr_bit_rep,
                };
            }
            Some(TagInfo::Regular { as_field, encoding }) => (as_field, encoding),
            None => return DiscriminantPossiblePlace::None,
        };
        let metadata = {
            let mut meta = PlaceMetadata::default();
            meta.set_address(
                place
                    .address()
                    .wrapping_byte_add(tag_as_field.offset as usize),
            );
            let tag_ty = type_manager.get_type(tag_as_field.ty);
            meta.set_type_id(tag_ty.id);
            if let Some(value_ty) = type_manager.try_to_value_type(tag_ty) {
                meta.set_ty(value_ty);
            }
            meta.set_size(tag_ty.size);
            meta
        };
        place.add_projection(Projection::Field(0));
        place.push_metadata(metadata);
        DiscriminantPossiblePlace::TagPlaceWithInfo(self.from_info(place), tag_encoding)
    }

    fn from_ptr(self, ptr: Self::Operand, ptr_type_id: TypeId) -> Self::Place {
        self.vars_state
            .ref_place_by_ptr(ptr, ptr_type_id, self.usage)
    }
}

pub(crate) struct BasicOperandHandler<'a> {
    vars_state: &'a mut dyn VariablesState,
    sym_values: RRef<BasicSymVariablesManager>,
}

impl OperandHandler for BasicOperandHandler<'_> {
    type Place = PlaceValueRef;
    type Operand = ValueRef;

    fn copy_of(self, place: Self::Place) -> Self::Operand {
        self.vars_state.copy_place(&place)
    }

    fn move_of(self, place: Self::Place) -> Self::Operand {
        self.vars_state.take_place(&place)
    }

    fn const_from(self, info: Self::Constant) -> Self::Operand {
        ConcreteValue::from(info).to_value_ref()
    }

    fn new_symbolic(self, var: SymVariable<Self::Operand>) -> Self::Operand {
        self.sym_values.borrow_mut().add_variable(var).into()
    }
}

pub(crate) struct BasicAssignmentHandler<'s, EB: OperationalExprBuilder> {
    dest: PlaceValueRef,
    vars_state: &'s mut dyn VariablesState,
    expr_builder: RRef<EB>,
    type_manager: &'s dyn TypeManager,
}

impl<'s> BasicAssignmentHandler<'s, BasicExprBuilder> {
    fn new(dest: PlaceValueRef, backend: &'s mut BasicBackend) -> Self {
        Self {
            dest,
            vars_state: backend.call_stack_manager.top(),
            expr_builder: backend.expr_builder.clone(),
            type_manager: backend.type_manager.as_ref(),
        }
    }
}

impl<EB: OperationalExprBuilder> AssignmentHandler for BasicAssignmentHandler<'_, EB> {
    type Place = PlaceValueRef;
    type DiscriminablePlace = DiscriminantPossiblePlace;
    type Operand = ValueRef;

    fn use_of(mut self, operand: Self::Operand) {
        self.set(operand)
    }

    fn repeat_of(mut self, operand: Self::Operand, count: usize) {
        let value = if operand.is_symbolic() {
            ConcreteValue::Array(ArrayValue {
                elements: vec![operand; count],
            })
            .into()
        } else {
            UnevalValue::Some.into()
        };
        self.set_value(value)
    }

    fn ref_to(mut self, place: Self::Place, _is_mutable: bool) {
        let value = if place.is_symbolic() {
            Expr::Ref(SymPlaceValueRef::new(place)).into()
        } else {
            UnevalValue::Some.into()
        };
        self.set_value(value)
    }

    fn thread_local_ref_to(mut self) {
        // Thread local references cannot refer to symbolic places, so the reference is concrete.
        self.set_value(UnevalValue::Some.into())
    }

    fn address_of(self, place: Self::Place, is_mutable: bool) {
        // For symbolic values `ref_to` and `address_of` should have the same behavior.
        self.ref_to(place, is_mutable)
    }

    fn len_of(mut self, place: Self::Place) {
        let value = if place.is_symbolic() {
            Expr::Len(SymPlaceValueRef::new(place)).into()
        } else {
            UnevalValue::Some.into()
        };
        self.set_value(value)
    }

    fn cast_of(mut self, operand: Self::Operand, target: CastKind) {
        let cast_value: ValueRef = self
            .expr_builder()
            .cast(operand.into(), target, self.dest.type_info().clone())
            .into();

        self.set(cast_value)
    }

    fn binary_op_between(
        mut self,
        operator: crate::abs::BinaryOp,
        first: Self::Operand,
        second: Self::Operand,
    ) {
        let result_value = self
            .expr_builder()
            .binary_op((first, second).into(), operator);
        self.set(result_value.into())
    }

    fn unary_op_on(mut self, operator: UnaryOp, operand: Self::Operand) {
        let result_value = self.expr_builder().unary_op(operand.into(), operator);
        self.set(result_value.into())
    }

    fn discriminant_from(mut self, place: Self::DiscriminablePlace) {
        let discr_value = match place {
            DiscriminantPossiblePlace::None => {
                // https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/mir/enum.Rvalue.html#variant.Discriminant
                ConstValue::new_int(0_u128, self.get_int_type(self.dest.type_info())).to_value_ref()
            }
            DiscriminantPossiblePlace::SingleVariant { discr_bit_rep } => {
                ConstValue::new_int(discr_bit_rep, self.get_int_type(self.dest.type_info()))
                    .to_value_ref()
            }
            DiscriminantPossiblePlace::TagPlaceWithInfo(tag_place, tag_encoding) => {
                let tag_value = self.vars_state.copy_place(&tag_place);
                if tag_value.is_symbolic() {
                    self.build_discriminant_expr(
                        SymValueRef::new(tag_value),
                        tag_encoding,
                        self.dest.type_info(),
                        tag_place.type_info(),
                    )
                    .into()
                } else {
                    tag_value
                }
            }
        };

        self.set(discr_value)
    }

    fn array_from(mut self, elements: impl Iterator<Item = Self::Operand>) {
        let mut has_symbolic = false;
        let value = ConcreteValue::Array(ArrayValue {
            elements: elements
                .inspect(|e| has_symbolic |= e.is_symbolic())
                .collect(),
        });
        let value = if has_symbolic {
            value.into()
        } else {
            UnevalValue::Some.into()
        };
        self.set_value(value)
    }

    fn adt_from(
        mut self,
        fields: impl Iterator<Item = Self::Field>,
        variant: Option<VariantIndex>,
    ) {
        let kind = match variant {
            Some(variant) => AdtKind::Enum { variant },
            None => AdtKind::Struct,
        };
        self.set_adt_value(kind, fields.map(|f| Some(f)))
    }

    fn union_from(mut self, active_field: abs::FieldIndex, value: Self::Field) {
        let fields = (0..active_field)
            .map(|_| None)
            .chain(iter::once(Some(value)));
        self.set_adt_value(AdtKind::Struct, fields.into_iter())
    }

    fn raw_ptr_from(self, data_ptr: Self::Operand, metadata: Self::Operand, _is_mutable: bool) {
        self.adt_from([data_ptr, metadata].into_iter(), None)
    }

    // TODO: Need to add support for the Deinit MIR instruction to have this working properly.
    // This solution works for now to avoid crashes when samples are run.
    fn variant_index(mut self, variant_index: VariantIndex) {
        let value = Value::Concrete(ConcreteValue::Adt(AdtValue {
            kind: AdtKind::Enum {
                variant: variant_index,
            },
            fields: vec![],
        }));
        self.set_value(value)
    }

    fn shallow_init_box_from(self, value: Self::Operand) {
        /* According to the Rust MIR documentation:
         * https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/mir/enum.Rvalue.html#variant.ShallowInitBox
         * > Transmutes a *mut u8 into shallow-initialized Box<T>.
         * BTW, very improbable to have a symbolic value here. */
        let dst_ty_id = self.dest.type_info().id().unwrap();
        self.cast_of(value, CastKind::Transmute(dst_ty_id));
    }

    fn use_if_eq(mut self, val: Self::Operand, current: Self::Operand, expected: Self::Operand) {
        let are_eq = self
            .expr_builder()
            .eq((current.clone(), expected.clone()).into());
        let are_eq = if !are_eq.is_symbolic() {
            // As it will be abstracted to Some, we need to resolve them explicitly here.
            let current =
                ConcreteValueRef::new(current.clone()).try_resolve_as_const(self.type_manager);
            let expected = ConcreteValueRef::new(expected).try_resolve_as_const(self.type_manager);
            match (current, expected) {
                (Some(current), Some(expected)) => {
                    let are_eq = current == expected;
                    ConstValue::Bool(are_eq).to_value_ref()
                }
                _ => are_eq,
            }
        } else {
            are_eq
        };
        let value = self.expr_builder().if_then_else((are_eq, val, current));
        self.set(value)
    }

    fn use_and_check_eq(mut self, val: Self::Operand, expected: Self::Operand) {
        let are_eq = self.expr_builder().eq((val.clone(), expected).into());
        self.set_adt_value(AdtKind::Struct, [Some(val), Some(are_eq)].into_iter())
    }
}

impl<EB: OperationalExprBuilder> BasicAssignmentHandler<'_, EB> {
    #[inline]
    fn set(&mut self, value: ValueRef) {
        self.vars_state.set_place(&self.dest, value);
    }

    #[inline]
    fn set_value(&mut self, value: Value) {
        self.set(value.to_value_ref());
    }

    fn expr_builder(&self) -> impl DerefMut<Target = EB> + '_ {
        self.expr_builder.as_ref().borrow_mut()
    }

    fn set_adt_value(
        &mut self,
        kind: AdtKind,
        fields: impl Iterator<Item = Option<<Self as AssignmentHandler>::Field>>,
    ) {
        let mut has_symbolic = false;
        let fields = fields
            .map(|f| AdtField {
                value: f.inspect(|f| has_symbolic |= f.is_symbolic()),
            })
            .collect();
        let value = if has_symbolic {
            ConcreteValue::Adt(AdtValue { kind, fields }).into()
        } else {
            UnevalValue::Some.into()
        };
        self.set_value(value)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn build_discriminant_expr(
        &self,
        tag_value: SymValueRef,
        tag_encoding: &common::tyexp::TagEncodingInfo,
        discr_ty_info: &LazyTypeInfo,
        tag_ty_info: &LazyTypeInfo,
    ) -> SymValueRef {
        use common::tyexp::TagEncodingInfo::*;
        match tag_encoding {
            Direct => tag_value,
            Niche {
                non_niche_value,
                niche_value_range,
                tag_value_start,
            } => {
                let discr_ty = self.get_int_type(discr_ty_info);
                let tag_ty = self.get_int_type(tag_ty_info);

                let into_tag_value = |v: u128| ConstValue::new_int(v, tag_ty).to_value_ref();
                let into_discr_value = |v: u128| ConstValue::new_int(v, discr_ty).to_value_ref();

                // Based on: `rustc_codegen_ssa::mir::place::PlaceRef::codegen_get_discr`
                let niche_start = *niche_value_range.start();
                let tag_value: ValueRef = tag_value.into();
                let relative_max = niche_value_range.end() - niche_start;
                let (is_niche, tagged_discr) = if relative_max == 0 {
                    let is_niche = SymValueRef::new(
                        self.expr_builder()
                            .eq((tag_value.clone(), into_tag_value(*tag_value_start)).into())
                            .into(),
                    );
                    let tagged_discr = into_discr_value(niche_start);
                    (is_niche, tagged_discr)
                } else {
                    let relative_tag_value: ValueRef = self
                        .expr_builder()
                        .sub((tag_value.clone(), into_tag_value(*tag_value_start)).into())
                        .into();
                    let is_niche = SymValueRef::new(
                        self.expr_builder()
                            .le((relative_tag_value.clone(), into_tag_value(relative_max)).into())
                            .into(),
                    );
                    let relative_discr_value = self
                        .expr_builder()
                        .to_int(relative_tag_value.into(), discr_ty, discr_ty_info.clone())
                        .into();
                    let tagged_discr = self
                        .expr_builder()
                        .add((relative_discr_value, into_discr_value(niche_start)).into())
                        .into();
                    let tagged_discr =
                        self.expr_builder()
                            .to_int(tagged_discr, discr_ty, discr_ty_info.clone());
                    debug_assert!(tagged_discr.is_symbolic());
                    (is_niche, tagged_discr)
                };

                let discr_value = self.expr_builder().if_then_else((
                    is_niche.into(),
                    tagged_discr.into(),
                    into_discr_value(*non_niche_value),
                ));
                SymValueRef::new(discr_value)
            }
        }
    }

    fn get_int_type(&self, ty_info: &LazyTypeInfo) -> IntType {
        impl ValueType {
            #[inline]
            fn expect_int(&self) -> IntType {
                match self {
                    ValueType::Int(ty) => *ty,
                    _ => {
                        // https://doc.rust-lang.org/reference/type-layout.html#primitive-representations
                        panic!(
                            "Expected the type of the tag to be a int type. Found: {:?}",
                            self
                        )
                    }
                }
            }
        }

        self.type_manager
            .try_to_value_type(ty_info.clone())
            .expect("Expected the type of the discriminant raw value to be a primitive")
            .expect_int()
    }
}

pub(crate) struct BasicConstraintHandler<'a, EB> {
    location: BasicBlockLocation,
    trace_manager: RefMut<'a, BasicTraceManager>,
    expr_builder: RRef<EB>,
}

impl<'a> BasicConstraintHandler<'a, BasicExprBuilder> {
    fn new(backend: &'a mut BasicBackend, location: BasicBlockLocation) -> Self {
        Self {
            trace_manager: backend.trace_manager.borrow_mut(),
            expr_builder: backend.expr_builder.clone(),
            location,
        }
    }
}

impl<'a, EB: UnaryExprBuilder> ConstraintHandler for BasicConstraintHandler<'a, EB> {
    type Operand = ValueRef;
    type SwitchHandler = BasicSwitchHandler<'a, EB>;

    fn switch(self, discriminant: Self::Operand) -> Self::SwitchHandler {
        let discr = self.expr_builder.borrow_mut().no_op(discriminant);
        BasicSwitchHandler {
            discr,
            parent: self,
        }
    }

    fn assert(
        mut self,
        cond: Self::Operand,
        expected: bool,
        _assert_kind: AssertKind<Self::Operand>,
    ) {
        // For now, we will call this function before the assert occurs and assume that assertions always succeed.
        // TODO: add a result: bool parameter to this function, and add support for it using a panic hook.
        if cond.is_symbolic() {
            // NOTE: This is a trick to pass the value through the expression builder
            // to ensure value resolving and simplifications.
            let expr = self.expr_builder.borrow_mut().no_op(cond);
            let mut constraint = Constraint {
                discr: expr,
                kind: ConstraintKind::True,
            };
            if !expected {
                constraint = constraint.not();
            }

            self.notify_constraint(constraint);
        }
    }
}

impl<'a, EB> BasicConstraintHandler<'a, EB> {
    fn notify_constraint(&mut self, constraint: Constraint) {
        self.trace_manager
            .notify_step(self.location.into(), constraint);
    }
}

pub(crate) struct BasicSwitchHandler<'a, EB> {
    discr: ValueRef,
    parent: BasicConstraintHandler<'a, EB>,
}

impl<'a, EB> SwitchHandler for BasicSwitchHandler<'a, EB> {
    fn take(mut self, value: Self::Constant) {
        let constraint = self.create_constraint(vec![value]);
        self.parent.notify_constraint(constraint);
    }

    fn take_otherwise(mut self, non_values: Vec<Self::Constant>) {
        let constraint = self.create_constraint(non_values).not();
        self.parent.notify_constraint(constraint);
    }
}

impl<'a, EB> BasicSwitchHandler<'a, EB> {
    fn create_constraint(&mut self, values: Vec<<Self as SwitchHandler>::Constant>) -> Constraint {
        let kind = match values.first().unwrap() {
            abs::Constant::Bool(false) => ConstraintKind::False,
            _ => ConstraintKind::OneOf(
                values
                    .into_iter()
                    .map(|c| ConstValue::try_from(c).unwrap())
                    .collect(),
            ),
        };
        Constraint {
            discr: self.discr.clone(),
            kind,
        }
    }
}

pub(crate) struct BasicFunctionHandler<'a> {
    call_stack_manager: &'a mut dyn CallStackManager,
    type_manager: &'a dyn TypeManager,
}

impl<'a> BasicFunctionHandler<'a> {
    fn new(backend: &'a mut BasicBackend) -> Self {
        Self {
            call_stack_manager: &mut backend.call_stack_manager,
            type_manager: backend.type_manager.as_ref(),
        }
    }
}

impl<'a> FunctionHandler for BasicFunctionHandler<'a> {
    type Place = PlaceValueRef;
    type Operand = ValueRef;
    type MetadataHandler = ();

    #[inline]
    fn before_call(
        self,
        def: CalleeDef,
        func: Self::Operand,
        args: impl Iterator<Item = Self::Arg>,
        are_args_tupled: bool,
    ) {
        self.call_stack_manager
            .prepare_for_call(def, func, args.collect(), are_args_tupled);
    }

    fn enter(
        self,
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
                    Box::new(BasicUntupleHelper::new(self.type_manager, tuple_type_id))
                });
        }
        self.call_stack_manager.notify_enter(def);
    }

    #[inline]
    fn override_return_value(self, value: Self::Operand) {
        self.call_stack_manager.override_return_value(value)
    }

    #[inline]
    fn ret(self) {
        self.call_stack_manager.pop_stack_frame();
    }

    fn after_call(self, result_dest: Self::Place) {
        debug_assert!(!result_dest.is_symbolic());
        self.call_stack_manager
            .finalize_call(DeterPlaceValueRef::new(result_dest));
    }

    fn metadata(self) -> Self::MetadataHandler {
        Default::default()
    }
}

struct BasicUntupleHelper<'a> {
    type_manager: &'a dyn TypeManager,
    tuple_type_id: TypeId,
    type_info: Option<&'static TypeInfo>,
    fields_info: Option<&'static StructShape>,
}

impl GenericUntupleHelper for BasicUntupleHelper<'_> {
    type PlaceInfo = Place;
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

impl<'a> BasicUntupleHelper<'a> {
    fn new(type_manager: &'a dyn TypeManager, tuple_type_id: TypeId) -> Self {
        Self {
            type_manager,
            tuple_type_id,
            type_info: None,
            fields_info: None,
        }
    }

    #[inline]
    fn get_type(&self, type_id: TypeId) -> &'static TypeInfo {
        self.type_manager.get_type(type_id)
    }

    fn type_info(&mut self) -> &'static TypeInfo {
        if self.type_info.is_none() {
            self.type_info = Some(self.get_type(self.tuple_type_id));
        }
        self.type_info.unwrap()
    }

    fn fields_info(&mut self) -> &'static StructShape {
        let type_info = self.type_info();
        self.fields_info
            .get_or_insert_with(|| match type_info.expect_single_variant().fields {
                FieldsShapeInfo::Struct(ref shape) => shape,
                _ => panic!("Expected tuple type info, got: {:?}", type_info),
            })
    }
}

pub(crate) struct BasicAnnotationHandler<'a> {
    tags: RefMut<'a, Vec<common::pri::Tag>>,
}

impl<'a> BasicAnnotationHandler<'a> {
    fn new(backend: &'a mut BasicBackend) -> Self {
        Self {
            tags: backend.tags.borrow_mut(),
        }
    }

    fn log_current_tags(&self) {
        log_debug!(target: LOG_TAG_TAGS, "Current tags: [{}]", self.tags.join(", "));
    }
}

impl<'a> AnnotationHandler for BasicAnnotationHandler<'a> {
    fn push_tag(mut self, tag: common::pri::Tag) {
        self.tags.push(tag);
        self.log_current_tags();
    }

    fn pop_tag(mut self) {
        self.tags.pop();
        self.log_current_tags();
    }
}

impl Shutdown for BasicBackend {
    fn shutdown(&mut self) {
        log_info!("Shutting down the basic backend");
        self.trace_manager.borrow_mut().shutdown();
    }
}

type Constraint = crate::abs::Constraint<ValueRef, ConstValue>;

pub(crate) enum DiscriminantPossiblePlace {
    None,
    SingleVariant { discr_bit_rep: u128 },
    TagPlaceWithInfo(PlaceValueRef, &'static TagEncodingInfo),
}

trait GenericVariablesState {
    type PlaceInfo = Place;
    type PlaceValue = PlaceValueRef;
    type Value = ValueRef;

    fn id(&self) -> usize;

    /// Returns a value that corresponds to the place itself.
    /// The returned value does not necessarily access the actual value but
    /// should be dereferenceable to get the actual value.
    fn ref_place(&self, place: &Self::PlaceInfo, usage: PlaceUsage) -> Self::PlaceValue;

    /// Returns a value that corresponds to the place pointer by the pointer.
    /// Effectively, this is equivalent to the place that would be represented by `*ptr`.
    fn ref_place_by_ptr(
        &self,
        ptr: Self::Value,
        ptr_type_id: TypeId,
        usage: PlaceUsage,
    ) -> Self::PlaceValue;

    /// Returns a copy of the value stored at the given place. May not physically copy the value
    /// but the returned value should be independently usable from the original value.
    fn copy_place(&self, place: &Self::PlaceValue) -> Self::Value;

    /// Returns the value stored at the given place.
    /// Conceptually, it is required that the place will not contain the value right after this operation.
    fn take_place(&mut self, place: &Self::PlaceValue) -> Self::Value;

    /// Sets the value of a place. Overwrites the previous value if any, also defines a new local
    /// variable if it does not exist.
    fn set_place(&mut self, place: &Self::PlaceValue, value: Self::Value);
}

trait VariablesState:
    GenericVariablesState<PlaceInfo = Place, PlaceValue = PlaceValueRef, Value = ValueRef>
{
}
impl<T> VariablesState for T where
    T: GenericVariablesState<PlaceInfo = Place, PlaceValue = PlaceValueRef, Value = ValueRef>
{
}

trait GenericCallStackManager {
    type VariablesState: GenericVariablesState;
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
        untuple_helper: &dyn Fn() -> Box<dyn UntupleHelper + 'b>,
    );

    fn notify_enter(&mut self, current_func: FuncDef);

    fn pop_stack_frame(&mut self);

    fn override_return_value(&mut self, value: Self::Value);

    fn finalize_call(&mut self, result_dest: Self::Place);

    fn top(&mut self) -> &mut Self::VariablesState;
}

trait GenericUntupleHelper {
    type PlaceInfo;
    type Place;

    fn make_tupled_arg_pseudo_place(&mut self, addr: RawAddress) -> Self::Place;

    fn num_fields(&mut self) -> FieldIndex;

    /// Takes a place and returns a place with projection to the field.
    /// It should make a valid place with full information needed for the state.
    fn field_place(&mut self, base: Self::Place, field: FieldIndex) -> Self::Place;
}

trait UntupleHelper: GenericUntupleHelper<PlaceInfo = Place, Place = DeterPlaceValueRef> {}
impl<T> UntupleHelper for T where
    T: GenericUntupleHelper<PlaceInfo = Place, Place = DeterPlaceValueRef>
{
}

trait CallStackManager:
    GenericCallStackManager<
        VariablesState = BasicVariablesState,
        Place = DeterPlaceValueRef,
        Value = ValueRef,
    >
{
}
impl<T> CallStackManager for T where
    T: GenericCallStackManager<
            VariablesState = BasicVariablesState,
            Place = DeterPlaceValueRef,
            Value = ValueRef,
        >
{
}
