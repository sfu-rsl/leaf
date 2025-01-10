use delegate::delegate;

use crate::abs::{BinaryOp, UnaryOp};

use super::*;

/// This is the main expression builder, which is simply an interface for the
/// binary & unary expression builders.
#[derive(Clone, Default)]
pub(crate) struct CompositeExprBuilder<
    B: BinaryExprBuilder,
    U: UnaryExprBuilder,
    T: TernaryExprBuilder,
    C: CastExprBuilder,
> {
    pub(crate) binary: B,
    pub(crate) unary: U,
    pub(crate) ternary: T,
    pub(crate) cast: C,
}

macro_rules_method_with_optional_args!(impl_binary_expr_method {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        delegate! {
            to self.binary {
                fn $method<'a>(
                    &mut self,
                    operands: Self::ExprRefPair<'a>,
                    $($arg: $arg_type,)*
                ) -> Self::Expr<'a>;
            }
        }
    };
});

macro_rules_method_with_optional_args!(impl_unary_expr_method {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        delegate! {
            to self.unary {
                fn $method<'a>(
                    &mut self,
                    operand: Self::ExprRef<'a>,
                    $($arg: $arg_type,)*
                ) -> Self::Expr<'a>;
            }
        }
    };
});

macro_rules_method_with_optional_args!(impl_ternary_expr_method {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        delegate! {
            to self.ternary {
                fn $method<'a>(
                    &mut self,
                    operands: Self::ExprRefTriple<'a>,
                    $($arg: $arg_type,)*
                ) -> Self::Expr<'a>;
            }
        }
    };
});

macro_rules_method_with_optional_args!(impl_cast_expr_method {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        delegate! {
            to self.cast {
                fn $method<'a, 'b>(
                    &mut self,
                    operand: Self::ExprRef<'a>,
                    $($arg: $arg_type,)*
                    metadata: Self::Metadata<'b>,
                ) -> Self::Expr<'a>;
            }
        }
    };
});

impl<B, U, T, C> BinaryExprBuilder for CompositeExprBuilder<B, U, T, C>
where
    B: BinaryExprBuilder,
    U: UnaryExprBuilder,
    T: TernaryExprBuilder,
    C: CastExprBuilder,
{
    type ExprRefPair<'a> = B::ExprRefPair<'a>;
    type Expr<'a> = B::Expr<'a>;

    delegate! {
        to self.binary {
            fn binary_op<'a>(
                &mut self,
                operands: Self::ExprRefPair<'a>,
                op: BinaryOp,
            ) -> Self::Expr<'a>;
        }
    }

    impl_binary_expr_method!(add add_unchecked add_with_overflow add_saturating);
    impl_binary_expr_method!(sub sub_unchecked sub_with_overflow sub_saturating);
    impl_binary_expr_method!(mul mul_unchecked mul_with_overflow);
    impl_binary_expr_method!(div div_exact rem);
    impl_binary_expr_method!(and or xor);
    impl_binary_expr_method!(shl shl_unchecked shr shr_unchecked);
    impl_binary_expr_method!(rotate_left rotate_right);
    impl_binary_expr_method!(eq ne lt le gt ge cmp);
    impl_binary_expr_method!(offset);
}

impl<B, U, T, C> UnaryExprBuilder for CompositeExprBuilder<B, U, T, C>
where
    B: BinaryExprBuilder,
    U: UnaryExprBuilder,
    T: TernaryExprBuilder,
    C: CastExprBuilder,
{
    type ExprRef<'a> = U::ExprRef<'a>;
    type Expr<'a> = U::Expr<'a>;

    impl_unary_expr_method!(unary_op + op: UnaryOp);

    impl_unary_expr_method!(no_op not neg ptr_metadata);
    impl_unary_expr_method!(bit_reverse count_ones);
    impl_unary_expr_method!(trailing_zeros + non_zero: bool);
    impl_unary_expr_method!(leading_zeros + non_zero: bool);
}

impl<B, U, T, C> TernaryExprBuilder for CompositeExprBuilder<B, U, T, C>
where
    B: BinaryExprBuilder,
    U: UnaryExprBuilder,
    T: TernaryExprBuilder,
    C: CastExprBuilder,
{
    type ExprRefTriple<'a> = T::ExprRefTriple<'a>;
    type Expr<'a> = T::Expr<'a>;

    impl_ternary_expr_method!(ternary_op + op: TernaryOp);

    impl_ternary_expr_method!(if_then_else);
}

impl<B, U, T, C> CastExprBuilder for CompositeExprBuilder<B, U, T, C>
where
    B: BinaryExprBuilder,
    U: UnaryExprBuilder,
    T: TernaryExprBuilder,
    C: CastExprBuilder,
{
    type ExprRef<'a> = C::ExprRef<'a>;
    type Expr<'a> = C::Expr<'a>;
    type Metadata<'a> = C::Metadata<'a>;

    type IntType = C::IntType;
    type FloatType = C::FloatType;
    type PtrType = C::PtrType;
    type GenericType = C::GenericType;

    impl_cast_expr_method!(
        cast + target: CastKind<Self::IntType, Self::FloatType, Self::PtrType, Self::GenericType>
    );
    impl_cast_expr_method!(to_char);
    impl_cast_expr_method!(to_int + ty: Self::IntType);
    impl_cast_expr_method!(to_float + ty: Self::FloatType);
    impl_cast_expr_method!(to_ptr + ty: Self::PtrType);
    impl_cast_expr_method!(ptr_unsize expose_prov sized_dyn);
    impl_cast_expr_method!(transmute + ty: Self::GenericType);
}
