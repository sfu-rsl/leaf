use derive_more as dm;

pub(crate) mod shared;

use crate::abs::{
    AssertKind, AssignmentId, BasicBlockIndex, BinaryOp, CalleeDef, CastKind, Constant, FieldIndex,
    FuncDef, Local, PlaceUsage, Projection, RawAddress, SwitchCaseIndex, SymVariable, Tag,
    TernaryOp, TypeId, TypeSize, UnaryOp, ValueType, VariantIndex, backend::Shutdown,
};

pub(crate) trait RuntimeBackend: Shutdown {
    type PlaceHandler<'a>: for<'b> PlaceHandler<
            PlaceInfo<'b> = Self::PlaceInfo,
            Place = Self::Place,
            DiscriminablePlace = Self::DiscriminablePlace,
        >
    where
        Self: 'a;

    type OperandHandler<'a>: OperandHandler<Operand = Self::Operand, Place = Self::Place>
    where
        Self: 'a;

    type AssignmentHandler<'a>: AssignmentHandler<
            Place = Self::Place,
            Operand = Self::Operand,
            DiscriminablePlace = Self::DiscriminablePlace,
        >
    where
        Self: 'a;
    type MemoryHandler<'a>: LifetimeHandler<Place = Self::Place>
    where
        Self: 'a;
    type RawMemoryHandler<'a>: RawMemoryHandler<Place = Self::Place, Operand = Self::Operand>
    where
        Self: 'a;
    type ConstraintHandler<'a>: ConstraintHandler<Operand = Self::Operand>
    where
        Self: 'a;
    type CallHandler<'a>: CallHandler<Place = Self::Place, Operand = Self::Operand>
    where
        Self: 'a;
    type DropHandler<'a>: DropHandler<Place = Self::Place, Operand = Self::Operand>
    where
        Self: 'a;
    type AnnotationHandler<'a>: AnnotationHandler
    where
        Self: 'a;

    type PlaceInfo;
    type Place;
    type DiscriminablePlace;
    type Operand;

    fn place(&mut self, usage: PlaceUsage) -> Self::PlaceHandler<'_>;

    fn operand(&mut self) -> Self::OperandHandler<'_>;

    fn assign_to<'a>(
        &'a mut self,
        id: AssignmentId,
        dest: <Self::AssignmentHandler<'a> as AssignmentHandler>::Place,
    ) -> Self::AssignmentHandler<'a>;

    fn memory<'a>(&'a mut self) -> Self::MemoryHandler<'a>;

    fn raw_memory<'a>(&'a mut self) -> Self::RawMemoryHandler<'a>;

    fn constraint_at<'a>(&'a mut self, location: BasicBlockIndex) -> Self::ConstraintHandler<'a>;

    fn call_control(&mut self) -> Self::CallHandler<'_>;

    fn dropping(&mut self) -> Self::DropHandler<'_>;

    fn annotate(&mut self) -> Self::AnnotationHandler<'_>;
}

pub(crate) trait PlaceHandler {
    type PlaceInfo<'a>;
    type Place;
    type DiscriminablePlace = Self::Place;

    fn from_info<'a>(self, info: Self::PlaceInfo<'a>) -> Self::Place;

    /// # Remarks
    /// Used for discriminant of enums.
    fn tag_of<'a>(self, info: Self::PlaceInfo<'a>) -> Self::DiscriminablePlace;
}

#[derive(dm::From)]
pub(crate) enum PlaceInfoBase {
    Local(Local),
    Some,
}

#[derive(dm::From)]
pub(crate) enum PlaceInfoProjection<I> {
    Projection(Projection<I>),
    Some,
}

pub(crate) trait PlaceBuilder {
    type Place;
    type Index = Self::Place;
    type Projector<'a>: PlaceProjector<Index = Self::Index>;
    type MetadataHandler<'a>: PlaceMetadataHandler;

    fn from_base(self, base: PlaceInfoBase) -> Self::Place;

    fn project_on<'a>(self, place: &'a mut Self::Place) -> Self::Projector<'a>;

    fn metadata<'a>(self, place: &'a mut Self::Place) -> Self::MetadataHandler<'a>;
}

