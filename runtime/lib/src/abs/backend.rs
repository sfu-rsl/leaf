use std::collections::HashMap;

use super::{
    AssertKind, AssignmentId, BasicBlockIndex, BasicBlockLocation, BinaryOp, CalleeDef, CastKind,
    Constraint, ConstraintKind, FieldIndex, FuncDef, IntType, Local, PlaceUsage, Projection,
    SymVariable, Tag, TypeId, UnaryOp, ValueType, VariantIndex,
};

pub(crate) trait RuntimeBackend: Shutdown {
    type PlaceHandler<'a>: for<'b> PlaceHandler<PlaceInfo<'b> = Self::PlaceInfo, Place = Self::Place>
    where
        Self: 'a;

    type OperandHandler<'a>: OperandHandler
    where
        Self: 'a;

    type AssignmentHandler<'a>: AssignmentHandler<Place = Self::Place, Operand = Self::Operand>
    where
        Self: 'a;
    type MemoryHandler<'a>: MemoryHandler<Place = Self::Place>
    where
        Self: 'a;
    type ConstraintHandler<'a>: ConstraintHandler<Operand = Self::Operand>
    where
        Self: 'a;
    type CallHandler<'a>: CallHandler<Place = Self::Place, Operand = Self::Operand>
    where
        Self: 'a;
    type AnnotationHandler<'a>: AnnotationHandler
    where
        Self: 'a;

    type PlaceInfo;
    type Place;
    type Operand;

    fn place(&mut self, usage: PlaceUsage) -> Self::PlaceHandler<'_>;

    fn operand(&mut self) -> Self::OperandHandler<'_>;

    fn assign_to<'a>(
        &'a mut self,
        id: AssignmentId,
        dest: <Self::AssignmentHandler<'a> as AssignmentHandler>::Place,
    ) -> Self::AssignmentHandler<'a>;

    fn memory<'a>(&'a mut self) -> Self::MemoryHandler<'a>;

    fn constraint_at(&mut self, location: BasicBlockIndex) -> Self::ConstraintHandler<'_>;

    fn call_control(&mut self) -> Self::CallHandler<'_>;

    fn annotate(&mut self) -> Self::AnnotationHandler<'_>;
}

pub(crate) trait PlaceHandler {
    type PlaceInfo<'a>;
    type Place;
    type DiscriminablePlace = Self::Place;
    type Operand;

    fn from_info<'a>(self, info: Self::PlaceInfo<'a>) -> Self::Place;

    /// # Remarks
    /// Used for discriminant of enums.
    fn tag_of<'a>(self, info: Self::PlaceInfo<'a>) -> Self::DiscriminablePlace;

    fn from_ptr(self, ptr: Self::Operand, ptr_type_id: TypeId) -> Self::Place;
}

pub(crate) trait PlaceBuilder {
    type Place;
    type ProjectionBuilder<'a>;
    type MetadataHandler<'a>;

    fn of_local(self, local: Local) -> Self::Place;

    fn project_on<'a>(self, place: &'a mut Self::Place) -> Self::ProjectionBuilder<'a>;

    fn metadata<'a>(self, place: &'a mut Self::Place) -> Self::MetadataHandler<'a>;
}

pub(crate) trait PlaceProjectionBuilder: Sized {
    type Result = ();
    type Index;

    fn by(self, projection: Projection<Self::Index>) -> Self::Result;

    #[inline]
    fn deref(self) -> Self::Result {
        self.by(Projection::Deref)
    }

    #[inline]
    fn for_field(self, field: FieldIndex) -> Self::Result {
        self.by(Projection::Field(field))
    }

    #[inline]
    fn at_index(self, index: Self::Index) -> Self::Result {
        self.by(Projection::Index(index))
    }

    #[inline]
    fn at_constant_index(self, offset: u64, min_length: u64, from_end: bool) -> Self::Result {
        self.by(Projection::ConstantIndex {
            offset,
            min_length,
            from_end,
        })
    }

    #[inline]
    fn subslice(self, from: u64, to: u64, from_end: bool) -> Self::Result {
        self.by(Projection::Subslice { from, to, from_end })
    }

    #[inline]
    fn downcast(self, variant: VariantIndex) -> Self::Result {
        self.by(Projection::Downcast(variant))
    }

    #[inline]
    fn opaque_cast(self) -> Self::Result {
        self.by(Projection::OpaqueCast)
    }

    #[inline]
    fn subtype(self) -> Self::Result {
        self.by(Projection::Subtype)
    }
}

pub(crate) trait OperandHandler {
    type Operand;
    type Place;
    type Constant = super::Constant;

