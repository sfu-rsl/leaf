use std::{
    fmt::Display,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use common::{log_warn, types::RawAddress};

use crate::abs::{Local, TypeId, TypeSize, ValueType, place::HasMetadata};

use super::alias::backend;
use backend::{MdSanPlaceInfo, MdSanPlaceValue};

mod data_types {
    use common::log_error;

    use super::*;

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
            self.address.unwrap_or_else(||{
                log_error!("Presumably an unchecked null pointer dereference is happening in the program. Runtime will terminate the execution.");
                panic!("Address of place is not available.")
            }).as_ptr()
        }

        #[inline]
        pub(crate) fn set_address(&mut self, address: RawAddress) {
            debug_assert!(self.address.is_none());
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
            debug_assert!(self.size.is_none());
            self.size = Some(size);
        }
    }

    pub(crate) type LocalWithMetadata = crate::abs::place::LocalWithMetadata<PlaceMetadata>;

    pub(crate) type PlaceWithMetadata =
        crate::abs::place::PlaceWithMetadata<LocalWithMetadata, Projection, PlaceMetadata>;

    pub(crate) type Projection = crate::abs::Projection<MdSanPlaceValue>;

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
                Ok(place.base().clone())
            } else {
                Err(place)
            }
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
}
pub(crate) use data_types::*;

mod builders {
    use common::log_info;

    use crate::pri::fluent::backend::{
        PlaceBuilder, PlaceInfoBase, PlaceInfoProjection, PlaceMetadataHandler, PlaceProjector,
        shared::DefaultPlaceProjectionHandler,
    };

    use super::*;

    #[derive(Default)]
    pub(crate) struct MdSanPlaceBuilder;

    macro_rules! err_on_partial_place_info {
        () => {{
            log_info!("Place info is not fully available.");
            unimplemented!("Partial place info is not supported in this backend yet.")
        }};
    }

    impl PlaceBuilder for MdSanPlaceBuilder {
        type Place = PlaceWithMetadata;
        type Index = MdSanPlaceValue;
        type Projector<'a> = MdSanProjectionBuilder<'a>;
        type MetadataHandler<'a> = MdSanPlaceMetadataHandler<'a>;

        fn from_base(self, base: PlaceInfoBase) -> Self::Place {
            let base = match base {
                PlaceInfoBase::Local(local) => local,
                PlaceInfoBase::Some => err_on_partial_place_info!(),
            };
            PlaceWithMetadata::from(Self::Place::from(base))
        }

        fn project_on<'a>(self, place: &'a mut Self::Place) -> Self::Projector<'a> {
            MdSanProjectionBuilder(place)
        }

        fn metadata(self, place: &mut Self::Place) -> Self::MetadataHandler<'_> {
            MdSanPlaceMetadataHandler(place)
        }
    }

    pub(crate) struct MdSanProjectionBuilder<'a>(&'a mut PlaceWithMetadata);

    impl PlaceProjector for MdSanProjectionBuilder<'_> {
        type Index = MdSanPlaceValue;

        fn by(self, proj: PlaceInfoProjection<Self::Index>) {
            self.0.push_metadata(PlaceMetadata::default());
            DefaultPlaceProjectionHandler::new(&mut self.0.deref_mut()).by(proj);
        }
    }

    impl From<PlaceInfoProjection<MdSanPlaceValue>> for Projection {
        #[inline]
        fn from(value: PlaceInfoProjection<MdSanPlaceValue>) -> Self {
            match value {
                PlaceInfoProjection::Projection(proj) => proj.map(|index| index),
                PlaceInfoProjection::Some => err_on_partial_place_info!(),
            }
        }
    }

    pub(crate) struct MdSanPlaceMetadataHandler<'a>(&'a mut PlaceWithMetadata);

    impl PlaceMetadataHandler for MdSanPlaceMetadataHandler<'_> {
        fn set_address(&mut self, address: RawAddress) {
            if self.0.has_projection() {
                let last = &mut self.0.projs_metadata_mut().last().unwrap();
                last.set_address(address);
            } else {
                self.0.base_mut().set_address(address);
            }
        }

        fn set_type_id(&mut self, type_id: TypeId) {
            if self.0.has_projection() {
                let last = &mut self.0.projs_metadata_mut().last().unwrap();
                debug_assert!(last.type_id().is_none());
                last.set_type_id(type_id);
            } else {
                self.0.base_mut().set_type_id(type_id);
            }
        }

        fn set_primitive_type(&mut self, ty: ValueType) {
            self.0.metadata_mut().set_ty(ty);
        }

        fn set_size(self, byte_size: crate::abs::TypeSize) {
            self.0.metadata_mut().set_size(byte_size);
        }
    }
}
pub(crate) use builders::MdSanPlaceBuilder;

mod handlers {
    use common::type_info::{TagEncodingInfo, TagInfo};

    use crate::{
        abs::PlaceUsage, backends::mdsan::MdMemoryState, pri::fluent::backend::PlaceHandler,
    };

    use super::*;
    use backend::{MdSanBackend, MdSanPlaceInfo, MdSanVariablesState};

    pub(crate) struct MdSanPlaceHandler<'a> {
        vars_state: &'a mut MdSanVariablesState,
        usage: PlaceUsage,
        // type_manager: &'a dyn TypeDatabase,
    }

    impl<'a> MdSanPlaceHandler<'a> {
        pub fn new(usage: PlaceUsage, backend: &'a mut MdSanBackend) -> MdSanPlaceHandler<'a> {
            Self {
                vars_state: &mut backend.vars_state,
                usage,
                // type_manager: backend.type_manager.as_ref(),
            }
        }
    }

    impl PlaceHandler for MdSanPlaceHandler<'_> {
        type PlaceInfo<'a> = MdSanPlaceInfo;
        type Place = MdSanPlaceValue;

        fn from_info<'a>(self, info: Self::PlaceInfo<'a>) -> Self::Place {
            self.vars_state.ref_place(info, self.usage)
        }

        fn tag_of<'a>(self, info: Self::PlaceInfo<'a>) -> Self::DiscriminablePlace {
            // It cannot be an MD, but may be the wrapped data
            let place = self.vars_state.ref_place(info, self.usage);
            // If it is a wrapped data, then the tag is also a wrapped data, if not it is non-relevant
            place
        }
    }
}
pub(crate) use handlers::MdSanPlaceHandler;
