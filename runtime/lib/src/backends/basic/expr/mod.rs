pub(super) mod builders;
mod fmt;
pub(crate) mod lazy;
pub(super) mod place;
pub(crate) mod prelude;
pub(super) mod translators;
mod utils;

use std::{
    num::{NonZeroU32, Wrapping},
    rc::Rc,
};

use delegate::delegate;
use derive_more as dm;

use common::type_info::TypeInfo;

use crate::abs::expr::sym_place::{Select, SymbolicReadTree};
use crate::utils::meta::{define_reversible_pair, sub_enum};
use place::SymPlaceValueRef;

use crate::abs;
pub(crate) use crate::abs::{
    FloatType, IntType, PointerOffset, RawAddress, TypeId, ValueType, VariantIndex,
};

#[derive(Clone, PartialEq, Eq, dm::Deref, dm::DerefMut, dm::Debug, dm::Display)]
pub(crate) struct ValueRef(Rc<Value>);
pub(crate) type ConcreteValueRef = guards::ConcreteValueGuard<ValueRef>;
pub(crate) type SymValueRef = guards::SymValueGuard<ValueRef>;
pub(crate) type SymTernaryOperands = guards::SymTernaryOperands;

impl ValueRef {
    pub(crate) fn new(value: Value) -> Self {
        Self(Rc::new(value))
    }

    pub fn unwrap_or_clone(this: Self) -> Value {
        Rc::unwrap_or_clone(this.0)
    }

    pub fn make_mut(this: &mut Self) -> &mut Value {
        Rc::make_mut(&mut this.0)
    }
}

impl AsRef<Value> for ValueRef {
    delegate! {
        to self.0 {
            fn as_ref(&self) -> &Value;
        }
    }
}

pub(crate) type SymVarId = u32;

#[derive(Clone, Debug, PartialEq, Eq, dm::From)]
pub(crate) enum Value {
    #[from(ConcreteValue, ConstValue, AdtValue, ArrayValue, UnevalValue)]
    Concrete(ConcreteValue),
    #[from(SymValue, Expr)]
    Symbolic(SymValue),
}

impl Value {
    #[inline]
    pub(crate) fn is_symbolic(&self) -> bool {
        self.as_sym().is_some()
    }

    #[inline]
    pub(crate) fn as_sym(&self) -> Option<&SymValue> {
        match self {
            Value::Symbolic(sym) => Some(sym),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, dm::From)]
pub(crate) enum ConcreteValue {
    #[from]
    Const(ConstValue),
    #[from]
    Adt(AdtValue),
    #[from]
    Array(ArrayValue),
    #[from]
    FatPointer(FatPtrValue),
    #[from(forward)]
    Unevaluated(UnevalValue),
}

// FIXME: Remove this error suppression after adding support for floats.
#[allow(unused)]
#[derive(Clone, Debug, PartialEq, Eq, dm::From)]
pub(crate) enum ConstValue {
    Bool(bool),
    Char(char),
    Int {
        bit_rep: Wrapping<u128>,
        ty: IntType,
    },
    Float {
        bit_rep: u128,
        ty: FloatType,
    },
    #[from(ignore)]
    Addr(RawAddress),
}

impl ConstValue {
    #[inline]
    pub fn new_int<T: Into<u128>>(value: T, ty: IntType) -> Self {
        Self::Int {
            bit_rep: Wrapping(value.into()),
            ty,
        }
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        match self {
            Self::Bool(value) => !value,
            Self::Int { bit_rep, .. } => *bit_rep == Wrapping(0),
            Self::Float { .. } => todo!(),
            _ => unreachable!("Only numerical values can be checked for zero."),
        }
    }

    #[inline]
    pub fn is_one(&self) -> bool {
        match self {
            Self::Bool(value) => *value,
            Self::Int { bit_rep, .. } => *bit_rep == Wrapping(1),
            Self::Float { .. } => todo!(),
            _ => unreachable!("Only numerical values can be checked for one."),
        }
    }

    pub fn unary_op(this: &Self, operator: UnaryOp) -> ConstValue {
        match operator {
            UnaryOp::Neg => match this {
                Self::Int {
                    bit_rep,
                    ty:
                        ty @ IntType {
                            is_signed: true, ..
                        },
                } => Self::Int {
                    bit_rep: !bit_rep + Wrapping(1),
                    ty: *ty,
                },
                Self::Float { .. } => unimplemented!(),
                _ => unreachable!("Negation is meant only for signed integers and floats."),
            },
            UnaryOp::Not => match this {
                Self::Bool(value) => Self::Bool(!value),
                Self::Int { bit_rep, ty } => Self::Int {
                    bit_rep: !bit_rep,
                    ty: *ty,
                },
                _ => unreachable!("Not operand only works on boolean and integers."),
            },
            _ => unimplemented!("This function over constant values will be dropped soon."),
        }
    }