    fn copy_of(self, place: Self::Place) -> Self::Operand;

    fn move_of(self, place: Self::Place) -> Self::Operand;

    fn const_from(self, info: Self::Constant) -> Self::Operand;

    fn new_symbolic(self, var: SymVariable<Self::Operand>) -> Self::Operand;
}

pub(crate) trait AssignmentHandler: Sized {
    type Place;
    type DiscriminablePlace = Self::Place;
    type Operand;
    type Field = Self::Operand;

    fn use_of(self, operand: Self::Operand);

    fn repeat_of(self, operand: Self::Operand, count: usize);

    fn ref_to(self, place: Self::Place, is_mutable: bool);

    fn thread_local_ref_to(self);

    // FIXME: Rename
    fn address_of(self, place: Self::Place, is_mutable: bool);

    fn len_of(self, place: Self::Place);

    fn cast_of(self, operand: Self::Operand, target: CastKind);

    fn binary_op_between(self, operator: BinaryOp, first: Self::Operand, second: Self::Operand);

    fn unary_op_on(self, operator: UnaryOp, operand: Self::Operand);

    fn discriminant_from(self, place: Self::DiscriminablePlace);

    fn array_from(self, items: impl Iterator<Item = Self::Operand>);

    fn tuple_from(self, fields: impl Iterator<Item = Self::Field>) {
        self.adt_from(fields, None)
    }

    fn adt_from(self, fields: impl Iterator<Item = Self::Field>, variant: Option<VariantIndex>);

    fn union_from(self, active_field: FieldIndex, value: Self::Field);

    fn closure_from(self, upvars: impl Iterator<Item = Self::Field>) {
        self.adt_from(upvars, None)
    }

    fn coroutine_from(self, upvars: impl Iterator<Item = Self::Field>) {
        self.adt_from(upvars, None)
    }

    fn coroutine_closure_from(self, upvars: impl Iterator<Item = Self::Field>) {
        self.adt_from(upvars, None)
    }

    fn raw_ptr_from(self, data_ptr: Self::Operand, metadata: Self::Operand, is_mutable: bool);

    fn variant_index(self, variant_index: VariantIndex);

    fn shallow_init_box_from(self, value: Self::Operand);

    fn use_if_eq(self, current: Self::Operand, expected: Self::Operand, then: Self::Operand);

    fn use_and_check_eq(self, val: Self::Operand, expected: Self::Operand);
}

pub(crate) trait MemoryHandler {
    type Place;

    fn mark_dead(self, place: Self::Place);
}

pub(crate) trait ConstraintHandler {
    type Operand;
    type SwitchHandler: SwitchHandler;

    fn switch(self, discriminant: Self::Operand) -> Self::SwitchHandler;

    fn assert(self, cond: Self::Operand, expected: bool, assert_kind: AssertKind<Self::Operand>);
}

pub(crate) trait SwitchHandler {
    type Constant = super::Constant;

    fn take(self, value: Self::Constant);
    fn take_otherwise(self, non_values: Vec<Self::Constant>);
}

pub(crate) enum ArgsTupling {
    Normal,
    Untupled {
        tupled_arg_index: Local,
        tuple_type: TypeId,
    },
    Tupled,
}

pub(crate) trait CallHandler {
    type Place;
    type Operand;
    type Arg = Self::Operand;
    type MetadataHandler;

    fn before_call(
        self,
        def: CalleeDef,
        call_site: BasicBlockIndex,
        func: Self::Operand,
        args: impl IntoIterator<Item = Self::Arg>,
        are_args_tupled: bool,
    );

    fn enter(
        self,
        def: FuncDef,
        arg_places: Vec<Self::Place>,
        ret_val_place: Self::Place,
        tupling: ArgsTupling,
    );

    fn override_return_value(self, value: Self::Operand);

    fn ret(self, ret_point: BasicBlockIndex);

    fn after_call(self, assignment_id: AssignmentId, result_dest: Self::Place);

    fn metadata(self) -> Self::MetadataHandler;
}

pub(crate) trait AnnotationHandler {
    fn push_tag(self, tag: Tag);

    fn pop_tag(self);
}

pub(crate) trait Shutdown {
    fn shutdown(&mut self);
}

/// Keeps track of all the compounding constraints in a single trace
pub(crate) trait TraceManager<S, V, C> {
    fn notify_step(&mut self, step: S, constraint: Constraint<V, C>);
}

pub(crate) type Model<I, A> = HashMap<I, A>;

/// A trait for the SMT solver.
/// It takes a set of constraints to check satisfiability of them together.
pub(crate) trait Solver {
    type Value;
    type Case;
    type Model;