pub(crate) trait PlaceProjector: Sized {
    type Index;

    fn by(self, projection: PlaceInfoProjection<Self::Index>);

    #[inline]
    fn deref(self) {
        self.by(Projection::Deref.into())
    }

    #[inline]
    fn for_field(self, field: FieldIndex) {
        self.by(Projection::Field(field).into())
    }

    #[inline]
    fn at_index(self, index: Self::Index) {
        self.by(Projection::Index(index).into())
    }

    #[inline]
    fn at_constant_index(self, offset: u64, min_length: u64, from_end: bool) {
        self.by(Projection::ConstantIndex {
            offset,
            min_length,
            from_end,
        }
        .into())
    }

    #[inline]
    fn subslice(self, from: u64, to: u64, from_end: bool) {
        self.by(Projection::Subslice { from, to, from_end }.into())
    }

    #[inline]
    fn downcast(self, variant: VariantIndex) {
        self.by(Projection::Downcast(variant).into())
    }

    #[inline]
    fn opaque_cast(self) {
        self.by(Projection::OpaqueCast.into())
    }

    #[inline]
    fn unwrap_unsafe_binder(self) {
        self.by(Projection::UnwrapUnsafeBinder.into())
    }
}

pub(crate) trait PlaceMetadataHandler {
    fn set_address(&mut self, address: RawAddress);

    fn set_type_id(&mut self, type_id: TypeId);

    fn set_primitive_type(&mut self, ty: ValueType);

    fn set_size(self, byte_size: TypeSize);
}

pub(crate) trait OperandHandler {
    type Operand;
    type Place;

    fn copy_of(self, place: Self::Place) -> Self::Operand;

    fn move_of(self, place: Self::Place) -> Self::Operand;

    fn const_from(self, info: super::Constant) -> Self::Operand;

    fn some(self) -> Self::Operand;

    fn new_symbolic(self, var: SymVariable<Self::Operand>) -> Self::Operand;
}

pub(crate) trait AssignmentHandler: Sized {
    type Place;
    type DiscriminablePlace = Self::Place;
    type Operand;

    fn use_of(self, _operand: Self::Operand) {
        self.some()
    }

    fn repeat_of(self, _operand: Self::Operand, _count: usize) {
        self.some()
    }

    fn ref_to(self, _place: Self::Place, _is_mutable: bool) {
        self.some()
    }

    fn thread_local_ref_to(self) {
        self.some()
    }

    // FIXME: Rename
    fn address_of(self, _place: Self::Place, _is_mutable: bool) {
        self.some()
    }

    fn cast_of(self, _operand: Self::Operand, _target: CastKind) {
        self.some()
    }

    fn binary_op_between(self, _operator: BinaryOp, _first: Self::Operand, _second: Self::Operand) {
        self.some()
    }

    fn unary_op_on(self, _operator: UnaryOp, _operand: Self::Operand) {
        self.some()
    }

    fn ternary_op_between(
        self,
        _operator: TernaryOp,
        _first: Self::Operand,
        _second: Self::Operand,
        _third: Self::Operand,
    ) {
        self.some()
    }

    fn carrying_mul_add(
        self,
        _multiplier: Self::Operand,
        _multiplicand: Self::Operand,
        _addend: Self::Operand,
        _carry: Self::Operand,
    ) {
        self.some()
    }

    fn discriminant_from(self, _place: Self::DiscriminablePlace) {
        self.some()
    }

    fn array_from(self, _items: impl Iterator<Item = Self::Operand>) {
        self.some()
    }

    fn tuple_from(self, fields: impl Iterator<Item = Self::Operand>) {
        self.adt_from(fields, None)
    }

    fn adt_from(
        self,
        _fields: impl Iterator<Item = Self::Operand>,
        _variant: Option<VariantIndex>,
    ) {
        self.some()
    }

    fn union_from(self, _active_field: FieldIndex, _value: Self::Operand) {
        self.some()
    }

    fn closure_from(self, upvars: impl Iterator<Item = Self::Operand>) {
        self.adt_from(upvars, None)
    }

    fn coroutine_from(self, upvars: impl Iterator<Item = Self::Operand>) {
        self.adt_from(upvars, None)
    }

