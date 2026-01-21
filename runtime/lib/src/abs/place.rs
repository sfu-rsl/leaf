use super::{FieldIndex, LocalIndex, VariantIndex};

use derive_more as dm;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Local {
    ReturnValue,          // 0
    Argument(LocalIndex), // 1-n
    Normal(LocalIndex),   // > n
}

impl Local {
    #[inline(always)]
    pub(crate) fn is_func_local(&self) -> bool {
        matches!(self, Local::ReturnValue | Local::Argument(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Place<L = Local, P = Projection<L>> {
    local: L,
    projections: Vec<P>,
}

impl<L, P> Place<L, P> {
    pub fn new(local: L) -> Self {
        Self {
            local,
            /* As most of the places are just locals, we try not to allocate at start. */
            projections: Vec::with_capacity(0),
        }
    }

    #[inline]
    pub fn local(&self) -> &L {
        &self.local
    }

    #[inline]
    pub fn local_mut(&mut self) -> &mut L {
        &mut self.local
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

    #[inline]
    pub fn with_projection(mut self, projection: P) -> Self {
        self.add_projection(projection);
        self
    }
}

impl<L, P> From<L> for Place<L, P> {
    fn from(value: L) -> Self {
        Self::new(value)
    }
}

impl<P> TryFrom<Place<Local, P>> for Local {
    type Error = Place<Local, P>;

    fn try_from(value: Place<Local, P>) -> Result<Self, Self::Error> {
        if !value.has_projection() {
            Ok(value.local)
        } else {
            Err(value)
        }
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
pub(crate) struct LocalWithMetadata<M> {
    pub local: Local,
    #[deref]
    #[deref_mut]
    metadata: M,
}

impl<M> LocalWithMetadata<M> {
    pub(crate) fn new(local: Local, metadata: M) -> Self {
        Self { local, metadata }
    }
}

impl<M> AsRef<Local> for LocalWithMetadata<M> {
    fn as_ref(&self) -> &Local {
        &self.local
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
pub(crate) struct PlaceWithMetadata<L, P, M> {
    #[deref]
    #[deref_mut]
    place: Place<L, P>,
    projs_metadata: Vec<M>,
}

impl<L, P> HasMetadata for PlaceWithMetadata<L, P, L::Metadata>
where
    L: HasMetadata,
{
    type Metadata = L::Metadata;

    fn metadata(&self) -> &Self::Metadata {
        if self.has_projection() {
            debug_assert_eq!(self.projs_metadata.len(), self.projections().len());
            &self.projs_metadata.last().unwrap()
        } else {
            self.place.local().metadata()
        }
    }

    fn metadata_mut(&mut self) -> &mut Self::Metadata {
        if self.has_projection() {
            debug_assert_eq!(self.projs_metadata.len(), self.projections().len());
            self.projs_metadata.last_mut().unwrap()
        } else {
            self.place.local_mut().metadata_mut()
        }
    }
}

impl<L, P, M> PlaceWithMetadata<L, P, M> {
    pub(crate) fn push_metadata(&mut self, metadata: M) {
        self.projs_metadata.push(metadata);
    }

    pub(crate) fn projs_metadata(&self) -> impl Iterator<Item = &M> + '_ {
        self.projs_metadata.iter()
    }

    pub(crate) fn projs_metadata_mut(&mut self) -> impl Iterator<Item = &mut M> + '_ {
        self.projs_metadata.iter_mut()
    }
}

impl<L, P, M> From<L> for PlaceWithMetadata<L, P, M> {
    fn from(value: L) -> Self {
        Self::from(Place::from(value))
    }
}

impl<L, P, M> From<Place<L, P>> for PlaceWithMetadata<L, P, M> {
    fn from(value: Place<L, P>) -> Self {
        Self {
            place: value,
            projs_metadata: Vec::with_capacity(0),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PlaceUsage {
    Read,
    Write,
    Ref,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PlaceAsOperandUsage {
    Copy,
    Move,
}
