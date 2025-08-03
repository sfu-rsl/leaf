use tracing::debug_span;

use std::fmt::Display;

use common::log_debug;

use crate::abs::CastKind;

use super::{macros::macro_rules_method_with_optional_args, *};

pub(crate) const TAG: &str = "expr_builder";
const SPAN_BINARY: &str = "binary_op";
const SPAN_UNARY: &str = "unary_op";
const SPAN_TERNARY: &str = "ternary_op";
const SPAN_CAST: &str = "cast_op";

#[derive(Clone, Default)]
pub(crate) struct LoggerExprBuilder<B> {
    pub(crate) builder: B,
}

macro_rules_method_with_optional_args!(impl_binary_expr_method {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        fn $method<'a>(
            &mut self,
            operands: Self::ExprRefPair<'a>,
            $($arg: $arg_type),*
        ) -> Self::Expr<'a>
        {
            let span = debug_span!(
                target: TAG, SPAN_BINARY,
                op = stringify!($method), operands = %operands)
            .entered();

            let result = self.builder.$method(operands, $($arg),*);

            log_debug!(target: TAG, expr = %result);
            span.exit();
            result
        }
    };
});

macro_rules_method_with_optional_args!(impl_unary_expr_method {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        fn $method<'a>(
            &mut self,
            operand: Self::ExprRef<'a>,
            $($arg: $arg_type),*
        ) -> Self::Expr<'a>
        {
            let span = debug_span!(
                target: TAG, SPAN_UNARY,
                op = stringify!($method), operand = %operand, $($arg = %$arg,)*)
            .entered();

            let result = self.builder.$method(operand, $($arg),*);

            log_debug!(target: TAG, expr = %result);
            span.exit();
            result
        }
    };
});

macro_rules_method_with_optional_args!(impl_ternary_expr_method {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        fn $method<'a>(
            &mut self,
            operands: Self::ExprRefTriple<'a>,
            $($arg: $arg_type),*
        ) -> Self::Expr<'a>
        {
            let span = debug_span!(
                target: TAG, SPAN_TERNARY,
                op = stringify!($method), operands = %operands)
            .entered();

            let result = self.builder.$method(operands, $($arg),*);

            log_debug!(target: TAG, expr = %result);
            span.exit();
            result
        }
    };
});

macro_rules_method_with_optional_args!(impl_cast_expr_method {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        fn $method<'a, 'b>(
            &mut self,
            operand: Self::ExprRef<'a>,
            $($arg: $arg_type,)*
            metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a>
        {
            let span = debug_span!(
                target: TAG, SPAN_CAST,
                kind = stringify!($method), operand = %operand, $($arg = %$arg,)* metadata = %metadata)
            .entered();

            let result = self.builder.$method(operand, $($arg,)* metadata,);

            log_debug!(target: TAG, expr = %result);
            span.exit();
            result
        }
    };
});

impl<B> BinaryExprBuilder for LoggerExprBuilder<B>
where
    B: BinaryExprBuilder,
    for<'a> B::ExprRefPair<'a>: Display,
    for<'a> B::Expr<'a>: Display,
{
    type ExprRefPair<'a> = B::ExprRefPair<'a>;
    type Expr<'a> = B::Expr<'a>;

    fn binary_op<'a>(&mut self, operands: Self::ExprRefPair<'a>, op: BinaryOp) -> Self::Expr<'a> {
        let span = debug_span!(
            target: TAG, SPAN_BINARY,
            op =  %op, operands = %operands)
        .entered();

        let result = self.builder.binary_op(operands, op);

        log_debug!(target: TAG, expr = %result);
        span.exit();
        result
    }

    impl_binary_expr_method!(add add_unchecked add_with_overflow add_saturating);
    impl_binary_expr_method!(sub sub_unchecked sub_with_overflow sub_saturating);
    impl_binary_expr_method!(mul mul_unchecked mul_with_overflow);
    impl_binary_expr_method!(div div_exact rem);
    impl_binary_expr_method!(and or xor);
    impl_binary_expr_method!(shl shl_unchecked shr shr_unchecked);
    impl_binary_expr_method!(rotate_left rotate_right);
    impl_binary_expr_method!(eq ne lt le gt ge cmp);
    impl_binary_expr_method!(offset + pointee_size: TypeSize);
}

impl<B> UnaryExprBuilder for LoggerExprBuilder<B>
where
    B: UnaryExprBuilder,
    for<'a> B::ExprRef<'a>: Display,
    for<'a> B::Expr<'a>: Display,
{
    type ExprRef<'a> = B::ExprRef<'a>;
    type Expr<'a> = B::Expr<'a>;

    impl_unary_expr_method!(unary_op + op: UnaryOp);

    impl_unary_expr_method!(no_op not neg ptr_metadata);
    impl_unary_expr_method!(bit_reverse count_ones byte_swap);
    impl_unary_expr_method!(trailing_zeros + non_zero: bool);
    impl_unary_expr_method!(leading_zeros + non_zero: bool);
}

impl<B> TernaryExprBuilder for LoggerExprBuilder<B>
where
    B: TernaryExprBuilder,
    for<'a> B::ExprRefTriple<'a>: Display,
    for<'a> B::Expr<'a>: Display,
{
    type ExprRefTriple<'a> = B::ExprRefTriple<'a>;
    type Expr<'a> = B::Expr<'a>;

    impl_ternary_expr_method!(ternary_op + op: TernaryOp);

    impl_ternary_expr_method!(if_then_else);
}

impl<B> CastExprBuilder for LoggerExprBuilder<B>
where
    B: CastExprBuilder,
    for<'a> B::ExprRef<'a>: Display,
    for<'a> B::Expr<'a>: Display,
    for<'a> B::Metadata<'a>: Display,
    B::IntType: Display,
    B::FloatType: Display,
    B::PtrType: Display,
    B::GenericType: Display,
{
    type ExprRef<'a> = B::ExprRef<'a>;
    type Expr<'a> = B::Expr<'a>;
    type Metadata<'a> = B::Metadata<'a>;

    type IntType = B::IntType;
    type FloatType = B::FloatType;
    type PtrType = B::PtrType;
    type GenericType = B::GenericType;

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
