pub use super::super::types::{
    Alignment, AssignmentId, BasicBlockIndex, BasicBlockLocation, CalleeDef, DefId, DynRawMetadata,
    FieldIndex, FuncDef, InstanceKindId, LocalIndex, RawAddress, SwitchCaseIndex, TypeId, TypeSize,
    VariantIndex,
};
pub use super::refs::{OperandRef, PlaceRef, Ref};

pub type Tag = &'static str;

macro_rules! self_const {
    ($($name:ident = $value:expr;)*) => {
        $(
            #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
            pub const $name: Self = Self($value);
        )*
    };
}

macro_rules! cases_ifs {
    ($raw_val:expr, $($name:ident),* $(,)?) => {
        if false {
            unreachable!()
        }
        $(
        else if $raw_val == Self::$name.to_raw() {
            Self::$name
        }
        )*
        else {
            unreachable!()
        }
    };
}

macro_rules! enum_like_type {
    ($name:ident [$t:ty] {
        $($variant:ident = $value:expr;)*
    }) => {
        #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name(pub $t);

        #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
        impl $name {
            self_const! {
                $($variant = $value;)*
            }

            #[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
            #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
            pub const fn from_raw(raw: $t) -> Self {
                cases_ifs!(raw, $($variant),*)
            }

            #[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
            #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
            #[inline]
            pub const fn to_raw(self) -> $t {
                self.0
            }
        }
    };
    ($name:ident {
        $($variant:ident = $value:expr;)*
    }) => {
        enum_like_type!($name[u8] { $($variant = $value;)* });
    }
}

enum_like_type! {
    BinaryOp {
        ADD = 1;
        ADD_UNCHECKED = BinaryOp::ADD.0 | BinaryOp::UNCHECKED;
        ADD_WITH_OVERFLOW = BinaryOp::ADD.0 | BinaryOp::OVERFLOW;
        ADD_SATURATING = BinaryOp::ADD.0 | BinaryOp::SATURATING;
        SUB = 2;
        SUB_UNCHECKED = BinaryOp::SUB.0 | BinaryOp::UNCHECKED;
        SUB_WITH_OVERFLOW = BinaryOp::SUB.0 | BinaryOp::OVERFLOW;
        SUB_SATURATING = BinaryOp::SUB.0 | BinaryOp::SATURATING;
        MUL = 3;
        MUL_UNCHECKED = BinaryOp::MUL.0 | BinaryOp::UNCHECKED;
        MUL_WITH_OVERFLOW = BinaryOp::MUL.0 | BinaryOp::OVERFLOW;
        DIV = 4;
        // It is almost the same thing as unchecked.
        DIV_EXACT = BinaryOp::DIV.0 | BinaryOp::UNCHECKED;
        REM = 5;
        BIT_XOR = 6;
        BIT_AND = 7;
        BIT_OR = 8;
        SHL = 9;
        SHL_UNCHECKED = BinaryOp::SHL.0 | BinaryOp::UNCHECKED;
        SHR = 10;
        SHR_UNCHECKED = BinaryOp::SHR.0 | BinaryOp::UNCHECKED;
        ROT_L = 11;
        ROT_R = 12;
        EQ = 21;
        LT = 22;
        LE = 23;
        NE = 24;
        GE = 25;
        GT = 26;
        CMP = 27;
        OFFSET = 31;
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
impl BinaryOp {
    const OVERFLOW: u8 = 0b01 << (core::mem::size_of::<u8>() * 8 - 2);
    const UNCHECKED: u8 = 0b10 << (core::mem::size_of::<u8>() * 8 - 2);
    const SATURATING: u8 = 0b11 << (core::mem::size_of::<u8>() * 8 - 2);
}

enum_like_type! {
    UnaryOp {
        NOT = 31;
        NEG = 32;
        PTR_METADATA = 33;
        BIT_REVERSE = 34;
        CTTZ_NONZERO = 35;
        CTTZ = 36;
        CTPOP = 37;
        CTLZ_NONZERO = 38;
        CTLZ = 39;
        BSWAP = 40;
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SwitchInfo {
    pub node_location: BasicBlockIndex,
    pub discriminant: OperandRef,
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
impl Default for SwitchInfo {
    #[inline(always)]
    fn default() -> Self {
        SwitchInfo {
            node_location: 0,
            discriminant: 0,
        }
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AssertionInfo {
    pub location: BasicBlockIndex,
    pub condition: OperandRef,
    pub expected: bool,
}

enum_like_type! {
    AtomicOrdering {
        UNORDERED = 0;
        RELAXED = 1;
        RELEASE = 2;
        ACQUIRE = 3;
        ACQ_REL = 4;
        SEQ_CST = 5;
    }
}

enum_like_type! {
    AtomicBinaryOp {
        ADD = 1;
        SUB = 2;

        XOR = 6;
        AND = 7;
        NAND = 13;
        OR = 8;

        MIN = 40;
        MAX = 41;
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub type DebugInfo = &'static [u8];

enum_like_type! {
    PrimitiveType [i8] {
        U8 = 1;
        U16 = 2;
        U32 = 4;
        U64 = 8;
        U128 = 16;

        I8 = -1;
        I16 = -2;
        I32 = -4;
        I64 = -8;
        I128 = -16;

        F16 = 5;
        F32 = 9;
        F64 = 17;
        F128 = 33;

        BOOL = -3;

        CHAR = -5;
    }
}
