use tracing::debug_span;

use std::fmt::Display;

use common::log_debug;

use super::{macros::macro_rules_method_with_optional_args, BinaryExprBuilder, UnaryExprBuilder};

pub(crate) const TAG: &str = "expr_builder";
const SPAN_BINARY: &str = "binary_op";
const SPAN_UNARY: &str = "unary_op";

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
                op = stringify!($method), operand = %operand)
            .entered();

            let result = self.builder.$method(operand, $($arg),*);

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

    fn binary_op<'a>(
        &mut self,
        operands: Self::ExprRefPair<'a>,
        op: crate::abs::BinaryOp,
    ) -> Self::Expr<'a> {
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
    impl_binary_expr_method!(offset);
}

impl<B> UnaryExprBuilder for LoggerExprBuilder<B>
where
    B: UnaryExprBuilder,
    for<'a> B::ExprRef<'a>: Display,
    for<'a> B::Expr<'a>: Display,
{
    type ExprRef<'a> = B::ExprRef<'a>;
    type Expr<'a> = B::Expr<'a>;

    impl_unary_expr_method!(unary_op + op: crate::abs::UnaryOp);

    impl_unary_expr_method!(not neg ptr_metadata);
    impl_unary_expr_method!(address_of len discriminant);
    impl_unary_expr_method!(cast + target: crate::abs::CastKind);
}