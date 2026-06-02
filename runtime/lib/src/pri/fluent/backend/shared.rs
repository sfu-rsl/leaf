use core::marker::PhantomData;

use crate::abs::{Place, Projection};

use super::*;

pub(crate) struct DefaultPlaceBuilder<B = Local, I = B, P = Projection<I>> {
    _phantom: PhantomData<(B, I, P)>,
}

impl<L, P> Default for DefaultPlaceBuilder<L, P> {
    fn default() -> Self {
        Self {
            _phantom: Default::default(),
        }
    }
}

impl<B, I, P> PlaceBuilder for DefaultPlaceBuilder<B, I, P>
where
    B: From<PlaceInfoBase>,
    P: From<PlaceInfoProjection<I>>,
    for<'a> B: 'a,
    for<'a> I: 'a,
    for<'a> P: 'a,
{
    type Place = Place<B, P>;
    type Index = I;
    type Projector<'a>
        = DefaultPlaceProjectionHandler<'a, Self::Place, Self::Index>
    where
        Self::Place: 'a;
    type MetadataHandler<'a> = ();

    fn from_base(self, base: PlaceInfoBase) -> Self::Place {
        Place::new(base.into())
    }

    fn project_on<'a>(self, place: &'a mut Self::Place) -> Self::Projector<'a> {
        DefaultPlaceProjectionHandler::new(place)
    }

    fn metadata(self, _place: &mut Self::Place) {}
}

pub(crate) struct DefaultPlaceProjectionHandler<'a, P, I> {
    place: &'a mut P,
    _phantom: PhantomData<I>,
}

impl<'a, B, I, P> DefaultPlaceProjectionHandler<'a, Place<B, P>, I> {
    pub(crate) fn new(place: &'a mut Place<B, P>) -> Self {
        Self {
            place,
            _phantom: Default::default(),
        }
    }
}

impl<'a, B, I, P> PlaceProjector for DefaultPlaceProjectionHandler<'a, Place<B, P>, I>
where
    P: From<PlaceInfoProjection<I>>,
{
    type Index = I;

    fn by(self, projection: PlaceInfoProjection<Self::Index>) {
        self.place.add_projection(projection.into())
    }
}

impl PlaceMetadataHandler for () {
    fn set_address(&mut self, _address: RawAddress) {}

    fn set_type_id(&mut self, _type_id: TypeId) {}

    fn set_primitive_type(&mut self, _ty: ValueType) {}

    fn set_size(self, _byte_size: TypeSize) {}
}

pub(crate) mod noop {
    use super::*;

    pub(crate) type NullPlace = ();

    #[derive(Default)]
    pub(crate) struct NoOpPlaceBuilder<P, I>(PhantomData<(P, I)>);

    impl<P: Default, I> PlaceBuilder for NoOpPlaceBuilder<P, I> {
        type Place = P;
        type Index = I;
        type Projector<'a> = Self;
        type MetadataHandler<'a> = ();

        fn from_base(self, _base: PlaceInfoBase) -> Self::Place {
            Default::default()
        }