    fn check(
        &mut self,
        constraints: impl Iterator<Item = Constraint<Self::Value, Self::Case>>,
    ) -> SolveResult<Self::Model>;
}

/// The result of the checking performed by [`Solver`].
/// [`Sat`]: The constraints are satisfiable and a model is found.
/// [`Unsat`]: The constraints are unsatisfiable.
/// [`Unknown`]: The solver could not determine the satisfiability.
pub(crate) enum SolveResult<M> {
    Sat(M),
    Unsat,
    Unknown,
}

pub(crate) use common::type_info::TypeDatabase;

macro_rules! fn_by_name {
    ($($name:ident),*$(,)?) => {
        $(
            #[allow(unused)]
            fn $name(&self) -> V;
        )*
    };
}

pub trait CoreTypeProvider<V> {
    common::type_info::pass_core_type_names_to!(fn_by_name);

    fn int_type(&self, ty: IntType) -> V {
        match (ty.is_signed, ty.bit_size as u32) {
            (true, i8::BITS) => self.i8(),
            (false, u8::BITS) => self.u8(),
            (true, i16::BITS) => self.i16(),
            (false, u16::BITS) => self.u16(),
            (true, i32::BITS) => self.i32(),
            (false, u32::BITS) => self.u32(),
            (true, i64::BITS) => self.i64(),
            (false, u64::BITS) => self.u64(),
            (true, i128::BITS) => self.i128(),
            (false, u128::BITS) => self.u128(),
            _ => unreachable!("Unexpected integer type: {:?}", ty),
        }
    }

    fn try_to_value_type<'a>(&self, ty: V) -> Option<ValueType>;
}

pub(crate) trait CallTraceRecorder {
    fn notify_call(
        &mut self,
        call_site: BasicBlockLocation<FuncDef>,
        entered_func: FuncDef,
        broken: bool,
    );

    fn notify_return(
        &mut self,
        ret_point: BasicBlockLocation<FuncDef>,
        caller_func: FuncDef,
        broken: bool,
    );
}

pub(crate) trait PhasedCallTraceRecorder: CallTraceRecorder {
    fn start_call(&mut self, call_site: BasicBlockLocation<FuncDef>);

    fn finish_call(&mut self, entered_func: FuncDef, broken: bool);

    fn start_return(&mut self, ret_point: BasicBlockLocation<FuncDef>);

    /// # Returns
    /// The call site which the execution has returned to.
    fn finish_return(&mut self, broken: bool) -> BasicBlockLocation<FuncDef>;
}

pub(crate) trait DecisionTraceRecorder {
    type Case;

    /// # Returns
    /// The step index.
    fn notify_decision(
        &mut self,
        node_location: BasicBlockLocation,
        kind: &ConstraintKind<Self::Case>,
    ) -> usize;
}

pub(crate) mod implementation {
    use super::super::*;
    use super::*;

    use std::marker::PhantomData;

    pub(crate) struct DefaultPlaceBuilder<L = Local, P = Projection<Local>> {
        _phantom: PhantomData<(L, P)>,
    }

    impl<L, P> Default for DefaultPlaceBuilder<L, P> {
        fn default() -> Self {
            Self {
                _phantom: Default::default(),
            }
        }
    }

    impl<L, P> PlaceBuilder for DefaultPlaceBuilder<L, P>
    where
        L: From<Local>,
        for<'a> L: 'a,
        for<'a> P: 'a,
    {
        type Place = Place<L, P>;
        type ProjectionBuilder<'a>
            = DefaultPlaceProjectionHandler<'a, Self::Place>
        where
            Self::Place: 'a;
        type MetadataHandler<'a> = ();

        fn of_local(self, local: Local) -> Self::Place {
            Place::new(local.into())
        }

        fn project_on<'a>(self, place: &'a mut Self::Place) -> Self::ProjectionBuilder<'a> {
            DefaultPlaceProjectionHandler { place }
        }

        fn metadata(self, _place: &mut Self::Place) {}
    }

    pub(crate) struct DefaultPlaceProjectionHandler<'a, P> {
        place: &'a mut P,
    }

    impl<'a, L, P> DefaultPlaceProjectionHandler<'a, Place<L, P>> {
        pub(crate) fn new(place: &'a mut Place<L, P>) -> Self {
            Self { place }
        }
    }

    impl<L, I> PlaceProjectionBuilder for DefaultPlaceProjectionHandler<'_, Place<L, Projection<I>>> {
        type Index = I;

        fn by(self, projection: Projection<Self::Index>) {
            self.place.add_projection(projection.into())
        }
    }
}