    pub fn binary_op(first: &Self, second: &Self, operator: abs::BinaryOp) -> ConcreteValue {
        use abs::BinaryOp::*;
        match operator {
            Add | Sub | Mul | Div | Rem | BitXor | BitAnd | BitOr => ConcreteValue::Const(
                Self::binary_op_arithmetic(first, second, operator.try_into().unwrap()),
            ),
            Shl | Shr => ConcreteValue::Const(Self::binary_op_shift(first, second, operator)),
            AddWithOverflow | SubWithOverflow | MulWithOverflow => ConcreteValue::Adt(
                Self::binary_op_with_overflow_arithmetic(first, second, operator),
            ),
            AddUnchecked | SubUnchecked | MulUnchecked => {
                todo!("#197")
            }
            Eq | Lt | Le | Ne | Ge | Gt => ConcreteValue::Const(ConstValue::Bool(
                Self::binary_op_cmp(first, second, operator.try_into().unwrap()),
            )),
            _ => unimplemented!("{:?} {:?} {:?}", first, second, operator),
        }
    }

    pub fn integer_cast(this: &Self, to: IntType) -> Self {
        match this {
            /* This seems overly simple but when the number is originally cast to the u128 to get its bit representation,
             * this covers any of the casting that would need to be done here. If the original number was unsigned then
             * the leading bits of the u128 will be 0s and if it was signed then the leading bits will be 1s to handle
             * the sign extension. Now here when we track the actual cast that needs to be done, the target type has at
             * most 128 bits so we can just truncate the leading bits to get the correct bit representation.
             */
            Self::Int { bit_rep, .. } => Self::Int {
                bit_rep: {
                    // truncate leading bits as necessary
                    let result = Wrapping(Self::to_size((*bit_rep).0, &to));
                    debug_assert!(Self::in_bounds(result.0, &to), "result out of bounds");
                    result
                },
                ty: to,
            },
            Self::Bool(value) => Self::Int {
                bit_rep: Wrapping(*value as u128),
                ty: to,
            },
            Self::Char(value) => Self::Int {
                bit_rep: Wrapping(*value as u128),
                ty: to,
            },
            Self::Float { .. } => todo!("Casting float to integer is not implemented yet."),
            _ => unreachable!("Casting {this:?} to integer is not possible."),
        }
    }

    fn binary_op_arithmetic(first: &Self, second: &Self, operator: BinaryOp) -> Self {
        match (first, second) {
            (
                Self::Int {
                    bit_rep: first,
                    ty: first_ty,
                },
                Self::Int {
                    bit_rep: second,
                    ty: second_ty,
                },
            ) => {
                assert_eq!(first_ty.bit_size, second_ty.bit_size);

                let result = match operator {
                    BinaryOp::Add => first + second,
                    BinaryOp::Sub => first - second,
                    BinaryOp::Mul => first * second,
                    BinaryOp::Div => first / second,
                    BinaryOp::Rem => first % second,
                    BinaryOp::BitXor => first ^ second,
                    BinaryOp::BitAnd => first & second,
                    BinaryOp::BitOr => first | second,
                    _ => unreachable!(),
                };

                Self::Int {
                    bit_rep: result,
                    ty: *first_ty,
                }
            }

            (Self::Float { .. }, Self::Float { .. }) => unimplemented!(),

            (Self::Bool(first_value), Self::Bool(second_value)) => {
                let result = match operator {
                    BinaryOp::BitXor => first_value ^ second_value,
                    BinaryOp::BitAnd => first_value & second_value,
                    BinaryOp::BitOr => first_value | second_value,
                    _ => unreachable!(),
                };
                Self::Bool(result)
            }

            _ => unreachable!(),
        }
    }

