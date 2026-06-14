use super::alias::backend;
use backend::{PlaceValueRef, expr::place::DeterPlaceValueRef};

mod data_types {
    use super::*;

    /* Index's place is always deterministic. */
    pub(crate) type Projection = crate::abs::Projection<DeterPlaceValueRef>;

    pub(crate) type PlaceMetadata = crate::abs::place::DefaultPlaceMetadata;
    pub(crate) type LocalWithMetadata = crate::abs::LocalWithMetadata;
    pub(crate) type PlaceWithMetadata = crate::abs::PlaceWithMetadata<Projection>;
}
pub(crate) use data_types::*;

mod builders {
    use crate::pri::fluent::backend::shared::DefaultPlaceBuilder;

    use super::*;

    pub(crate) type SymExPlaceBuilder =
        DefaultPlaceBuilder<LocalWithMetadata, PlaceValueRef, Projection>;

    impl From<crate::abs::Projection<PlaceValueRef>> for Projection {
        #[inline]
        fn from(value: crate::abs::place::Projection<PlaceValueRef>) -> Self {
            value.map(DeterPlaceValueRef::new)
        }
    }
}
pub(crate) use builders::SymExPlaceBuilder;

mod handlers {
    use common::type_info::{TagEncodingInfo, TagInfo};

    use crate::{
        abs::{PlaceUsage, place::HasMetadata},
        pri::fluent::backend::PlaceHandler,
    };

    use super::*;
    use backend::{SymExBackend, SymExPlaceInfo, TypeDatabase, VariablesState};

    pub(crate) struct SymExPlaceHandler<'a> {
        vars_state: &'a mut dyn VariablesState,
        usage: PlaceUsage,
        type_manager: &'a dyn TypeDatabase,
    }

    impl<'a> SymExPlaceHandler<'a> {
        pub fn new(usage: PlaceUsage, backend: &'a mut SymExBackend) -> SymExPlaceHandler<'a> {
            Self {
                vars_state: &mut backend.vars_state,
                usage,
                type_manager: backend.type_manager.as_ref(),
            }
        }
    }

    impl PlaceHandler for SymExPlaceHandler<'_> {
        type PlaceInfo<'a> = SymExPlaceInfo;
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
                        .metadata()
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
pub(crate) use handlers::{DiscriminantPossiblePlace, SymExPlaceHandler};
