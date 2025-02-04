use std::{
    fmt::Display,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use common::{log_warn, types::RawAddress};

use crate::abs::{
    Local, TypeId, TypeSize, ValueType,
    backend::{
        PlaceBuilder, PlaceProjectionHandler,
        implementation::{DefaultPlaceBuilder, DefaultPlaceProjectionHandler},
    },
    place::HasMetadata,
};

use super::{PlaceValueRef, expr::place::DeterPlaceValueRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaceMetadata {
    address: Option<NonNull<()>>,
    type_id: Option<TypeId>,
    // FIXME: Temporary until merged with type system
    ty: Option<ValueType>,
    size: Option<TypeSize>,
}

impl Default for PlaceMetadata {
    fn default() -> Self {
        Self {
            address: None,
            type_id: None,
            ty: None,
            size: None,
        }
    }
}

impl PlaceMetadata {
    #[inline]
    pub(crate) fn address(&self) -> RawAddress {
        self.address.unwrap().as_ptr()
    }

    #[inline]
    pub(crate) fn set_address(&mut self, address: RawAddress) {
        self.address = NonNull::new(address as *mut ());
        if self.address.is_none() {
            log_warn!("Setting null address to place metadata. {:?}", self);
        }
    }

    #[inline]
    pub(crate) fn type_id(&self) -> Option<TypeId> {
        self.type_id
    }

    #[inline]
    pub(crate) fn unwrap_type_id(&self) -> TypeId {
        self.type_id.expect("Type id is not available.")
    }

    #[inline]
    pub(crate) fn set_type_id(&mut self, type_id: TypeId) {
        self.type_id = Some(type_id);
    }

    #[inline]
    pub(crate) fn ty(&self) -> Option<&ValueType> {
        self.ty.as_ref()
    }

    #[inline]
    pub(crate) fn set_ty(&mut self, ty: ValueType) {
        self.ty = Some(ty);
    }

    #[inline]
    pub(crate) fn size(&self) -> Option<TypeSize> {
        self.size.as_ref().copied()
    }

    #[inline]
    pub(crate) fn set_size(&mut self, size: TypeSize) {
        self.size = Some(size);
    }
}

pub(crate) type LocalWithMetadata = crate::abs::place::LocalWithMetadata<PlaceMetadata>;

pub(crate) type PlaceWithMetadata =
    crate::abs::place::PlaceWithMetadata<LocalWithMetadata, Projection, PlaceMetadata>;

pub(crate) type Projection = crate::abs::Projection<DeterPlaceValueRef>;

impl PlaceWithMetadata {
    pub(crate) fn address(&self) -> RawAddress {
        self.metadata().address()
    }
}

impl From<Local> for PlaceWithMetadata {
    fn from(value: Local) -> Self {
        Self::from(LocalWithMetadata::from(value))
    }
}

impl AsMut<PlaceWithMetadata> for PlaceWithMetadata {
    fn as_mut(&mut self) -> &mut PlaceWithMetadata {
        self
    }
}

impl TryFrom<PlaceWithMetadata> for LocalWithMetadata {
    type Error = PlaceWithMetadata;

    fn try_from(place: PlaceWithMetadata) -> Result<Self, Self::Error> {
        if !place.has_projection() {
            Ok(place.local().clone())
        } else {
            Err(place)
        }
    }
}

#[derive(Default)]
pub(crate) struct BasicPlaceBuilder;

impl PlaceBuilder for BasicPlaceBuilder {
    type Place = PlaceWithMetadata;

    type ProjectionHandler<'a> = BasicProjectionBuilder<'a>;

    type MetadataHandler<'a> = BasicPlaceMetadataHandler<'a>;

    fn of_local(self, local: Local) -> Self::Place {
        PlaceWithMetadata::from(DefaultPlaceBuilder::default().of_local(local))
    }

    fn project_on<'a>(self, place: &'a mut Self::Place) -> Self::ProjectionHandler<'a> {
        BasicProjectionBuilder(place)
    }

    fn metadata(self, place: &mut Self::Place) -> Self::MetadataHandler<'_> {
        BasicPlaceMetadataHandler(place)
    }
}

pub(crate) struct BasicProjectionBuilder<'a>(&'a mut PlaceWithMetadata);

impl PlaceProjectionHandler for BasicProjectionBuilder<'_> {
    type Index = PlaceValueRef;

    fn by(self, proj: crate::abs::Projection<Self::Index>) {
        self.0.push_metadata(PlaceMetadata::default());
        let projection = proj.map(|index| DeterPlaceValueRef::new(index));
        DefaultPlaceProjectionHandler::new(&mut self.0.deref_mut()).by(projection);
    }
}

pub(crate) struct BasicPlaceMetadataHandler<'a>(&'a mut PlaceWithMetadata);

impl BasicPlaceMetadataHandler<'_> {
    pub(crate) fn set_address(&mut self, address: RawAddress) {
        if self.0.has_projection() {
            let last = &mut self.0.projs_metadata_mut().last().unwrap();
            debug_assert!(last.address.is_none());
            last.set_address(address);
        } else {
            self.0.local_mut().set_address(address);
        }
    }

    pub(crate) fn set_type_id(&mut self, type_id: TypeId) {
        if self.0.has_projection() {
            let last = &mut self.0.projs_metadata_mut().last().unwrap();
            debug_assert!(last.type_id().is_none());
            last.set_type_id(type_id);
        } else {
            self.0.local_mut().set_type_id(type_id);
        }
    }

    pub(crate) fn set_primitive_type(&mut self, ty: ValueType) {
        self.0.metadata_mut().set_ty(ty);
    }

    pub(crate) fn set_size(self, byte_size: crate::abs::TypeSize) {
        self.0.metadata_mut().size = Some(byte_size);
    }
}

impl Display for LocalWithMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{:p}", AsRef::<Local>::as_ref(self), self.address())
    }
}

impl Display for PlaceWithMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{:p}", self.deref(), self.address())
    }
}