    fn binary_op_with_overflow_arithmetic(
        first: &Self,
        second: &Self,
        operator: abs::BinaryOp,
    ) -> AdtValue {
        /// use logic to determine whether the operation will overflow or underflow or be in bounds
        fn checked_op(
            operator: abs::BinaryOp,
            first: &Wrapping<u128>,
            second: &Wrapping<u128>,
            ty @ IntType { is_signed, .. }: IntType, // pattern matching in function args, cool!
        ) -> Option<u128> {
            // we don't want any wrapping in this function, so we take the 0th parameter
            let (first, second) = (first.0, second.0);
            if is_signed {
                // casting between same sized integers is a no-op (see rust docs)
                let first = first as i128;
                let second = second as i128;

                let result = match operator {
                    abs::BinaryOp::Add => first.checked_add(second),
                    abs::BinaryOp::Sub => first.checked_sub(second),
                    abs::BinaryOp::Mul => first.checked_mul(second),
                    _ => unreachable!("unsupported by rust"),
                };

                if let Some(result) = result {
                    // case: i128 has not overflowed, so result is valid. Ensure we're in our type's bounds
                    if ConstValue::in_bounds(result as u128, &ty) {
                        Some(result as u128)
                    } else {
                        None
                    }
                } else {
                    // case: i128 overflowed, so any smaller type also overflowed
                    None
                }
            } else {
                let result = match operator {
                    abs::BinaryOp::Add => first.checked_add(second),
                    abs::BinaryOp::Sub => first.checked_sub(second),
                    abs::BinaryOp::Mul => first.checked_mul(second),
                    _ => unreachable!("unsupported by rust"),
                };

                // If u128 overflows, any smaller type also overflows.
                result.filter(|result| {
                    // u128 has not overflowed, check if we're higher than our max.
                    ConstValue::in_bounds(*result, &ty)
                })
            }
        }

        // only integers are supported, as per the rust docs:
        // https://doc.rust-lang.org/beta/nightly-rustc/rustc_middle/mir/syntax/enum.Rvalue.html#variant.CheckedBinaryOp
        match (first, second) {
            (
                Self::Int {
                    bit_rep: first,
                    ty:
                        ty @ IntType {
                            bit_size: first_size,
                            is_signed: first_signed,
                        },
                },
                Self::Int {
                    bit_rep: second,
                    ty: ty_second @ IntType { .. },
                },
            ) => {
                assert_eq!(*ty, *ty_second);

                let result = checked_op(operator, first, second, *ty);
                match result {
                    Some(result) => {
                        let ty = IntType {
                            bit_size: *first_size,
                            is_signed: *first_signed,
                        };
                        AdtValue::checked_success(Wrapping(result), ty)
                    }
                    None => AdtValue::checked_overflow(),
                }
            }
            _ => unreachable!("only integers are supported by rust"),
        }
    }

    fn binary_op_shift(first: &Self, second: &Self, operator: abs::BinaryOp) -> Self {
        match (first, second) {
            (
                Self::Int {
                    bit_rep: first,
                    ty: first_ty,
                },
                Self::Int {
                    bit_rep: second,
                    ty: second_ty,
                },
            ) => {
                assert!(
                    !second_ty.is_signed || Self::is_positive(second.0, second_ty.bit_size),
                    "Shifting by a negative value is not expected."
                ); //TODO we can get rid of this assertion in the future

                let result = match operator {
                    // if second.0 is u128 or u64 & too big for usize, it will be usize::MAX
                    abs::BinaryOp::Shl => first << second.0 as usize,
                    abs::BinaryOp::Shr => first >> second.0 as usize,
                    _ => unreachable!("invalid binop"),
                };

                let result = Wrapping(Self::to_size(result.0, first_ty));

                debug_assert!(
                    Self::in_bounds(result.0, first_ty),
                    "result {} out of bounds",
                    result.0
                );

                Self::Int {
                    bit_rep: result,
                    ty: *first_ty,
                }
            }
            _ => unreachable!("Shifting is only possible on integers."),
        }
    }

    fn binary_op_cmp(first: &Self, second: &Self, operator: BinaryOp) -> bool {
        match operator {
            BinaryOp::Eq => first.eq(second),
            BinaryOp::Ne => first.ne(second),
            _ => match (first, second) {
                (
                    Self::Int {
                        bit_rep: first,
                        ty: first_ty,
                    },
                    Self::Int {
                        bit_rep: second,
                        ty: second_ty,
                    },
                ) => {
                    assert_eq!(*first_ty, *second_ty);

                    if first_ty.is_signed {
                        // `as i128` is a bitwise transmute
                        let first = first.0 as i128;
                        let second = second.0 as i128;
                        match operator {
                            BinaryOp::Lt => first < second,
                            BinaryOp::Le => first <= second,
                            BinaryOp::Ge => first >= second,
                            BinaryOp::Gt => first > second,
                            _ => unreachable!(),
                        }
                    } else {
                        match operator {
                            BinaryOp::Lt => first < second,
                            BinaryOp::Le => first <= second,
                            BinaryOp::Ge => first >= second,
                            BinaryOp::Gt => first > second,
                            _ => unreachable!(),
                        }
                    }
                }

                (Self::Float { .. }, Self::Float { .. }) => unimplemented!(),

                _ => unimplemented!(),
            },
        }
    }

