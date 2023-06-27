use delegate::delegate;

use crate::abs::{BinaryOp, UnaryOp};

use super::*;

/// This is the main expression builder, which is simply an interface for the
/// binary & unary expression builders.
#[derive(Default)]
pub(crate) struct CompositeExprBuilder<B: BinaryExprBuilder, U: UnaryExprBuilder> {
    pub(crate) binary: B,
    pub(crate) unary: U,
}

macro_rules! impl_binary_expr_method {
    ($($method:ident)*) => { 
        $(impl_binary_expr_method!($method +);)* 
    };
    ($($method:ident)* + $arg: ident : $arg_type: ty) => { 
        $(impl_binary_expr_method!($method + $arg: $arg_type,);)* 
    };
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        delegate! {
            to self.binary {
                fn $method<'a>(
                    &mut self,
                    operands: Self::ExprRefPair<'a>,
                    $($arg: $arg_type),*
                ) -> Self::Expr<'a>;
            }
        }
    };
}

macro_rules! impl_unary_expr_method {
    ($($method:ident)*) => { 
        $(impl_unary_expr_method!($method +);)* 
    };
    ($($method:ident)* + $arg: ident : $arg_type: ty) => { 
        $(impl_unary_expr_method!($method + $arg: $arg_type,);)* 
    };
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        delegate! {
            to self.unary {
                fn $method<'a>(
                    &mut self,
                    operand: Self::ExprRef<'a>,
                    $($arg: $arg_type),*
                ) -> Self::Expr<'a>;
            }
        }
    };
}

impl<B, U> BinaryExprBuilder for CompositeExprBuilder<B, U>
where
    B: BinaryExprBuilder,
    U: UnaryExprBuilder,
{
    type ExprRefPair<'a> = B::ExprRefPair<'a>;
    type Expr<'a> = B::Expr<'a>;

    delegate! {
        to self.binary {
            fn binary_op<'a>(
                &mut self,
                operands: Self::ExprRefPair<'a>,
                op: BinaryOp,
                checked: bool,
            ) -> Self::Expr<'a>;
        }
    }

    // note: this interface is more clear
    impl_binary_expr_method!(add sub mul + checked: bool);

    impl_binary_expr_method!(div rem);
    impl_binary_expr_method!(and or xor);
    impl_binary_expr_method!(shl shr);
    impl_binary_expr_method!(eq ne lt le gt ge);
    impl_binary_expr_method!(offset);
}

impl<B, U> UnaryExprBuilder for CompositeExprBuilder<B, U>
where
    B: BinaryExprBuilder,
    U: UnaryExprBuilder,
{
    type ExprRef<'a> = U::ExprRef<'a>;
    type Expr<'a> = U::Expr<'a>;

    impl_unary_expr_method!(unary_op + op: UnaryOp);

    impl_unary_expr_method!(not neg address_of len);
    
    impl_unary_expr_method!(cast + target: CastKind);
}
