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
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(C)]
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