    fn is_positive(bit_rep: u128, size: u64) -> bool {
        let mask: u128 = 1 << (size - 1);
        bit_rep & mask == 0
    }

    /// determines whether value is valid for the type ty (not left zero-padded, for example)
    fn in_bounds(value: u128, ty: &IntType) -> bool {
        if ty.is_signed {
            let max = ((1_u128 << (ty.bit_size - 1)) - 1) as i128;
            let min = match ty.bit_size {
                0..=127 => -(1_i128 << (ty.bit_size - 1)),
                128 => i128::MIN,
                129..=u64::MAX => panic!("unsupported integer size; too large"),
            };
            let value = value as i128;
            value <= max && value >= min
        } else {
            let max = match ty.bit_size {
                0..=127 => (1_u128 << ty.bit_size) - 1,
                128 => u128::MAX,
                129..=u64::MAX => panic!("unsupported integer size; too large"),
            };
            value <= max
        }
    }

    /// Applies truncation and casting as necessary to keep value within ty's size bounds
    /// (despite still being a u128 value)
    fn to_size(value: u128, ty: &IntType) -> u128 {
        if ty.bit_size == 128 {
            return value;
        }

        // create a mask of all the significant bits, then truncate to size
        let mask: u128 = (1_u128 << (ty.bit_size as u128)) - 1;
        let value = value & mask;

        if ty.is_signed {
            Self::sign_ext(value, ty.bit_size)
        } else {
            value // implicit zero extension
        }
    }

    /// A sign extension is equivalent to casting value to ty, then to u128
    fn sign_ext(value: u128, bit_size: u64) -> u128 {
        let bits_to_shift = 128 - bit_size;
        let value = value as i128;
        ((value << bits_to_shift) >> bits_to_shift) as u128
    }