    fn coroutine_closure_from(self, upvars: impl Iterator<Item = Self::Operand>) {
        self.adt_from(upvars, None)
    }

    fn raw_ptr_from(self, _data_ptr: Self::Operand, _metadata: Self::Operand, _is_mutable: bool) {
        self.some()
    }

    fn variant_index(self, _variant_index: VariantIndex) {
        self.some()
    }

    fn wrap_in_unsafe_binder(self, _value: Self::Operand) {
        self.some()
    }

    fn use_if_eq(self, _current: Self::Operand, _expected: Self::Operand, _then: Self::Operand) {
        self.some()
    }

    fn use_and_check_eq(self, _val: Self::Operand, _expected: Self::Operand) {
        self.some()
    }

    fn some(self);
}

pub(crate) trait LifetimeHandler {
    type Place;

    fn mark_live(self, place: Self::Place);

    fn mark_dead(self, place: Self::Place);
}

pub(crate) trait RawMemoryHandler {
    type Place;
    type Operand;

    fn place_from_ptr(
        self,
        ptr: Self::Operand,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        usage: PlaceUsage,
    ) -> Self::Place;

    fn copy(
        self,
        assignment_id: AssignmentId,
        src_ptr: Self::Operand,
        conc_src_ptr: RawAddress,
        dst_ptr: Self::Operand,
        conc_dst_ptr: RawAddress,
        count: Self::Operand,
        conc_count: usize,
        ptr_type_id: TypeId,
    );

    fn swap(
        self,
        assignment_id: AssignmentId,
        first_ptr: Self::Operand,
        conc_first_ptr: RawAddress,
        second_ptr: Self::Operand,
        conc_second_ptr: RawAddress,
        ptr_type_id: TypeId,
    );

    fn set(
        self,
        assignment_id: AssignmentId,
        ptr: Self::Operand,
        conc_ptr: RawAddress,
        value: Self::Operand,
        count: Self::Operand,
        conc_count: usize,
        ptr_type_id: TypeId,
    );
}

pub(crate) trait ConstraintHandler {
    type Operand;
    type SwitchHandler: SwitchHandler;

    fn switch(self, discriminant: Option<Self::Operand>) -> Self::SwitchHandler;

    fn assert(self, cond: Self::Operand, expected: bool, assert_kind: AssertKind<Self::Operand>);
}

pub(crate) trait SwitchHandler {
    fn take(self, case_index: SwitchCaseIndex, value: Option<super::Constant>);
    fn take_otherwise(self, non_values: Option<Vec<super::Constant>>);
}

#[derive(Clone, Copy)]
pub(crate) enum ArgsTupling {
    Normal,
    Untupled {
        tupled_arg_index: Local,
        tuple_type: TypeId,
    },
    Tupled,
}

// FIXME: Merge before calls and shift temporary storage to PRI.
pub(crate) trait CallHandler {
    type Place;
    type Operand;
    type MetadataHandler;

    fn before_call(self, def: CalleeDef, call_site: BasicBlockIndex);

    fn before_call_some(self);

    fn take_data_before_call(
        self,
        func: Self::Operand,
        args: impl IntoIterator<Item = Self::Operand>,
        are_args_tupled: bool,
    );

    fn enter(self, def: FuncDef);

    fn emplace_arguments(
        self,
        arg_places: Vec<Self::Place>,
        ret_val_place: Self::Place,
        tupling: ArgsTupling,
    );

    fn override_return_value(self, value: Self::Operand);

    fn ret(self, ret_point: BasicBlockIndex);

    fn after_call(self, assignment_id: AssignmentId, result_dest: Self::Place);

    fn metadata(self) -> Self::MetadataHandler;
}

pub(crate) trait DropHandler {
    type Place;
    type Operand;

    fn before_drop(self, def: CalleeDef, call_site: BasicBlockIndex);

    fn before_drop_some(self);

    fn take_data_before_drop(self, func: Self::Operand, arg: Self::Operand, place: Self::Place);

    fn after_drop(self);
}

pub(crate) trait AnnotationHandler {
    fn push_tag(self, tag: Tag);

    fn pop_tag(self);
}