        fn project_on<'a>(self, _place: &'a mut Self::Place) -> Self::Projector<'a> {
            self
        }

        fn metadata<'a>(self, _place: &'a mut Self::Place) -> Self::MetadataHandler<'a> {
            Default::default()
        }
    }

    impl<P, I> PlaceProjector for NoOpPlaceBuilder<P, I> {
        type Index = I;

        fn by(self, _proj: PlaceInfoProjection<Self::Index>) {}
    }

    #[derive(Default)]
    pub(crate) struct NoOpPlaceHandler<PI, P>(PhantomData<(PI, P)>);

    impl<PI, P: Default> PlaceHandler for NoOpPlaceHandler<PI, P> {
        type PlaceInfo<'a> = PI;
        type Place = P;

        fn from_info<'a>(self, _info: Self::PlaceInfo<'a>) -> Self::Place {
            Default::default()
        }

        fn tag_of<'a>(self, _info: Self::PlaceInfo<'a>) -> Self::DiscriminablePlace {
            Default::default()
        }
    }

    #[derive(Default)]
    pub(crate) struct NoOpOperandHandler<P, O>(PhantomData<(P, O)>);

    impl<P, O: Default> OperandHandler for NoOpOperandHandler<P, O> {
        type Operand = O;
        type Place = P;

        fn copy_of(self, _place: Self::Place) -> Self::Operand {
            Default::default()
        }

        fn move_of(self, _place: Self::Place) -> Self::Operand {
            Default::default()
        }

        fn const_from(self, _info: Constant) -> Self::Operand {
            Default::default()
        }

        fn some(self) -> Self::Operand {
            Default::default()
        }

        fn new_symbolic(self, _var: SymVariable<Self::Operand>) -> Self::Operand {
            Default::default()
        }
    }

    #[derive(Default)]
    pub(crate) struct NoOpAssignmentHandler<P, O>(PhantomData<(P, O)>);

    impl<P, O> AssignmentHandler for NoOpAssignmentHandler<P, O> {
        type Place = P;
        type Operand = O;

        fn use_of(self, _operand: Self::Operand) {}

        fn repeat_of(self, _operand: Self::Operand, _count: usize) {}

        fn ref_to(self, _place: Self::Place, _is_mutable: bool) {}

        fn thread_local_ref_to(self) {}

        fn address_of(self, _place: Self::Place, _is_mutable: bool) {}
        fn cast_of(self, _operand: Self::Operand, _target: CastKind) {}

        fn binary_op_between(
            self,
            _operator: BinaryOp,
            _first: Self::Operand,
            _second: Self::Operand,
        ) {
        }

        fn unary_op_on(self, _operator: UnaryOp, _operand: Self::Operand) {}

        fn discriminant_from(self, _place: Self::DiscriminablePlace) {}

        fn array_from(self, _items: impl Iterator<Item = Self::Operand>) {}

        fn adt_from(
            self,
            _fields: impl Iterator<Item = Self::Operand>,
            _variant: Option<VariantIndex>,
        ) {
        }

        fn union_from(self, _active_field: FieldIndex, _value: Self::Operand) {}
        fn raw_ptr_from(
            self,
            _data_ptr: Self::Operand,
            _metadata: Self::Operand,
            _is_mutable: bool,
        ) {
        }

        fn variant_index(self, _variant_index: VariantIndex) {}

        fn shallow_init_box_from(self, _value: Self::Operand) {}

        fn wrap_in_unsafe_binder(self, _value: Self::Operand) {}

        fn use_if_eq(
            self,
            _current: Self::Operand,
            _expected: Self::Operand,
            _then: Self::Operand,
        ) {
        }
        fn use_and_check_eq(self, _val: Self::Operand, _expected: Self::Operand) {}

        fn some(self) {}
    }

    #[derive(Default)]
    pub(crate) struct NoOpMemoryHandler;

    impl MemoryHandler for NoOpMemoryHandler {
        type Place = NullPlace;

        fn mark_live(self, _place: Self::Place) {}

        fn mark_dead(self, _place: Self::Place) {}
    }

    #[derive(Default)]
    pub(crate) struct NoOpRawMemoryHandler<P, O>(PhantomData<(P, O)>);

    impl<P: Default, O> RawMemoryHandler for NoOpRawMemoryHandler<P, O> {
        type Place = P;
        type Operand = O;

        fn place_from_ptr(
            self,
            _ptr: Self::Operand,
            _conc_ptr: RawAddress,
            _ptr_type_id: TypeId,
            _usage: PlaceUsage,
        ) -> Self::Place {
            Default::default()
        }

        fn copy(
            self,
            _assignment_id: AssignmentId,
            _src_ptr: Self::Operand,
            _conc_src_ptr: RawAddress,
            _dst_ptr: Self::Operand,
            _conc_dst_ptr: RawAddress,
            _count: Self::Operand,
            _conc_count: usize,
            _ptr_type_id: TypeId,
        ) {
            Default::default()
        }

        fn swap(
            self,
            _assignment_id: AssignmentId,
            _first_ptr: Self::Operand,
            _conc_first_ptr: RawAddress,
            _second_ptr: Self::Operand,
            _conc_second_ptr: RawAddress,
            _ptr_type_id: TypeId,
        ) {
            Default::default()
        }

        fn set(
            self,
            _assignment_id: AssignmentId,
            _ptr: Self::Operand,
            _conc_ptr: RawAddress,
            _value: Self::Operand,
            _count: Self::Operand,
            _conc_count: usize,
            _ptr_type_id: TypeId,
        ) {
            Default::default()
        }
    }

    pub(crate) struct NoOpConstraintHandler<O>(PhantomData<O>);

    impl<O> Default for NoOpConstraintHandler<O> {
        fn default() -> Self {
            Self(Default::default())
        }
    }

    #[derive(Default)]
    pub(crate) struct NoOpSwitchHandler;

    impl<O> ConstraintHandler for NoOpConstraintHandler<O> {
        type Operand = O;

        type SwitchHandler = NoOpSwitchHandler;

        fn switch(self, _discriminant: Option<Self::Operand>) -> Self::SwitchHandler {
            Default::default()
        }

        fn assert(
            self,
            _cond: Self::Operand,
            _expected: bool,
            _assert_kind: AssertKind<Self::Operand>,
        ) {
            Default::default()
        }
    }

    impl SwitchHandler for NoOpSwitchHandler {
        fn take(self, _case_index: SwitchCaseIndex, _value: Option<Constant>) {
            Default::default()
        }

        fn take_otherwise(self, _non_values: Option<Vec<Constant>>) {
            Default::default()
        }
    }

    #[derive(Default)]
    pub(crate) struct NoOpAnnotationHandler;

    impl AnnotationHandler for NoOpAnnotationHandler {
        fn push_tag(self, _tag: Tag) {}

        fn pop_tag(self) {}
    }
}
