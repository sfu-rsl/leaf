use super::alias::backend;
use backend::MdSanPlaceValue;

mod data_types {
    use super::*;

    pub(crate) type Projection = crate::abs::Projection<MdSanPlaceValue>;

    pub(crate) type LocalWithMetadata = crate::abs::LocalWithMetadata;
    pub(crate) type PlaceWithMetadata = crate::abs::PlaceWithMetadata<Projection>;
}
pub(crate) use data_types::*;

pub(crate) type MdSanPlaceBuilder = crate::pri::fluent::backend::shared::DefaultPlaceBuilder<
    LocalWithMetadata,
    MdSanPlaceValue,
    Projection,
>;

mod handlers {
    use crate::{
        abs::PlaceUsage, backends::mdsan::MdMemoryState, pri::fluent::backend::PlaceHandler,
    };

    use super::*;
    use backend::{MdSanBackend, MdSanPlaceInfo, MdSanVariablesState};

    pub(crate) struct MdSanPlaceHandler<'a> {
        vars_state: &'a mut MdSanVariablesState,
        usage: PlaceUsage,
    }

    impl<'a> MdSanPlaceHandler<'a> {
        pub fn new(usage: PlaceUsage, backend: &'a mut MdSanBackend) -> MdSanPlaceHandler<'a> {
            Self {
                vars_state: &mut backend.vars_state,
                usage,
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
