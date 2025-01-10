use std::marker::PhantomData;

use super::*;

/// A builder that first tries to build an expression using the current builder,
/// and if it fails (does not support building it), it tries to build it using
/// the next builder.
///
/// The current builder should return a `Result` that either is the built
/// expression (as `Ok`) or the expressions passed to it (as `Err`).
/// The builder may be a transformer over the expressions by returning an `Err`
/// containing the transformed version.
///
/// This struct is useful for overriding certain functions with optimizations, for example.
#[derive(Clone)]
pub(crate) struct ChainedExprBuilder<Current, Next, Expr, CurrentExpr: Into<Expr> = Expr> {
    pub(crate) current: Current,
    pub(crate) next: Next,
    _phantom: PhantomData<(Expr, CurrentExpr)>,
}

impl<C, N, E, CE: Into<E>> ChainedExprBuilder<C, N, E, CE> {
    pub(crate) fn new(current: C, next: N) -> Self {
        Self {
            current,
            next,
            _phantom: Default::default(),
        }
    }
}

impl<C, N, E, CE> Default for ChainedExprBuilder<C, N, E, CE>
where
    C: Default,
    N: Default,
    CE: Into<E>,
{
    fn default() -> Self {
        Self {
            current: Default::default(),
            next: Default::default(),
            _phantom: Default::default(),
        }
    }
}

macro_rules! try_on_current_then_next {
    /* If there's a single argument, it's straightforward. */
    ($self:ident, $method:ident, $arg:ident $(,)?) => {
        try_on_current_then_next!($self, $method, ($arg), |$arg|)
    };
    /* If there are multiple arguments, we need to take both the ones that
     * are passed back and the regular arguments. */
    ($self:ident, $method:ident, ($($args:expr),+), |$($err_args:tt),+| $(,)?) => {
        $self.current
            .$method($($args),*)
            .map(Into::<E>::into)
            .unwrap_or_else(|$($err_args),*| $self.next.$method($($args),*).into())
    };
}

macro_rules_method_with_optional_args!(impl_binary_expr_method {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        impl_binary_expr_method!($method + ($($arg : $arg_type { $arg }),*));
    };
    /* Gives the ability to give a custom expression passed to the builders. */
    ($method: ident + ($($arg: ident : $arg_type: ty { $arg_expr:expr }),*) $(,)?) => {
        fn $method<'a>(
            &mut self,
            operands: Self::ExprRefPair<'a>,
            $($arg: $arg_type),*
        ) -> Self::Expr<'a> {
            try_on_current_then_next!(self, $method, (operands$(, $arg_expr)*), |operands|)
        }
    };
});

macro_rules_method_with_optional_args!(impl_unary_expr_method {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        impl_unary_expr_method!($method + ($($arg : $arg_type { $arg }),*));
    };
    /* Gives the ability to give a custom expression passed to the builders. */
    ($method: ident + ($($arg: ident : $arg_type: ty { $arg_expr:expr }),*) $(,)?) => {
        fn $method<'a>(
            &mut self,
            operand: Self::ExprRef<'a>,
            $($arg: $arg_type),*
        ) -> Self::Expr<'a> {
            try_on_current_then_next!(self, $method, (operand$(, $arg_expr)*), |operand|)
        }
    };
});

macro_rules_method_with_optional_args!(impl_ternary_expr_method {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        impl_ternary_expr_method!($method + ($($arg : $arg_type { $arg }),*));
    };
    /* Gives the ability to give a custom expression passed to the builders. */
    ($method: ident + ($($arg: ident : $arg_type: ty { $arg_expr:expr }),*) $(,)?) => {
        fn $method<'a>(
            &mut self,
            operands: Self::ExprRefTriple<'a>,
            $($arg: $arg_type),*
        ) -> Self::Expr<'a> {
            try_on_current_then_next!(self, $method, (operands$(, $arg_expr)*), |operands|)
        }
    };
});

macro_rules_method_with_optional_args!(impl_cast_expr_method {
    ($method: ident + $($arg: ident : $arg_type: ty),* $(,)?) => {
        impl_cast_expr_method!($method + ($($arg : $arg_type { $arg }),*));
    };
    /* Gives the ability to give a custom expression passed to the builders. */
    ($method: ident + ($($arg: ident : $arg_type: ty { $arg_expr:expr }),*) $(,)?) => {
        fn $method<'a, 'b>(
            &mut self,
            operand: Self::ExprRef<'a>,
            $($arg: $arg_type,)*
            metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            try_on_current_then_next!(self, $method, (operand$(, $arg_expr)*, metadata.clone()), |operand|)
        }
    };
});

impl<C, N, E, CE> BinaryExprBuilder for ChainedExprBuilder<C, N, E, CE>
where
    N: BinaryExprBuilder,
    for<'a> C: BinaryExprBuilder<Expr<'a> = Result<CE, N::ExprRefPair<'a>>>,
    CE: Into<E>,
    for<'a> N::Expr<'a>: Into<E>,
{
    type ExprRefPair<'a> = C::ExprRefPair<'a>;
    type Expr<'a> = E;

    impl_binary_expr_method!(binary_op + op: BinaryOp);

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

impl<C, N, E, CE> UnaryExprBuilder for ChainedExprBuilder<C, N, E, CE>
where
    N: UnaryExprBuilder,
    for<'a> C: UnaryExprBuilder<Expr<'a> = Result<CE, N::ExprRef<'a>>>,
    CE: Into<E>,
    for<'a> N::Expr<'a>: Into<E>,
{
    type ExprRef<'a> = C::ExprRef<'a>;
    type Expr<'a> = E;

    impl_unary_expr_method!(unary_op + op: UnaryOp);

    impl_unary_expr_method!(no_op not neg ptr_metadata);
    impl_unary_expr_method!(bit_reverse count_ones);
    impl_unary_expr_method!(trailing_zeros + non_zero: bool);
    impl_unary_expr_method!(leading_zeros + non_zero: bool);
}

impl<C, N, E, CE> TernaryExprBuilder for ChainedExprBuilder<C, N, E, CE>
where
    N: TernaryExprBuilder,
    for<'a> C: TernaryExprBuilder<Expr<'a> = Result<CE, N::ExprRefTriple<'a>>>,
    CE: Into<E>,
    for<'a> N::Expr<'a>: Into<E>,
{
    type ExprRefTriple<'a> = C::ExprRefTriple<'a>;
    type Expr<'a> = E;

    impl_ternary_expr_method!(ternary_op + op: TernaryOp);

    impl_ternary_expr_method!(if_then_else);
}

impl<C, N, E, CE> CastExprBuilder for ChainedExprBuilder<C, N, E, CE>
where
    N: CastExprBuilder,
    for<'a> C: CastExprBuilder<
            ExprRef<'a> = N::ExprRef<'a>,
            Expr<'a> = Result<CE, N::ExprRef<'a>>,
            Metadata<'a> = N::Metadata<'a>,
            IntType = N::IntType,
            FloatType = N::FloatType,
            PtrType = N::PtrType,
            GenericType = N::GenericType,
        >,
    CE: Into<E>,
    for<'a> N::Expr<'a>: Into<E>,
    for<'a> N::Metadata<'a>: Clone,
    N::IntType: Copy,
    N::FloatType: Copy,
    N::PtrType: Copy,
    N::GenericType: Copy,
{
    type ExprRef<'a> = N::ExprRef<'a>;
    type Expr<'a> = E;
    type Metadata<'a> = N::Metadata<'a>;

    type IntType = N::IntType;
    type FloatType = N::FloatType;
    type PtrType = N::PtrType;
    type GenericType = N::GenericType;

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
