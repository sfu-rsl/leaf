pub(crate) mod backend;
pub(crate) mod expr;

pub(crate) type LocalIndex = u32;
pub type BasicBlockIndex = u32;
pub type VariantIndex = u32;
pub type FieldIndex = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Local {
    ReturnValue,          // 0
    Argument(LocalIndex), // 1-n
    Normal(LocalIndex),   // > n
}
impl std::fmt::Display for Local {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::ReturnValue => write!(f, "ReturnValue"),
            Self::Argument(local) => write!(f, "Arg({})", local),
            Self::Normal(local) => write!(f, "Var({})", local),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitXor,
    BitAnd,
    BitOr,
    Shl,
    Shr,
    Eq,
    Lt,
    Le,
    Ne,
    Ge,
    Gt,
    Offset,
}

#[derive(Clone, Copy, Debug)]
pub enum UnaryOp {
    Not,
    Neg,
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
}

pub(crate) struct BranchingMetadata {
    pub node_location: BasicBlockIndex,
    /* NOTE: If more type information was passed (such as reporting type for all local variables),
     * this field wouldn't be required. The main usage is for integer types, where
     * they are all compared to an u128. Also, if the backend is able to record
     * type information on the expressions, this field doesn't give any additional
     * information.
     */
    pub discr_as_int: DiscriminantAsIntType,
}

pub struct DiscriminantAsIntType {
    pub bit_size: u64,
    pub is_signed: bool,
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

#[derive(Clone, Debug)]
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
}

#[derive(Clone, Debug)]
pub(crate) struct IntType {
    pub bit_size: u64,
    pub is_signed: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct FloatType {
    pub e_bits: u64,
    pub s_bits: u64,
}
