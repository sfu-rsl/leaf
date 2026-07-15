use core::ptr::NonNull;

use derive_more as dm;

use common::{log_error, log_warn};

use super::{FieldIndex, LocalIndex, RawAddress, TypeId, TypeSize, ValueType, VariantIndex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Local {
    ReturnValue,          // 0
    Argument(LocalIndex), // 1-n
    Normal(LocalIndex),   // > n
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Place<B, P = Projection<B>> {
    base: B,
    projections: Vec<P>,
}

impl<B, P> Place<B, P> {
    pub fn new(base: B) -> Self {
        Self {
            base,
            /* As most of the places are just locals, we try not to allocate at start. */
            projections: Vec::with_capacity(0),
        }
    }

    #[inline]
    pub fn base(&self) -> &B {
        &self.base
    }

    #[inline]
    pub fn base_mut(&mut self) -> &mut B {
        &mut self.base
    }

    #[inline]
    pub fn has_projection(&self) -> bool {
        !self.projections.is_empty()
    }

    #[inline]
    pub fn projections(&self) -> &[P] {
        &self.projections
    }

    #[inline]
    pub fn add_projection(&mut self, projection: P) {
        self.projections.push(projection);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Projection<I = Local> {
    Field(FieldIndex),
    Deref,
    Index(I),
    ConstantIndex {
        offset: u64,
        min_length: u64,
        from_end: bool,
    },
    Subslice {
        from: u64,
        to: u64,
        from_end: bool,
    },
    Downcast(VariantIndex),
    // These may be removed as they are not expected to have effects/appear at runtime.
    OpaqueCast,
    UnwrapUnsafeBinder,
}

impl<I> Projection<I> {
    pub(crate) fn map<IInto>(self, f: impl FnOnce(I) -> IInto) -> Projection<IInto> {
        use Projection::*;
        match self {
            Field(index) => Field(index),
            Deref => Deref,
            Index(index) => Index(f(index)),
            ConstantIndex {
                offset,
                min_length,
                from_end,
            } => ConstantIndex {
                offset,
                min_length,
                from_end,
            },
            Subslice { from, to, from_end } => Subslice { from, to, from_end },
            Downcast(index) => Downcast(index),
            OpaqueCast => OpaqueCast,
            UnwrapUnsafeBinder => UnwrapUnsafeBinder,
        }
    }
}

pub(crate) trait HasMetadata {
    type Metadata;

    fn metadata(&self) -> &Self::Metadata;

    fn metadata_mut(&mut self) -> &mut Self::Metadata;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, dm::Deref, dm::DerefMut, dm::From)]
pub(crate) struct LocalWithMetadata<M = DefaultPlaceMetadata> {
    #[deref]
    #[deref_mut]
    pub local: Local,
    metadata: M,
}

impl<M> HasMetadata for LocalWithMetadata<M> {
    type Metadata = M;

    fn metadata(&self) -> &Self::Metadata {
        &self.metadata
    }

    fn metadata_mut(&mut self) -> &mut Self::Metadata {
        &mut self.metadata
    }
}

/* NOTE: Why not the following alternative structure?
   struct PlaceWithAddress {
       pub place: Place,
       pub addresses: Vec<RawPointer>,
   }

   While this structure is more intuitive and more compatible with the original
   `Place` structure, it causes problems with index projection where the index
   place should be backed by an address as well.
*/

#[derive(Debug, Clone, dm::Deref, dm::DerefMut)]
pub(crate) struct GenericPlaceWithMetadata<B, P, M> {
    #[deref]
    #[deref_mut]
    place: Place<B, P>,
    projs_metadata: Vec<M>,
}

impl<B, P> HasMetadata for GenericPlaceWithMetadata<B, P, B::Metadata>
where
    B: HasMetadata,
{
    type Metadata = B::Metadata;

    #[inline]
    fn metadata(&self) -> &Self::Metadata {
        if self.has_projection() {
            debug_assert_eq!(self.projs_metadata.len(), self.projections().len());
            &self.projs_metadata.last().unwrap()
        } else {
            self.place.base().metadata()
        }
    }

    #[inline]
    fn metadata_mut(&mut self) -> &mut Self::Metadata {
        if self.has_projection() {
            debug_assert_eq!(self.projs_metadata.len(), self.projections().len());
            self.projs_metadata.last_mut().unwrap()
        } else {
            self.place.base_mut().metadata_mut()
        }
    }
}

impl<B, P, M> GenericPlaceWithMetadata<B, P, M> {
    pub(crate) fn push_metadata(&mut self, metadata: M) {
        self.projs_metadata.push(metadata);
    }

    pub(crate) fn projs_metadata(&self) -> impl Iterator<Item = &M> + '_ {
        self.projs_metadata.iter()
    }
}

pub(crate) type PlaceWithMetadata<P, M = DefaultPlaceMetadata> =
    GenericPlaceWithMetadata<LocalWithMetadata<M>, P, M>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DefaultPlaceMetadata {
    address: Option<NonNull<()>>,
    type_id: Option<TypeId>,
    // Fast-accessible type information
    ty: Option<ValueType>,
    size: Option<TypeSize>,
}

impl Default for DefaultPlaceMetadata {
    fn default() -> Self {
        Self {
            address: None,
            type_id: None,
            ty: None,
            size: None,
        }
    }
}

impl DefaultPlaceMetadata {
    #[inline]
    pub(crate) fn address(&self) -> RawAddress {
        self.address
            .unwrap_or_else(|| {
                log_error!("Presumably an unchecked null pointer dereference is happening in the program. Runtime will terminate the execution.");
                panic!("Address of place is not available.")
            })
            .as_ptr()
    }

    #[inline]
    pub(crate) fn set_address(&mut self, address: RawAddress) {
        debug_assert!(self.address.is_none());
        if cfg!(debug_assertions) && address.is_null() {
            log_warn!("Setting null address to place metadata. {:?}", self);
        }
        self.address = NonNull::new(address as *mut ());
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
        debug_assert!(self.type_id.is_none());
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

#[derive(Clone, Copy, Debug)]
pub(crate) enum PlaceUsage {
    Copy,
    Move,
    Write,
    Ref,
    Drop,
    Mark,
}

mod conversions {
    use super::*;

    impl<B, P> From<B> for Place<B, P> {
        fn from(value: B) -> Self {
            Self::new(value)
        }
    }

    // Cannot generalize to B
    impl<P> TryFrom<Place<Local, P>> for Local {
        type Error = Place<Local, P>;

        fn try_from(value: Place<Local, P>) -> Result<Self, Self::Error> {
            if !value.has_projection() {
                Ok(value.base)
            } else {
                Err(value)
            }
        }
    }

    impl<M> From<Local> for LocalWithMetadata<M>
    where
        M: Default,
    {
        fn from(value: Local) -> Self {
            Self {
                local: value,
                metadata: Default::default(),
            }
        }
    }

    impl<B, P, M> From<B> for GenericPlaceWithMetadata<B, P, M> {
        fn from(value: B) -> Self {
            Self::from(Place::from(value))
        }
    }

    impl<B, P, M> From<Place<B, P>> for GenericPlaceWithMetadata<B, P, M> {
        fn from(value: Place<B, P>) -> Self {
            Self {
                place: value,
                projs_metadata: Vec::with_capacity(0),
            }
        }
    }

    impl<P, M: Default> From<Local> for PlaceWithMetadata<P, M> {
        fn from(value: Local) -> Self {
            Self::from(LocalWithMetadata::from(value))
        }
    }

    impl<P, M> TryFrom<PlaceWithMetadata<P, M>> for LocalWithMetadata<M>
    where
        LocalWithMetadata<M>: Clone,
    {
        type Error = PlaceWithMetadata<P, M>;

        fn try_from(place: PlaceWithMetadata<P, M>) -> Result<Self, Self::Error> {
            if !place.has_projection() {
                Ok(place.base().clone())
            } else {
                Err(place)
            }
        }
    }
}
