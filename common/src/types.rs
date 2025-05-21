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

pub type AssignmentId = u16;

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

// FIXME: Possibly large data structure.
// FIXME: Merge virtual identifier with def_id
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FuncDef {
    pub static_addr: RawAddress,
    pub as_dyn_method: Option<(DynRawMetadata, u64)>,
    pub def_id: DefId,
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg(feature = "trace_types")]
pub mod trace {
    use std::{vec, vec::Vec};

    use super::{BasicBlockLocation, DefId};

    #[derive(Debug, Clone)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

        /// # Returns
        /// A stronger constraint that satisfies both, or `None` if unsatisfiable (empty set).
        pub fn and(&self, other: &Self, universe_size: impl Fn() -> usize) -> Option<Self>
        where
            C: PartialEq + Clone,
        {
            use ConstraintKind::*;
            match (self, other) {
                (True, True) => Some(True),
                (False, False) => Some(False),
                (True, False) | (False, True) => None,
                (OneOf(cases), OneOf(other_cases)) => {
                    let intersection: Vec<C> = cases
                        .into_iter()
                        .filter(|c| other_cases.contains(&c))
                        .cloned()
                        .collect();
                    (!intersection.is_empty()).then_some(OneOf(intersection))
                }
                (NoneOf(non_cases), NoneOf(other_non_cases)) => {
                    let union: Vec<C> = non_cases
                        .into_iter()
                        .chain(
                            other_non_cases
                                .into_iter()
                                .filter(|c| !non_cases.contains(c)),
                        )
                        .cloned()
                        .collect();
                    (union.len() < universe_size()).then_some(NoneOf(union))
                }
                (OneOf(cases), NoneOf(non_cases)) | (NoneOf(non_cases), OneOf(cases)) => {
                    let diff: Vec<C> = cases
                        .into_iter()
                        .filter(|c| !non_cases.contains(&c))
                        .cloned()
                        .collect();
                    (!diff.is_empty()).then_some(OneOf(diff))
                }
                (True | False, OneOf(..) | NoneOf(..)) | (OneOf(..) | NoneOf(..), True | False) => {
                    panic!("Constraint types must be the same")
                }
            }
        }

        /// # Returns
        /// A weaker constraint that is satisfied by both, or `None` if always satisfiable (universal set).
        pub fn or(&self, other: &Self, universe_size: impl Fn() -> usize) -> Option<Self>
        where
            C: PartialEq + Clone,
        {
            // De Morgan's
            ConstraintKind::and(&self.as_ref().not(), &other.as_ref().not(), universe_size)
                .map(|c| c.not())
                .map(|c| c.map(Clone::clone))
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

    #[derive(Debug)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub enum ExeTraceRecord<C> {
        Call {
            from: BasicBlockLocation,
            to: DefId,
            broken: bool,
        },
        Return {
            from: BasicBlockLocation,
            to: DefId,
            broken: bool,
        },
        Branch {
            location: BasicBlockLocation,
            kind: ConstraintKind<C>,
        },
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
                    OneOf(cases) => match cases.as_slice() {
                        [single_case] => write!(f, "== {}", single_case),
                        _ => write!(f, "in {}", comma_separated(cases.iter())),
                    },
                    NoneOf(cases) => match cases.as_slice() {
                        [single_case] => write!(f, "!= {}", single_case),
                        _ => write!(f, "!in {}", comma_separated(cases.iter())),
                    },
                }
            }
        }

        impl<C> Display for ExeTraceRecord<C>
        where
            C: Display,
        {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                match self {
                    ExeTraceRecord::Call { from, to, broken } => write!(
                        f,
                        "{from} {sym}⤞ {to}",
                        sym = if *broken { "~" } else { "" }
                    ),
                    ExeTraceRecord::Return { from, to, broken } => write!(
                        f,
                        "{to} ⤝{sym} {from}",
                        sym = if *broken { "~" } else { "" }
                    ),
                    ExeTraceRecord::Branch { location, kind } => write!(f, "{location}: {kind}"),
                }
            }
        }
    }
}
