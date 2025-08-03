use std::{
    fmt::Display,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use common::{log_warn, types::RawAddress};

use crate::abs::{Local, TypeId, TypeSize, ValueType, place::HasMetadata};

use crate::backends::basic as backend;
use backend::{PlaceValueRef, expr::place::DeterPlaceValueRef};

mod data_types {
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
            self.address.unwrap().as_ptr()
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
    use crate::abs::backend::{
        PlaceBuilder, PlaceProjectionBuilder,
        implementation::{DefaultPlaceBuilder, DefaultPlaceProjectionHandler},
    };

    use super::*;

    #[derive(Default)]
    pub(crate) struct BasicPlaceBuilder;

    impl PlaceBuilder for BasicPlaceBuilder {
        type Place = PlaceWithMetadata;
        type ProjectionBuilder<'a> = BasicProjectionBuilder<'a>;
        type MetadataHandler<'a> = BasicPlaceMetadataHandler<'a>;

        fn of_local(self, local: Local) -> Self::Place {
            PlaceWithMetadata::from(DefaultPlaceBuilder::default().of_local(local))
        }

        fn project_on<'a>(self, place: &'a mut Self::Place) -> Self::ProjectionBuilder<'a> {
            BasicProjectionBuilder(place)
        }

        fn metadata(self, place: &mut Self::Place) -> Self::MetadataHandler<'_> {
            BasicPlaceMetadataHandler(place)
        }
    }

    pub(crate) struct BasicProjectionBuilder<'a>(&'a mut PlaceWithMetadata);

    impl PlaceProjectionBuilder for BasicProjectionBuilder<'_> {
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
            self.0.metadata_mut().set_size(byte_size);
        }
    }
}
pub(crate) use builders::BasicPlaceBuilder;

mod handlers {
    use common::type_info::{TagEncodingInfo, TagInfo};

    use crate::abs::{PlaceUsage, backend::PlaceHandler};

    use super::*;
    use backend::{BasicBackend, BasicPlaceInfo, CallStackInfo, TypeDatabase, VariablesState};

    pub(crate) struct BasicPlaceHandler<'a> {
        vars_state: &'a mut dyn VariablesState,
        usage: PlaceUsage,
        type_manager: &'a dyn TypeDatabase,
    }

    impl<'a> BasicPlaceHandler<'a> {
        pub fn new(usage: PlaceUsage, backend: &'a mut BasicBackend) -> BasicPlaceHandler<'a> {
            Self {
                vars_state: backend.call_stack_manager.top(),
                usage,
                type_manager: backend.type_manager.as_ref(),
            }
        }
    }

    impl PlaceHandler for BasicPlaceHandler<'_> {
        type PlaceInfo<'a> = BasicPlaceInfo;
        type Place = PlaceValueRef;
        type DiscriminablePlace = DiscriminantPossiblePlace;

        fn from_info<'a>(self, info: Self::PlaceInfo<'a>) -> Self::Place {
            self.vars_state.ref_place(&info, self.usage)
        }

        fn tag_of<'a>(self, info: Self::PlaceInfo<'a>) -> Self::DiscriminablePlace {
            let mut place = info;
            let type_manager: &dyn TypeDatabase = self.type_manager;
            let ty = type_manager.get_type(&place.metadata().unwrap_type_id());
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
                let tag_ty = type_manager.get_type(&tag_as_field.ty);
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
    }

    pub(crate) enum DiscriminantPossiblePlace {
        None,
        SingleVariant { discr_bit_rep: u128 },
        TagPlaceWithInfo(PlaceValueRef, &'static TagEncodingInfo),
    }
}
pub(crate) use handlers::{BasicPlaceHandler, DiscriminantPossiblePlace};