    fn try_to_bit_rep(&self) -> Result<u128, &Self> {
        match self {
            Self::Bool(value) => Ok(*value as u128),
            Self::Char(value) => Ok(*value as u128),
            Self::Int { bit_rep, .. } => Ok(bit_rep.0),
            Self::Float { bit_rep, .. } => Ok(*bit_rep),
            Self::Addr(value) => Ok((*value as usize) as u128),
        }
    }
}

// ----------------------------------------------- //

// FIXME: Remove this error suppression after adding support for more variants
#[allow(unused)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AdtKind {
    Struct,
    Enum {
        /* NOTE: Even when the variant index is set based on a decision on a
         * symbolic value, its value is still a concrete and the symbolic value
         * will affect the constraints. */
        variant: VariantIndex,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AdtValue {
    pub kind: AdtKind,
    pub fields: Vec<AdtField>,
}
impl AdtValue {
    /// creates an ADT that is the result of a checked operation that overflowed
    fn checked_overflow() -> Self {
        Self {
            kind: AdtKind::Struct,
            fields: vec![
                Self::field_for_checked(None),
                Self::field_for_checked(Some(true.into())),
            ],
        }
    }

    /// creates an ADT that represents the result of a successful checked operation (no overflow)
    fn checked_success(result: Wrapping<u128>, ty: IntType) -> Self {
        Self {
            kind: AdtKind::Struct,
            fields: vec![
                Self::field_for_checked(Some(ConstValue::Int {
                    bit_rep: result,
                    ty,
                })),
                Self::field_for_checked(Some(false.into())),
            ],
        }
    }

    fn field_for_checked(value: Option<ConstValue>) -> AdtField {
        AdtField {
            value: value.map(ConstValue::to_value_ref),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, dm::From)]
pub(crate) struct AdtField {
    pub value: Option<ValueRef>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ArrayValue {
    pub elements: Vec<ValueRef>,
}

impl ArrayValue {
    #[inline]
    pub fn len(&self) -> usize {
        self.elements.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FatPtrValue {
    pub address: ConcreteValueRef,
    pub metadata: ConcreteValueRef,
    pub ty: TypeId,
}

#[derive(Clone, Debug, PartialEq, Eq, dm::From)]
pub(crate) enum UnevalValue {
    Some,
    Lazy(RawConcreteValue),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawConcreteValue(pub(crate) RawAddress, pub(crate) LazyTypeInfo);

#[derive(Clone, Debug, PartialEq, Eq, dm::From)]
pub(crate) enum LazyTypeInfo {
    None,
    Id(TypeId),
    /* NOTE: Can we perform evaluation without storing the type separately?
     * It seems to be possible as we evaluate them only when we
     * are evaluating a operation which includes a symbolic value (otherwise
     * the concrete result would be referenced).
     * Thus, we should be able to infer the type based on the type of symbolic
     * value.
     * However there are a few cases that this is not easily achievable:
     * - In shift operations, which the operands may not be from the same type.
     * - Arrays and effectively symbolic projections.
     * So we may end up with a hybrid solution. Currently, we have defined
     * the type as an option to not pass the type in the future unless it is
     * necessary.
     */
    IdPrimitive(TypeId, ValueType),
    Fetched(&'static TypeInfo),
    #[from(ignore)]
    Forced(Rc<TypeInfo>),
}

impl LazyTypeInfo {
    pub(crate) fn id(&self) -> Option<TypeId> {
        match self {
            Self::None => None,
            Self::Id(id) => Some(*id),
            Self::IdPrimitive(id, _) => Some(*id),
            Self::Fetched(ty) => Some(ty.id),
            Self::Forced(ty) => Some(ty.id),
        }
    }
}

impl From<(Option<TypeId>, Option<ValueType>)> for LazyTypeInfo {
    fn from(pair: (Option<TypeId>, Option<ValueType>)) -> Self {
        match pair {
            (Some(ty_id), Some(value_ty)) => Self::IdPrimitive(ty_id, value_ty),
            (Some(ty_id), None) => Self::Id(ty_id),
            _ => Self::None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, dm::From)]
pub(crate) enum SymValue {
    Variable(SymbolicVar),
    #[from(forward)]
    Expression(Expr),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SymbolicVar {
    pub id: SymVarId,
    pub ty: ValueType,
}

impl SymbolicVar {
    pub fn new(id: SymVarId, ty: ValueType) -> Self {
        Self { id, ty }
    }
}

define_reversible_pair!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    BinaryOperands<F, S> {
        (Orig, Rev) {
            first: F,
            second: S,
        }
    },
    #[allow(dead_code)]
    impl
);

pub(crate) type SymBinaryOperands = BinaryOperands<SymValueRef, ValueRef>;

impl SymBinaryOperands {
    pub fn first(&self) -> &ValueRef {
        match self {
            Self::Orig { first, .. } => &first.0,
            Self::Rev { first, .. } => first,
        }
    }

    pub fn second(&self) -> &ValueRef {
        match self {
            Self::Orig { second, .. } => second,
            Self::Rev { second, .. } => &second.0,
        }
    }
}

sub_enum! {
    #[repr(u8)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum UnaryOp from abs::UnaryOp {
        Neg,
        Not,
        BitReverse,
        TrailingZeros +| NonZeroTrailingZeros,
        LeadingZeros +| NonZeroLeadingZeros,
        CountOnes,
    }
}

sub_enum! {
    #[repr(u8)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum BinaryOp from abs::BinaryOp {
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
        RotateL,
        RotateR,

        Eq,
        Lt,
        Le,
        Ne,
        Ge,
        Gt,
        Cmp,

        Offset,
    }
}

sub_enum! {
    #[repr(u8)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum OverflowingBinaryOp from BinaryOp {
        Add,
        Sub,
        Mul,
    }
}

impl OverflowingBinaryOp {
    #[inline]
    pub fn is_possible(&self, overflow: bool, is_signed: bool) -> bool {
        use OverflowingBinaryOp::*;
        match (overflow, self, is_signed) {
            // Impossible. Largest case: MAX - 0
            (true, Sub, false) => false,
            // Impossible. Smallest case: 0 . 0
            (false, Add | Mul, false) => false,
            _ => true,
        }
    }
}

// FIXME: Remove this error suppression after adding support for more variants
#[allow(unused)]
#[derive(Clone, Debug, PartialEq, Eq, dm::From)]
pub(crate) enum Expr {
    Unary {
        operator: UnaryOp,
        operand: SymValueRef,
    },

    Binary(BinaryExpr),
    #[from(ignore)]
    BinaryBoundCheck {
        bin_expr: BinaryExpr<OverflowingBinaryOp>,
        is_overflow: bool,
    },

    Extension(ExtensionExpr),

    Truncation(TruncationExpr),

    Ite {
        condition: SymValueRef,
        if_target: ValueRef,
        else_target: ValueRef,
    },

    Transmutation {
        source: SymValueRef,
        dst_ty: LazyTypeInfo,
    },

    Multi(MultiValue),

    /// A reference or the address of a value.
    /* NOTE: Should not we have a separate variant for the address of a value?
     * It should not be required in two ways:
     * First, the address of is obtained in the same way as a reference is obtained,
     * i.e., by taking a place. Also the case in which a reference is converted
     * to a pointer (address), it is first dereferenced and then the address is
     * obtained from the result. So effectively equivalent.
     * Second, we are working at runtime which means references are already
     * converted to pointers. So, we should also have the same behavior for both.
     */
    #[from(ignore)]
    Ref(SymPlaceValueRef),

    #[from(ignore)]
    Len(SymPlaceValueRef),

    Partial(PorterValue),

    #[from(ignore)]
    PtrMetadata(SymValueRef),
}

#[allow(unused)]
#[derive(Clone, Debug, PartialEq, Eq, dm::From)]
pub(crate) struct BinaryExpr<Operator = BinaryOp, Operands = SymBinaryOperands> {
    operator: Operator,
    operands: Operands,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExtensionExpr {
    pub(crate) source: SymValueRef,
    pub(crate) is_zero_ext: bool,
    pub(crate) bits_to_add: NonZeroU32,
    // The destination type can be an integer or a pointer type, or char.
    pub(crate) ty: ValueType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TruncationExpr {
    pub(crate) source: SymValueRef,
    // https://doc.rust-lang.org/reference/expressions/operator-expr.html#type-cast-expressions
    // A truncation only happens with a destination type that is an integer.
    pub(crate) ty: IntType,
}

pub(crate) type SymIndex = SliceIndex<SymValueRef>;
pub(crate) type MultiValueLeaf = ValueRef;
pub(crate) type MultiValueTree = SymbolicReadTree<SymIndex, MultiValueLeaf>;
pub(crate) type MultiValueArray = Vec<MultiValueTree>;
pub(crate) type MultiValue = Select<SymIndex, MultiValueTree>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SliceIndex<I> {
    pub index: I,
    pub from_end: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PorterValue {
    pub(crate) as_concrete: RawConcreteValue,
    pub(crate) sym_values: Vec<(PointerOffset, TypeId, SymValueRef)>,
}

mod guards {
    use super::*;

    macro_rules! define_guard {
        ($base_type:ty, $ref_type:ty, $guarded_type:ty, $name: ident, $pattern:pat, $value_name:ident) => {
            #[derive(Clone, PartialEq, Eq)]
            pub(crate) struct $name<V>(pub V);

            impl<V> $name<V>
            where
                V: AsRef<$base_type>,
            {
                pub fn new(value: V) -> Self {
                    #![allow(unused_variables)]
                    core::assert_matches::assert_matches!(
                        value.as_ref(),
                        $pattern,
                        concat!("Value should be ", stringify!($guarded_type), ".")
                    );
                    Self(value)
                }

                #[inline]
                pub fn $value_name(&self) -> &$guarded_type {
                    match self.0.as_ref() {
                        $pattern => $value_name,
                        _ => unreachable!(),
                    }
                }
            }

            impl $name<$ref_type> {
                #[allow(unused)]
                pub fn make_mut(this: &mut Self) -> &mut $guarded_type {
                    match <$ref_type>::make_mut(&mut this.0) {
                        $pattern => $value_name,
                        _ => unreachable!(),
                    }
                }
            }

            impl<V> AsRef<V> for $name<V> {
                #[inline]
                fn as_ref(&self) -> &V {
                    &self.0
                }
            }

            impl<V> AsMut<V> for $name<V> {
                #[inline]
                fn as_mut(&mut self) -> &mut V {
                    &mut self.0
                }
            }

            impl From<$name<$ref_type>> for $ref_type {
                #[inline]
                fn from(value: $name<$ref_type>) -> Self {
                    value.0
                }
            }

            impl<V: Clone> $name<V> {
                #[inline]
                pub fn clone_to(&self) -> V {
                    self.0.clone()
                }
            }

            impl<V> std::fmt::Debug for $name<V>
            where
                V: std::fmt::Debug,
            {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    self.0.fmt(f)
                }
            }

            impl<V> std::fmt::Display for $name<V>
            where
                V: std::fmt::Display,
            {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    self.0.fmt(f)
                }
            }
        };
    }
    pub(crate) use define_guard;

    macro_rules! define_value_guard {
        ($guarded_type:ty, $name: ident, $pattern:pat, $value_name:ident) => {
            define_guard!(Value, ValueRef, $guarded_type, $name, $pattern, $value_name);

            impl<V> core::ops::Deref for $name<V>
            where
                V: AsRef<Value>,
            {
                type Target = $guarded_type;

                #[inline]
                fn deref(&self) -> &Self::Target {
                    self.$value_name()
                }
            }

            impl<V> AsRef<$guarded_type> for $name<V>
            where
                Self: core::ops::Deref<Target = $guarded_type>,
            {
                #[inline]
                fn as_ref(&self) -> &$guarded_type {
                    self
                }
            }
        };
    }

    define_value_guard!(
        ConcreteValue,
        ConcreteValueGuard,
        Value::Concrete(value),
        value
    );
    define_value_guard!(SymValue, SymValueGuard, Value::Symbolic(value), value);

    #[derive(Debug)]
    // NOTE: We skip structural guarantee like in SymBinaryOperands because of 7 possible permutations.
    pub(crate) struct SymTernaryOperands(
        pub(crate) ValueRef,
        pub(crate) ValueRef,
        pub(crate) ValueRef,
    );

    impl SymTernaryOperands {
        pub(crate) fn new(first: ValueRef, second: ValueRef, third: ValueRef) -> Self {
            assert!(first.is_symbolic() || second.is_symbolic() || third.is_symbolic());
            Self(first, second, third)
        }
    }

    impl Into<(ValueRef, ValueRef, ValueRef)> for SymTernaryOperands {
        fn into(self) -> (ValueRef, ValueRef, ValueRef) {
            (self.0, self.1, self.2)
        }
    }
}
use guards::define_guard;

define_reversible_pair!(
    SymIndexPair {
        (SymHost, SymIndex) {
            host: SymValueRef,
            index: ValueRef,
        }
    },
    #[allow(dead_code)]
    impl
);

#[allow(clippy::wrong_self_convention)]
mod convert {
    use super::*;
    use crate::abs;

    impl Value {
        #[inline]
        pub(crate) fn to_value_ref(self) -> ValueRef {
            ValueRef::new(self)
        }
    }

    impl ConcreteValue {
        #[inline]
        pub(crate) fn to_value_ref(self) -> ValueRef {
            Into::<Value>::into(self).to_value_ref()
        }
    }

    impl TryFrom<abs::Constant> for ConstValue {
        type Error = abs::Constant;

        fn try_from(value: abs::Constant) -> Result<Self, Self::Error> {
            use abs::Constant::*;
            match value {
                Bool(value) => Ok(Self::Bool(value)),
                Char(value) => Ok(Self::Char(value)),
                Int { bit_rep, ty } => Ok(Self::Int {
                    bit_rep: Wrapping(bit_rep),
                    ty,
                }),
                Float { bit_rep, ty } => Ok(Self::Float { bit_rep, ty }),
                Zst | Str(..) | ByteStr(..) | Some => Err(value),
            }
        }
    }

    impl From<abs::Constant> for ConcreteValue {
        #[inline]
        fn from(val: abs::Constant) -> Self {
            use abs::Constant::*;
            match val {
                Zst | Str(..) | ByteStr(..) | Some => UnevalValue::Some.into(),
                _ => Self::Const(val.try_into().unwrap()),
            }
        }
    }

    macro_rules! impl_from_int_type {
        ($signed:expr, $($ty:ty),*) => {
            $(
                impl From<$ty> for ConstValue {
                    fn from(value: $ty) -> Self {
                        Self::Int {
                            bit_rep: Wrapping(value as u128),
                            ty: IntType {
                                bit_size: std::mem::size_of::<$ty>() as u64 * 8,
                                is_signed: $signed,
                            },
                        }
                    }
                }
            )*
        };
    }

    impl_from_int_type!(false, u8, u16, u32, u64, u128, usize);
    impl_from_int_type!(true, i8, i16, i32, i64, i128, isize);

    macro_rules! impl_conc_to_value_ref {
        ($($ty: ty),* $(,)?) => {
            $(
                impl $ty {
                    #[inline]
                    pub(crate) fn to_value_ref(self) -> ValueRef {
                        Into::<ConcreteValue>::into(self).to_value_ref()
                    }
                }
            )*
        };
    }

    impl_conc_to_value_ref!(
        ConstValue,
        AdtValue,
        ArrayValue,
        FatPtrValue,
        UnevalValue,
        RawConcreteValue,
    );

    impl SymValue {
        #[inline]
        pub fn to_value_ref(self) -> SymValueRef {
            SymValueRef::new(ValueRef::new(self.into()))
        }
    }

    macro_rules! impl_sym_to_value_ref {
        ($($ty: ty),* $(,)?) => {
            $(
                impl $ty {
                    #[inline]
                    pub(crate) fn to_value_ref(self) -> SymValueRef {
                        Into::<SymValue>::into(self).to_value_ref()
                    }
                }
            )*
        };
    }

    impl_sym_to_value_ref!(Expr, BinaryExpr, ExtensionExpr, TruncationExpr, PorterValue,);

    impl<'a> TryFrom<&'a Value> for ValueType {
        type Error = &'a Value;

        fn try_from(value: &'a Value) -> Result<Self, Self::Error> {
            match value {
                Value::Concrete(con_val) => ValueType::try_from(con_val).or(Err(value)),
                Value::Symbolic(sym_val) => ValueType::try_from(sym_val).or(Err(value)),
            }
        }
    }

    impl<'a> TryFrom<&'a SymValue> for ValueType {
        type Error = &'a SymValue;

        fn try_from(value: &'a SymValue) -> Result<Self, Self::Error> {
            match value {
                SymValue::Variable(SymbolicVar { id: _, ty }) => Ok(ty.clone()),
                SymValue::Expression(expr) => match expr {
                    Expr::Unary {
                        operator: _,
                        operand,
                    } => ValueType::try_from(operand.value()),
                    Expr::Binary(BinaryExpr { operator, operands }) => {
                        use BinaryOp::*;
                        match operator {
                            Eq | Lt | Le | Ne | Ge | Gt => Ok(ValueType::Bool),
                            _ => ValueType::try_from(operands.first().as_ref()).map_err(|_| value),
                        }
                    }
                    Expr::BinaryBoundCheck { .. } => Ok(ValueType::Bool),
                    Expr::Extension(ExtensionExpr { ty, .. }) => Ok(ty.clone()),
                    Expr::Truncation(TruncationExpr { ty, .. }) => Ok((*ty).into()),
                    Expr::Ite {
                        condition: _,
                        if_target,
                        else_target,
                    } => Option::zip(
                        ValueType::try_from(if_target.as_ref()).ok(),
                        ValueType::try_from(else_target.as_ref()).ok(),
                    )
                    .map(|(if_ty, else_ty)| {
                        debug_assert_eq!(
                            if_ty, else_ty,
                            "The types of ITE operands must be the same."
                        );
                        if_ty
                    })
                    .ok_or(value),
                    Expr::Transmutation { dst_ty, .. } => dst_ty.try_into().map_err(|_| value),
                    Expr::Len(..) => Ok(IntType::USIZE.into()),
                    Expr::Multi(select) => {
                        ValueType::try_from(select.first_leaf().as_ref()).map_err(|_| value)
                    }
                    Expr::Partial(PorterValue {
                        as_concrete: RawConcreteValue(_, ty),
                        ..
                    }) => ty.try_into().map_err(|_| value),
                    Expr::Ref(..) => Err(value),
                    Expr::PtrMetadata(..) => Err(value),
                },
            }
        }
    }

    impl<'a> TryFrom<&'a ConcreteValue> for ValueType {
        type Error = &'a ConcreteValue;

        fn try_from(value: &'a ConcreteValue) -> Result<Self, Self::Error> {
            match value {
                ConcreteValue::Const(const_value) => match const_value {
                    ConstValue::Bool(_) => Ok(ValueType::Bool),
                    ConstValue::Char(_) => Ok(ValueType::Char),
                    ConstValue::Int { bit_rep: _, ty } => Ok((*ty).into()),
                    ConstValue::Float { bit_rep: _, ty } => Ok((*ty).into()),
                    _ => Err(value),
                },
                ConcreteValue::Unevaluated(UnevalValue::Lazy(RawConcreteValue(
                    _,
                    LazyTypeInfo::IdPrimitive(_, value_ty),
                ))) => Ok(*value_ty),
                _ => Err(value),
            }
        }
    }

    impl<'a> TryFrom<&'a LazyTypeInfo> for ValueType {
        type Error = &'a LazyTypeInfo;

        fn try_from(value: &'a LazyTypeInfo) -> Result<Self, Self::Error> {
            match value {
                LazyTypeInfo::IdPrimitive(_, value_ty) => Ok(value_ty.clone()),
                _ => Err(value),
            }
        }
    }

    impl From<MultiValueArray> for MultiValueTree {
        fn from(value: MultiValueArray) -> Self {
            Self::Array(value)
        }
    }

    impl From<MultiValueLeaf> for MultiValueTree {
        fn from(value: MultiValueLeaf) -> Self {
            SymbolicReadTree::Single(value)
        }
    }
}
