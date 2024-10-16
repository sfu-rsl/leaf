pub(crate) mod backend;
pub(crate) mod expr;
pub(crate) mod fmt;
pub(crate) mod place;

pub(crate) use common::types::*;

use core::num::NonZeroU64;

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum BinaryOp {
    Add = common::pri::BinaryOp::ADD.as_u8(),
    AddUnchecked = common::pri::BinaryOp::ADD_UNCHECKED.as_u8(),
    AddWithOverflow = common::pri::BinaryOp::ADD_WITH_OVERFLOW.as_u8(),
    AddSaturating = common::pri::BinaryOp::ADD_SATURATING.as_u8(),
    Sub = common::pri::BinaryOp::SUB.as_u8(),
    SubUnchecked = common::pri::BinaryOp::SUB_UNCHECKED.as_u8(),
    SubWithOverflow = common::pri::BinaryOp::SUB_WITH_OVERFLOW.as_u8(),
    SubSaturating = common::pri::BinaryOp::SUB_SATURATING.as_u8(),
    Mul = common::pri::BinaryOp::MUL.as_u8(),
    MulUnchecked = common::pri::BinaryOp::MUL_UNCHECKED.as_u8(),
    MulWithOverflow = common::pri::BinaryOp::MUL_WITH_OVERFLOW.as_u8(),
    Div = common::pri::BinaryOp::DIV.as_u8(),
    DivExact = common::pri::BinaryOp::DIV_EXACT.as_u8(),
    Rem = common::pri::BinaryOp::REM.as_u8(),

    BitXor = common::pri::BinaryOp::BIT_XOR.as_u8(),
    BitAnd = common::pri::BinaryOp::BIT_AND.as_u8(),
    BitOr = common::pri::BinaryOp::BIT_OR.as_u8(),
    Shl = common::pri::BinaryOp::SHL.as_u8(),
    ShlUnchecked = common::pri::BinaryOp::SHL_UNCHECKED.as_u8(),
    Shr = common::pri::BinaryOp::SHR.as_u8(),
    ShrUnchecked = common::pri::BinaryOp::SHR_UNCHECKED.as_u8(),
    RotateL = common::pri::BinaryOp::ROT_L.as_u8(),
    RotateR = common::pri::BinaryOp::ROT_R.as_u8(),

    Eq = common::pri::BinaryOp::EQ.as_u8(),
    Lt = common::pri::BinaryOp::LT.as_u8(),
    Le = common::pri::BinaryOp::LE.as_u8(),
    Ne = common::pri::BinaryOp::NE.as_u8(),
    Ge = common::pri::BinaryOp::GE.as_u8(),
    Gt = common::pri::BinaryOp::GT.as_u8(),
    Cmp = common::pri::BinaryOp::CMP.as_u8(),

    Offset = common::pri::BinaryOp::OFFSET.as_u8(),
}

impl BinaryOp {
    #[inline]
    pub(crate) fn is_unchecked(&self) -> bool {
        match self {
            Self::AddUnchecked
            | Self::SubUnchecked
            | Self::MulUnchecked
            | Self::ShlUnchecked
            | Self::ShrUnchecked
            | Self::DivExact => true,
            _ => false,
        }
    }

    #[inline]
    pub(crate) fn is_with_overflow(&self) -> bool {
        match self {
            Self::AddWithOverflow | Self::SubWithOverflow | Self::MulWithOverflow => true,
            _ => false,
        }
    }

    #[inline]
    pub(crate) fn is_saturating(&self) -> bool {
        match self {
            Self::AddSaturating | Self::SubSaturating => true,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum UnaryOp {
    Not = common::pri::UnaryOp::NOT.as_u8(),
    Neg = common::pri::UnaryOp::NEG.as_u8(),
    PtrMetadata = common::pri::UnaryOp::PTR_METADATA.as_u8(),
    BitReverse = common::pri::UnaryOp::BIT_REVERSE.as_u8(),
    NonZeroTrailingZeros = common::pri::UnaryOp::CTTZ_NONZERO.as_u8(),
    TrailingZeros = common::pri::UnaryOp::CTTZ.as_u8(),
    CountOnes = common::pri::UnaryOp::CTPOP.as_u8(),
}

pub(crate) type Local = place::Local;
pub(crate) type Place<L = Local, P = Projection<L>> = place::Place<L, P>;
pub(crate) type Projection<L> = place::Projection<L>;

#[derive(Debug)]
pub(crate) enum PlaceUsage {
    Copy,
    Move,
}

#[derive(Debug)]
pub(crate) enum Operand<P, C, S> {
    Place(P, PlaceUsage),
    Const(C),
    Symbolic(S),
}

#[derive(Debug)]
pub(crate) enum Constant {
    Bool(bool),
    Char(char),
    Int { bit_rep: u128, ty: IntType },
    Float { bit_rep: u128, ty: FloatType },
    Str(&'static str),
    ByteStr(&'static [u8]),
    Func(FuncId),
    Zst,
    Some,
}

impl<P, C, S> From<Constant> for Operand<P, C, S>
where
    C: From<Constant>,
{
    fn from(value: Constant) -> Self {
        Self::Const(value.into())
    }
}

#[derive(Debug)]
pub enum AssertKind<Operand> {
    BoundsCheck { len: Operand, index: Operand },
    Overflow(BinaryOp, Operand, Operand),
    OverflowNeg(Operand),
    DivisionByZero(Operand),
    RemainderByZero(Operand),
    ResumedAfterReturn(Operand), // NOTE: TODO: check if these exist in HIR only
    ResumedAfterPanic(Operand),  // NOTE: TODO: check if these exist in HIR only
    MisalignedPointerDereference { required: Operand, found: Operand },
}

#[derive(Debug)]
pub(crate) struct BranchingMetadata {
    pub node_location: BasicBlockIndex,
    /* NOTE: If more type information was passed (such as reporting type for all local variables),
     * this field wouldn't be required. The main usage is for integer types, where
     * they are all compared to an u128. Also, if the backend is able to record
     * type information on the expressions, this field doesn't give any additional
     * information.
     */
    pub discr_as_int: IntType,
}

#[derive(Debug, Clone)]
pub(crate) enum Constraint<V> {
    Bool(V),
    Not(V),
}

impl<V> Constraint<V> {
    pub fn destruct_ref(&self) -> (&V, bool) {
        match self {
            Constraint::Bool(value) => (value, false),
            Constraint::Not(value) => (value, true),
        }
    }

    pub fn not(self) -> Constraint<V> {
        match self {
            Constraint::Bool(value) => Constraint::Not(value),
            Constraint::Not(value) => Constraint::Bool(value),
        }
    }
}

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

    pub(crate) fn bit_size(&self) -> Option<NonZeroU64> {
        match self {
            Self::Bool => None,
            Self::Char => NonZeroU64::new((core::mem::size_of::<char>() * 8) as u64),
            Self::Int(IntType { bit_size, .. }) => NonZeroU64::new(*bit_size),
            Self::Float(FloatType { e_bits, s_bits }) => NonZeroU64::new(e_bits + s_bits),
        }
    }
}

/* FIXME: These types will have a limited set of possible values. Thus they can be
 * optimized using techniques such as interning or even changing them to enums.
 * Enums are not much a favorable option, since they are against the abstract
 * representation of integers and floats in the engine.
 */
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
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
    SizedDynamize, // dyn*
    Transmute(T),
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
