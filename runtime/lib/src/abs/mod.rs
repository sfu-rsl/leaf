use core::{num::NonZeroU64, panic, ptr::NonNull};

use derive_more as dm;

pub(crate) mod backend;
pub(crate) mod fmt;
pub(crate) mod place;
mod serdes;
pub(crate) mod utils;

pub(crate) use common::{
    pri::Tag,
    types::{trace::*, *},
};

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum BinaryOp {
    Add = common::pri::BinaryOp::ADD.to_raw(),
    AddUnchecked = common::pri::BinaryOp::ADD_UNCHECKED.to_raw(),
    AddWithOverflow = common::pri::BinaryOp::ADD_WITH_OVERFLOW.to_raw(),
    AddSaturating = common::pri::BinaryOp::ADD_SATURATING.to_raw(),
    Sub = common::pri::BinaryOp::SUB.to_raw(),
    SubUnchecked = common::pri::BinaryOp::SUB_UNCHECKED.to_raw(),
    SubWithOverflow = common::pri::BinaryOp::SUB_WITH_OVERFLOW.to_raw(),
    SubSaturating = common::pri::BinaryOp::SUB_SATURATING.to_raw(),
    Mul = common::pri::BinaryOp::MUL.to_raw(),
    MulUnchecked = common::pri::BinaryOp::MUL_UNCHECKED.to_raw(),
    MulWithOverflow = common::pri::BinaryOp::MUL_WITH_OVERFLOW.to_raw(),
    Div = common::pri::BinaryOp::DIV.to_raw(),
    DivExact = common::pri::BinaryOp::DIV_EXACT.to_raw(),
    Rem = common::pri::BinaryOp::REM.to_raw(),

    BitXor = common::pri::BinaryOp::BIT_XOR.to_raw(),
    BitAnd = common::pri::BinaryOp::BIT_AND.to_raw(),
    BitOr = common::pri::BinaryOp::BIT_OR.to_raw(),
    Shl = common::pri::BinaryOp::SHL.to_raw(),
    ShlUnchecked = common::pri::BinaryOp::SHL_UNCHECKED.to_raw(),
    Shr = common::pri::BinaryOp::SHR.to_raw(),
    ShrUnchecked = common::pri::BinaryOp::SHR_UNCHECKED.to_raw(),
    RotateL = common::pri::BinaryOp::ROT_L.to_raw(),
    RotateR = common::pri::BinaryOp::ROT_R.to_raw(),
    CarrylessMul = common::pri::BinaryOp::CARRYLESS_MUL.to_raw(),

    Eq = common::pri::BinaryOp::EQ.to_raw(),
    Lt = common::pri::BinaryOp::LT.to_raw(),
    Le = common::pri::BinaryOp::LE.to_raw(),
    Ne = common::pri::BinaryOp::NE.to_raw(),
    Ge = common::pri::BinaryOp::GE.to_raw(),
    Gt = common::pri::BinaryOp::GT.to_raw(),
    Cmp = common::pri::BinaryOp::CMP.to_raw(),

    Offset = common::pri::BinaryOp::OFFSET.to_raw(),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum UnaryOp {
    Not = common::pri::UnaryOp::NOT.to_raw(),
    Neg = common::pri::UnaryOp::NEG.to_raw(),
    PtrMetadata = common::pri::UnaryOp::PTR_METADATA.to_raw(),
    BitReverse = common::pri::UnaryOp::BIT_REVERSE.to_raw(),
    NonZeroTrailingZeros = common::pri::UnaryOp::CTTZ_NONZERO.to_raw(),
    TrailingZeros = common::pri::UnaryOp::CTTZ.to_raw(),
    CountOnes = common::pri::UnaryOp::CTPOP.to_raw(),
    NonZeroLeadingZeros = common::pri::UnaryOp::CTLZ_NONZERO.to_raw(),
    LeadingZeros = common::pri::UnaryOp::CTLZ.to_raw(),
    ByteSwap = common::pri::UnaryOp::BSWAP.to_raw(),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TernaryOp {
    IfThenElse = 1,
    FunnelShl = 2,
    FunnelShr = 3,
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum AtomicOrdering {
    Unordered = common::pri::AtomicOrdering::UNORDERED.to_raw(),
    Relaxed = common::pri::AtomicOrdering::RELAXED.to_raw(),
    Acquire = common::pri::AtomicOrdering::ACQUIRE.to_raw(),
    Release = common::pri::AtomicOrdering::RELEASE.to_raw(),
    AcquireRelease = common::pri::AtomicOrdering::ACQ_REL.to_raw(),
    SequentiallyConsistent = common::pri::AtomicOrdering::SEQ_CST.to_raw(),
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum AtomicBinaryOp {
    Add = common::pri::AtomicBinaryOp::ADD.to_raw(),
    Sub = common::pri::AtomicBinaryOp::SUB.to_raw(),

    Xor = common::pri::AtomicBinaryOp::XOR.to_raw(),
    And = common::pri::AtomicBinaryOp::AND.to_raw(),
    Nand = common::pri::AtomicBinaryOp::NAND.to_raw(),
    Or = common::pri::AtomicBinaryOp::OR.to_raw(),

    Min = common::pri::AtomicBinaryOp::MIN.to_raw(),
    Max = common::pri::AtomicBinaryOp::MAX.to_raw(),
}

pub(crate) use place::{Local, LocalWithMetadata, PlaceUsage, PlaceWithMetadata};

pub(crate) type Place<L = Local, P = Projection<L>> = place::Place<L, P>;
pub(crate) type Projection<L> = place::Projection<L>;

#[derive(Debug, dm::From)]
pub(crate) enum Constant {
    Bool(bool),
    Char(char),
    Int {
        bit_rep: u128,
        ty: IntType,
    },
    Float {
        bit_rep: u128,
        ty: FloatType,
    },
    Str(&'static str),
    ByteStr(&'static [u8]),
    Addr(RawAddress),
    Zst,
    /// Constant of some type that is not modeled by instrumentation but exists in MIR.
    Some,
}

pub(crate) struct SymVariable<C> {
    pub(crate) ty: ValueType,
    pub(crate) conc_value: Option<C>,
}

#[derive(Debug)]
pub enum AssertKind<O> {
    BoundsCheck { len: O, index: O },
    Overflow(BinaryOp, O, O),
    OverflowNeg(O),
    DivisionByZero(O),
    RemainderByZero(O),
    ResumedAfterReturn(O), // NOTE: TODO: check if these exist in HIR only
    ResumedAfterPanic(O),  // NOTE: TODO: check if these exist in HIR only
    MisalignedPointerDereference { required: O, found: O },
    NullPointerDereference,
    InvalidEnumConstruction(O),
}

#[derive(Clone, Copy, Debug)]
#[repr(i8)]
pub enum PrimitiveType {
    U8 = common::pri::PrimitiveType::U8.to_raw(),
    U16 = common::pri::PrimitiveType::U16.to_raw(),
    U32 = common::pri::PrimitiveType::U32.to_raw(),
    U64 = common::pri::PrimitiveType::U64.to_raw(),
    U128 = common::pri::PrimitiveType::U128.to_raw(),

    I8 = common::pri::PrimitiveType::I8.to_raw(),
    I16 = common::pri::PrimitiveType::I16.to_raw(),
    I32 = common::pri::PrimitiveType::I32.to_raw(),
    I64 = common::pri::PrimitiveType::I64.to_raw(),
    I128 = common::pri::PrimitiveType::I128.to_raw(),

    F32 = common::pri::PrimitiveType::F32.to_raw(),
    F64 = common::pri::PrimitiveType::F64.to_raw(),

    Bool = common::pri::PrimitiveType::BOOL.to_raw(),

    Char = common::pri::PrimitiveType::CHAR.to_raw(),
}

impl PrimitiveType {
    pub const fn bit_size(&self) -> NonZeroU64 {
        NonZeroU64::new(self.byte_size().get() * 8).unwrap()
    }

    pub const fn byte_size(&self) -> NonZeroU64 {
        use PrimitiveType::*;
        let byte_size = match self {
            U8 | I8 => 1,
            U16 | I16 => 2,
            U32 | I32 => 4,
            U64 | I64 => 8,
            U128 | I128 => 16,
            F32 => 4,
            F64 => 8,
            Bool => size_of::<bool>(),
            Char => size_of::<char>(),
        };
        NonZeroU64::new(byte_size as u64).unwrap()
    }
}

// FIXME: Replace with PrimitiveType
#[derive(Clone, Copy, Debug, PartialEq, Eq, derive_more::From)]
pub(crate) enum ValueType {
    Bool,
    Char,
    Int(IntType),
    Float(FloatType),
}

impl ValueType {
    pub(crate) fn new_int(bit_size: u64, is_signed: bool) -> Self {
        Self::Int(IntType {
            bit_size,
            is_signed,
        })
    }

    pub(crate) fn new_float(e_bits: u64, s_bits: u64) -> Self {
        Self::Float(FloatType { e_bits, s_bits })
    }

    /// Returns whether the type is signed if it.
    /// Remarks: Returns false for bool and char.
    pub(crate) fn is_signed(&self) -> bool {
        match self {
            Self::Bool | Self::Char => false,
            Self::Int(IntType { is_signed, .. }) => *is_signed,
            Self::Float(_) => true,
        }
    }

    pub(crate) fn size(&self) -> NonZeroU64 {
        use core::mem::size_of;
        let size = match self {
            ValueType::Bool => size_of::<bool>() as u64,
            ValueType::Char => size_of::<char>() as u64,
            ValueType::Int(IntType { bit_size, .. }) => bit_size / 8,
            Self::Float(FloatType { e_bits, s_bits }) => (e_bits + s_bits) / 8,
        };
        NonZeroU64::new(size as u64).unwrap()
    }

    pub(crate) fn bit_size(&self) -> Option<NonZeroU64> {
        match self {
            Self::Bool => None,
            Self::Char => NonZeroU64::new((core::mem::size_of::<char>() * 8) as u64),
            Self::Int(IntType { bit_size, .. }) => NonZeroU64::new(*bit_size),
            Self::Float(FloatType { e_bits, s_bits }) => NonZeroU64::new(e_bits + s_bits),
        }
    }

    #[inline]
    pub(crate) fn as_int(&self) -> Option<&IntType> {
        match self {
            Self::Int(ref int_type) => Some(int_type),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn expect_int(self) -> IntType {
        match self {
            Self::Int(int_type) => int_type,
            _ => panic!("Expected an IntType, found: {:?}", self),
        }
    }
}

impl From<PrimitiveType> for ValueType {
    fn from(value: PrimitiveType) -> Self {
        use PrimitiveType::*;
        match value {
            U8 | U16 | U32 | U64 | U128 | I8 | I16 | I32 | I64 | I128 => ValueType::Int(IntType {
                bit_size: value.bit_size().get(),
                is_signed: matches!(value, I8 | I16 | I32 | I64 | I128),
            }),
            F32 | F64 => ValueType::Float(FloatType {
                e_bits: match value {
                    F32 => size_of::<f32>() as u64 * 8 - f32::MANTISSA_DIGITS as u64,
                    F64 => size_of::<f64>() as u64 * 8 - f64::MANTISSA_DIGITS as u64,
                    _ => unreachable!(),
                },
                s_bits: match value {
                    F32 => f32::MANTISSA_DIGITS as u64,
                    F64 => f64::MANTISSA_DIGITS as u64,
                    _ => unreachable!(),
                },
            }),
            PrimitiveType::Bool => ValueType::Bool,
            PrimitiveType::Char => ValueType::Char,
        }
    }
}

/* FIXME: These types will have a limited set of possible values. Thus they can be
 * optimized using techniques such as interning or even changing them to enums.
 * Enums are not much a favorable option, since they are against the abstract
 * representation of integers and floats in the engine.
 */
#[derive(Clone, Copy, Eq, PartialEq, Hash, dm::Debug)]
#[debug("{}", self)]
pub(crate) struct IntType {
    pub bit_size: u64,
    pub is_signed: bool,
}

impl IntType {
    pub(crate) const USIZE: Self = Self {
        bit_size: std::mem::size_of::<usize>() as u64 * 8,
        is_signed: false,
    };
    pub(crate) const U32: Self = Self {
        bit_size: 32,
        is_signed: false,
    };
    pub(crate) const U8: Self = Self {
        bit_size: 8,
        is_signed: false,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) struct FloatType {
    pub e_bits: u64,
    pub s_bits: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum CastKind<I = IntType, F = FloatType, P = TypeId, T = TypeId> {
    ToChar,
    ToInt(I),
    ToFloat(F),
    ToPointer(P),
    PointerUnsize,
    ExposeProvenance,
    Transmute(T),
    Subtype(T),
}

impl TryFrom<CastKind> for ValueType {
    type Error = CastKind;

    fn try_from(value: CastKind) -> Result<Self, Self::Error> {
        match value {
            CastKind::ToChar => Ok(ValueType::Char),
            CastKind::ToInt(to) => Ok(ValueType::Int(to)),
            CastKind::ToFloat(to) => Ok(ValueType::Float(to)),
            _ => Err(value),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, dm::From, dm::Into)]
pub(crate) struct FuncDef {
    pub body_id: InstanceKindId,
    pub raw: Option<FuncRawAddr>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, dm::From, dm::Into)]
pub(crate) struct FuncRawAddr {
    pub static_addr: NonNull<()>,
    pub as_dyn_method: Option<(DynRawMetadata, u64)>,
}

impl Into<InstanceKindId> for FuncDef {
    fn into(self) -> InstanceKindId {
        self.body_id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, dm::From)]
pub(crate) struct CalleeDef {
    pub callee_id: InstanceKindId,
    pub raw: Option<FuncRawAddr>,
}

pub(crate) trait HasTags {
    fn tags(&self) -> &[Tag];

    fn has_tag<T: PartialEq<Tag>>(&self, tag: &T) -> bool {
        self.tags().iter().any(|t| tag == t)
    }
}
