/// This module provides some traits to make it easier to implement expression builders for wrappers
/// that work as adapters. Most of the adapters provide non-trivial covariance and contravariance
/// over the input and output types of the wrapped expression builder.
use super::{
    macros::macro_rules_method_with_optional_args, BinaryExprBuilder, CastExprBuilder,
    TernaryExprBuilder, UnaryExprBuilder,
};
use crate::abs::{BinaryOp, CastKind, TernaryOp, UnaryOp};
use std::ops::DerefMut;

use BinaryExprBuilder as BEB;
use CastExprBuilder as CEB;
use TernaryExprBuilder as TEB;
use UnaryExprBuilder as UEB;

pub(crate) trait FnH<I, O>: FnOnce(I) -> O {}
impl<I, O, F> FnH<I, O> for F where F: FnOnce(I) -> O {}

pub(crate) trait BinaryExprBuilderAdapter: DerefMut
where
    Self::Target: Sized,
    Self::Target: BinaryExprBuilder,
{
    type TargetExprRefPair<'a> = <Self::Target as BEB>::ExprRefPair<'a>;
    type TargetExpr<'a> = <Self::Target as BEB>::Expr<'a>;

    /// Takes the input from the type of the adapted operands, passes it to the wrapped builder,
    /// then converts and returns the output to the type of the adapted expression.
    /// [`build`] is the method in the wrapped builder that corresponds to the one called currently
    /// on this builder.
    fn adapt<'t, F>(operands: Self::TargetExprRefPair<'t>, build: F) -> Self::TargetExpr<'t>
    where
        F: for<'s> FnH<<Self::Target as BEB>::ExprRefPair<'s>, <Self::Target as BEB>::Expr<'s>>;
}

macro_rules_method_with_optional_args!(delegate_binary_op {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        fn $method<'a>(
            &mut self,
            operands: Self::ExprRefPair<'a>,
            $($arg: $arg_type),*
        ) -> Self::Expr<'a> {
            Self::adapt(operands, |operands| self.deref_mut().$method(operands, $($arg),*))
        }
    };
});

impl<T: BinaryExprBuilderAdapter> BinaryExprBuilder for T
where
    T::Target: Sized,
    T::Target: BinaryExprBuilder,
{
    type ExprRefPair<'a> = T::TargetExprRefPair<'a>;
    type Expr<'a> = T::TargetExpr<'a>;

    delegate_binary_op!(binary_op + op: BinaryOp);

    delegate_binary_op!(add add_unchecked add_with_overflow add_saturating);
    delegate_binary_op!(sub sub_unchecked sub_with_overflow sub_saturating);
    delegate_binary_op!(mul mul_unchecked mul_with_overflow);
    delegate_binary_op!(div div_exact rem);
    delegate_binary_op!(and or xor);
    delegate_binary_op!(shl shl_unchecked shr shr_unchecked);
    delegate_binary_op!(rotate_left rotate_right);
    delegate_binary_op!(eq ne lt le gt ge cmp);
    delegate_binary_op!(offset);
}

pub(crate) trait UnaryExprBuilderAdapter: DerefMut
where
    Self::Target: Sized,
    Self::Target: UnaryExprBuilder,
{
    type TargetExprRef<'a> = <Self::Target as UEB>::ExprRef<'a>;
    type TargetExpr<'a> = <Self::Target as UEB>::Expr<'a>;

    /// Takes the input from the type of the adapted operand, passes it to the wrapped builder,
    /// then converts and returns the output to the type of the adapted expression.
    /// [`build`] is the method in the wrapped builder that corresponds to the one called currently
    /// on this builder.
    fn adapt<'t, F>(operand: Self::TargetExprRef<'t>, build: F) -> Self::TargetExpr<'t>
    where
        F: for<'s> FnH<<Self::Target as UEB>::ExprRef<'s>, <Self::Target as UEB>::Expr<'s>>;
}

macro_rules_method_with_optional_args!(delegate_singular_unary_op {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        fn $method<'a>(
            &mut self,
            operand: Self::ExprRef<'a>,
            $($arg: $arg_type),*
        ) -> Self::Expr<'a> {
            Self::adapt(operand, |operand| self.deref_mut().$method(operand, $($arg),*))
        }
    };
});

