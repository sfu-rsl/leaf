use core::{marker::PhantomData, ops::DerefMut};

use common::log_info;

use crate::abs::place::{
    DefaultPlaceMetadata, GenericPlaceWithMetadata, HasMetadata, Local, Place, Projection,
};

use super::*;

pub(crate) struct DefaultPlaceBuilder<B = Local, I = B, P = Projection<I>> {
    _phantom: PhantomData<(B, I, P)>,
}

impl<B, I, P> Default for DefaultPlaceBuilder<B, I, P> {
    fn default() -> Self {
        Self {
            _phantom: Default::default(),
        }
    }
}

impl<B, I, P> PlaceBuilder for DefaultPlaceBuilder<B, I, P>
where
    B: From<Local>,
    P: From<Projection<I>>,
    B: HasMetadata<Metadata = DefaultPlaceMetadata>,
    for<'a> B: 'a,
    for<'a> I: 'a,
    for<'a> P: 'a,
{
    type Place = GenericPlaceWithMetadata<B, P, DefaultPlaceMetadata>;
    type Index = I;
    type Projector<'a>
        = DefaultPlaceProjectionHandler<'a, B, P, I>
    where
        Self::Place: 'a;
    type MetadataHandler<'a> = DefaultMetadataHandler<'a, Self::Place>;

    fn from_base(self, base: PlaceInfoBase) -> Self::Place {
        match base {
            PlaceInfoBase::Local(local) => GenericPlaceWithMetadata::from(Place::new(local.into())),
            PlaceInfoBase::Some => {
                log_info!("Place info is not fully available.");
                unimplemented!("Partial place info is not supported in this backend yet.")
            }
        }
    }

    fn project_on<'a>(self, place: &'a mut Self::Place) -> Self::Projector<'a> {
        place.push_metadata(Default::default());
        DefaultPlaceProjectionHandler::new(place.deref_mut())
    }

    fn metadata<'a>(self, place: &'a mut Self::Place) -> Self::MetadataHandler<'a> {
        DefaultMetadataHandler::new(place)
    }
}

pub(crate) struct DefaultPlaceProjectionHandler<'a, B, P, I> {
    place: &'a mut Place<B, P>,
    _phantom: PhantomData<I>,
}

impl<'a, B, P, I> DefaultPlaceProjectionHandler<'a, B, P, I> {
    pub(crate) fn new(place: &'a mut Place<B, P>) -> Self {
        Self {
            place,
            _phantom: Default::default(),
        }
    }
}

impl<'a, B, P, I> PlaceProjector for DefaultPlaceProjectionHandler<'a, B, P, I>
where
    P: From<Projection<I>>,
{
    type Index = I;

    fn by(self, projection: PlaceInfoProjection<Self::Index>) {
        match projection {
            PlaceInfoProjection::Projection(projection) => {
                self.place.add_projection(projection.into())
            }
            PlaceInfoProjection::Some => {
                log_info!("Place info is not fully available.");
                unimplemented!("Partial place info is not supported in this backend yet.")
            }
        }
    }
}

impl PlaceMetadataHandler for () {
    fn set_address(&mut self, _address: RawAddress) {}

    fn set_type_id(&mut self, _type_id: TypeId) {}

    fn set_primitive_type(&mut self, _ty: ValueType) {}

    fn set_size(self, _byte_size: TypeSize) {}
}

pub(crate) struct DefaultMetadataHandler<'a, P> {
    place: &'a mut P,
}

impl<'a, P> DefaultMetadataHandler<'a, P> {
    pub(crate) fn new(place: &'a mut P) -> Self {
        Self { place }
    }
}

impl<P: HasMetadata<Metadata = DefaultPlaceMetadata>> PlaceMetadataHandler
    for DefaultMetadataHandler<'_, P>
{
    fn set_address(&mut self, address: RawAddress) {
        self.place.metadata_mut().set_address(address);
    }

    fn set_type_id(&mut self, type_id: TypeId) {
        self.place.metadata_mut().set_type_id(type_id);
    }

    fn set_primitive_type(&mut self, ty: ValueType) {
        self.place.metadata_mut().set_ty(ty);
    }

    fn set_size(self, byte_size: TypeSize) {
        self.place.metadata_mut().set_size(byte_size);
    }
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

        fn ternary_op_between(
            self,
            _operator: TernaryOp,
            _first: Self::Operand,
            _second: Self::Operand,
            _third: Self::Operand,
        ) {
        }

        fn carrying_mul_add(
            self,
            _multiplier: Self::Operand,
            _multiplicand: Self::Operand,
            _addend: Self::Operand,
            _carry: Self::Operand,
        ) {
        }

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
    pub(crate) struct NoOpLifetimeHandler;

    impl LifetimeHandler for NoOpLifetimeHandler {
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
