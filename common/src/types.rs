use core::{num::NonZero, ptr::NonNull};

pub type LocalIndex = u32;
pub type BasicBlockIndex = u32;
pub type VariantIndex = u32;
pub type FieldIndex = u32;

pub type RawAddress = *const ();
pub type PointerOffset = u64;
pub type TypeSize = PointerOffset;
pub type Alignment = TypeSize;

pub type TypeId = NonZero<u128>;

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(C)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize))]
#[cfg_attr(feature = "rkyv",rkyv(
    // Derives can be passed through to the generated type:
    derive(Debug, Hash, PartialEq, Eq),
    compare(PartialEq),
))]
pub struct DefId(pub u32, pub u32);
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
impl core::fmt::Display for DefId {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "Def({}:{})", self.0, self.1)
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(C)]
pub struct BasicBlockLocation {
    pub body: DefId,
    pub index: BasicBlockIndex,
}
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
impl core::fmt::Display for BasicBlockLocation {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}[{}]", self.body, self.index)
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct DynRawMetadata(NonNull<RawAddress>);
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
impl core::fmt::Pointer for DynRawMetadata {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{:p}", self.0)
    }
}

// FIXME: Possibly large data structure.
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CalleeDef {
    pub static_addr: RawAddress,
    pub as_virtual: Option<(DynRawMetadata, u64)>,
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FuncDef {
    pub static_addr: RawAddress,
    pub as_dyn_method: Option<(DynRawMetadata, u64)>,
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg(feature = "trace_types")]
pub mod trace {
    use std::{vec, vec::Vec};

    #[derive(Debug, Clone)]
    #[cfg_attr(feature = "trace_types", derive(serde::Serialize, serde::Deserialize))]
    pub struct Constraint<V, C> {
        pub discr: V,
        pub kind: ConstraintKind<C>,
    }

    impl<V, C> Constraint<V, C> {
        pub fn equality(discr: V, value: C) -> Self {
            Self {
                discr,
                kind: ConstraintKind::OneOf(vec![value]),
            }
        }

        #[inline]
        pub fn not(self) -> Self {
            Self {
                kind: self.kind.not(),
                ..self
            }
        }

        pub fn map<VTo, CTo>(
            self,
            mut f: impl FnMut(V) -> VTo,
            g: impl FnMut(C) -> CTo,
        ) -> Constraint<VTo, CTo> {
            Constraint {
                discr: f(self.discr),
                kind: self.kind.map(g),
            }
        }

        pub fn as_ref(&self) -> Constraint<&V, &C> {
            Constraint {
                discr: &self.discr,
                kind: self.kind.as_ref(),
            }
        }
    }

    impl<V, C> Constraint<&V, &C> {
        pub fn cloned(self) -> Constraint<V, C>
        where
            V: Clone,
            C: Clone,
        {
            self.map(Clone::clone, Clone::clone)
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    #[cfg_attr(feature = "trace_types", derive(serde::Serialize, serde::Deserialize))]
    pub enum ConstraintKind<C> {
        True,
        False,
        // FIXME: Good candidates for stack-based vectors
        OneOf(Vec<C>),
        NoneOf(Vec<C>),
    }

    impl<C> ConstraintKind<C> {
        #[inline]
        pub fn not(self) -> Self {
            use ConstraintKind::*;
            match self {
                True => False,
                False => True,
                OneOf(matches) => NoneOf(matches),
                NoneOf(options) => OneOf(options),
            }
        }

        #[inline]
        pub fn is_boolean(&self) -> bool {
            match self {
                ConstraintKind::True | ConstraintKind::False => true,
                _ => false,
            }
        }

        pub fn map<CTo>(self, f: impl FnMut(C) -> CTo) -> ConstraintKind<CTo> {
            use ConstraintKind::*;
            match self {
                True => True,
                False => False,
                OneOf(options) => OneOf(options.into_iter().map(f).collect()),
                NoneOf(options) => NoneOf(options.into_iter().map(f).collect()),
            }
        }

        pub fn as_ref(&self) -> ConstraintKind<&C> {
            use ConstraintKind::*;
            match self {
                True => True,
                False => False,
                OneOf(options) => OneOf(options.iter().collect()),
                NoneOf(options) => NoneOf(options.iter().collect()),
            }
        }
    }

    mod fmt {
        use core::fmt::{Display, Formatter, Result};

        use super::super::super::utils::comma_separated;

        use super::*;

        impl<V, C> Display for Constraint<V, C>
        where
            V: Display,
            C: Display,
        {
            fn fmt(&self, f: &mut Formatter) -> Result {
                use ConstraintKind::*;
                match &self.kind {
                    True => write!(f, "{{{}}}", self.discr),
                    False => write!(f, "!{{{}}}", self.discr),
                    _ => write!(f, "{{{} {}}}", self.discr, self.kind),
                }
            }
        }

        impl<C> Display for ConstraintKind<C>
        where
            C: Display,
        {
            fn fmt(&self, f: &mut Formatter) -> Result {
                use ConstraintKind::*;
                match self {
                    True => write!(f, "{}", true),
                    False => write!(f, "{}", false),
                    OneOf(options) => {
                        if options.len() == 1 {
                            write!(f, "== {}", options[0])
                        } else {
                            write!(f, "in {}", comma_separated(options.iter()))
                        }
                    }
                    NoneOf(options) => {
                        if options.len() == 1 {
                            write!(f, "!= {}", options[0])
                        } else {
                            write!(f, "!in {}", comma_separated(options.iter()))
                        }
                    }
                }
            }
        }
    }
}