impl<T: UnaryExprBuilderAdapter> UnaryExprBuilder for T
where
    T::Target: Sized,
    T::Target: UnaryExprBuilder,
{
    type ExprRef<'a> = T::TargetExprRef<'a>;
    type Expr<'a> = T::TargetExpr<'a>;

    delegate_singular_unary_op!(unary_op + op: UnaryOp);
    delegate_singular_unary_op!(no_op not neg ptr_metadata);
    delegate_singular_unary_op!(bit_reverse count_ones);
    delegate_singular_unary_op!(trailing_zeros + non_zero: bool);
    delegate_singular_unary_op!(leading_zeros + non_zero: bool);
}

pub(crate) trait TernaryExprBuilderAdapter: DerefMut
where
    Self::Target: Sized,
    Self::Target: TernaryExprBuilder,
{
    type TargetExprRefTriple<'a> = <Self::Target as TEB>::ExprRefTriple<'a>;
    type TargetExpr<'a> = <Self::Target as TEB>::Expr<'a>;

    /// Takes the input from the type of the adapted operands, passes it to the wrapped builder,
    /// then converts and returns the output to the type of the adapted expression.
    /// [`build`] is the method in the wrapped builder that corresponds to the one called currently
    /// on this builder.
    fn adapt<'t, F>(operands: Self::TargetExprRefTriple<'t>, build: F) -> Self::TargetExpr<'t>
    where
        F: for<'s> FnH<<Self::Target as TEB>::ExprRefTriple<'s>, <Self::Target as TEB>::Expr<'s>>;
}

macro_rules_method_with_optional_args!(delegate_ternary_op {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        fn $method<'a>(
            &mut self,
            operands: Self::ExprRefTriple<'a>,
            $($arg: $arg_type),*
        ) -> Self::Expr<'a> {
            Self::adapt(operands, |operands| self.deref_mut().$method(operands, $($arg),*))
        }
    };
});

impl<T: TernaryExprBuilderAdapter> TernaryExprBuilder for T
where
    T::Target: Sized,
    T::Target: TernaryExprBuilder,
{
    type ExprRefTriple<'a> = T::TargetExprRefTriple<'a>;
    type Expr<'a> = T::TargetExpr<'a>;

    delegate_ternary_op!(ternary_op + op: TernaryOp);

    delegate_ternary_op!(if_then_else);
}

pub(crate) trait CastExprBuilderAdapter: DerefMut
where
    Self::Target: Sized,
    Self::Target: CastExprBuilder,
{
    type TargetExprRef<'a> = <Self::Target as CEB>::ExprRef<'a>;
    type TargetExpr<'a> = <Self::Target as CEB>::Expr<'a>;

    /// Takes the input from the type of the adapted operand, passes it to the wrapped builder,
    /// then converts and returns the output to the type of the adapted expression.
    /// [`build`] is the method in the wrapped builder that corresponds to the one called currently
    /// on this builder.
    fn adapt<'t, F>(operand: Self::TargetExprRef<'t>, build: F) -> Self::TargetExpr<'t>
    where
        F: for<'s> FnH<<Self::Target as CEB>::ExprRef<'s>, <Self::Target as CEB>::Expr<'s>>;
}

macro_rules_method_with_optional_args!(delegate_singular_cast_op {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        fn $method<'a, 'b>(
            &mut self,
            operand: Self::ExprRef<'a>,
            $($arg: $arg_type,)*
            metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            Self::adapt(operand, |operand| self.deref_mut().$method(operand, $($arg,)* metadata,))
        }
    };
});

impl<T: CastExprBuilderAdapter> CastExprBuilder for T
where
    T::Target: Sized,
    T::Target: CastExprBuilder,
{
    type ExprRef<'a> = T::TargetExprRef<'a>;
    type Expr<'a> = T::TargetExpr<'a>;
    type Metadata<'a> = <T::Target as CEB>::Metadata<'a>;

    type IntType = <T::Target as CEB>::IntType;
    type FloatType = <T::Target as CEB>::FloatType;
    type PtrType = <T::Target as CEB>::PtrType;
    type GenericType = <T::Target as CEB>::GenericType;

    delegate_singular_cast_op!(
        cast + target: CastKind<Self::IntType, Self::FloatType, Self::PtrType, Self::GenericType>
    );
    delegate_singular_cast_op!(to_char);
    delegate_singular_cast_op!(to_int + ty: Self::IntType);
    delegate_singular_cast_op!(to_float + ty: Self::FloatType);
    delegate_singular_cast_op!(to_ptr + ty: Self::PtrType);
    delegate_singular_cast_op!(ptr_unsize expose_prov sized_dyn);
    delegate_singular_cast_op!(transmute + ty: Self::GenericType);
}
