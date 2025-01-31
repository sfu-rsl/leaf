use super::super::alias::{
    SymValueRefExprBuilder, TypeManager, ValueRefBinaryExprBuilder, ValueRefExprBuilder,
    ValueRefUnaryExprBuilder,
};
use super::{BinaryOp as BasicBinaryOp, UnaryOp as BasicUnaryOp, *};
use crate::abs::{
    BinaryOp as AbsBinaryOp, CastKind, TernaryOp as AbsTernaryOp, UnaryOp as AbsUnaryOp,
    expr::{
        BinaryExprBuilder, CastExprBuilder, ChainedExprBuilder, CompositeExprBuilder,
        LoggerExprBuilder, TernaryExprBuilder, UnaryExprBuilder, macros::*,
    },
};
use crate::utils::alias::RRef;

type Composite<Binary, Unary, Ternary, Cast> = CompositeExprBuilder<Binary, Unary, Ternary, Cast>;
type Chained<Current, Next, Expr = ValueRef, CurrentExpr = Expr> =
    ChainedExprBuilder<Current, Next, Expr, CurrentExpr>;
type Logger<B> = LoggerExprBuilder<B>;

type CastMetadata = LazyTypeInfo;

pub(crate) type DefaultExprBuilder = toplevel::TopLevelBuilder;
pub(crate) type DefaultSymExprBuilder =
    adapters::SymValueRefExprBuilderAdapter<toplevel::TopLevelBuilder>;

pub(crate) fn new_expr_builder(type_manager: Rc<dyn TypeManager>) -> DefaultExprBuilder {
    DefaultExprBuilder::new(type_manager)
}

pub(crate) fn to_sym_expr_builder(expr_builder: RRef<DefaultExprBuilder>) -> DefaultSymExprBuilder {
    adapters::SymValueRefExprBuilderAdapter(expr_builder)
}

impl ValueRefExprBuilder for DefaultExprBuilder {}

impl ValueRefBinaryExprBuilder for DefaultExprBuilder {}

impl ValueRefUnaryExprBuilder for DefaultExprBuilder {}

mod toplevel {
    use super::{concrete::ConcreteBuilder, symbolic::SymbolicBuilder, *};

    /// An expression builder that separates the path for expressions that involve symbolic values,
    /// or the ones that are fully based on concrete values.
    /// NOTE: In an ideal case, fully concrete expressions should not be asked to created. So in
    /// the future, this top-level builder will be reduced to the symbolic builder.
    pub(crate) struct TopLevelBuilder {
        sym_builder: SymbolicBuilder,
        conc_builder: ConcreteBuilder,
    }

    impl TopLevelBuilder {
        pub(crate) fn new(type_manager: Rc<dyn TypeManager>) -> Self {
            Self {
                sym_builder: SymbolicBuilder::new(type_manager.clone()),
                conc_builder: ConcreteBuilder::default(),
            }
        }
    }

    impl BinaryExprBuilder for TopLevelBuilder {
        type ExprRefPair<'a> = (ValueRef, ValueRef);
        type Expr<'a> = ValueRef;

        fn binary_op<'a>(
            &mut self,
            (first, second): Self::ExprRefPair<'a>,
            op: AbsBinaryOp,
        ) -> Self::Expr<'a> {
            if first.is_symbolic() {
                self.sym_builder.binary_op(
                    BinaryOperands::Orig {
                        first: SymValueRef::new(first),
                        second,
                    },
                    op,
                )
            } else if second.is_symbolic() {
                self.sym_builder.binary_op(
                    BinaryOperands::Rev {
                        first,
                        second: SymValueRef::new(second),
                    },
                    op,
                )
            } else {
                self.conc_builder.binary_op(
                    (ConcreteValueRef::new(first), ConcreteValueRef::new(second)),
                    op,
                )
            }
        }

        impl_singular_binary_ops_through_general!();
    }

    impl UnaryExprBuilder for TopLevelBuilder {
        type ExprRef<'a> = ValueRef;
        type Expr<'a> = ValueRef;

        fn unary_op<'a>(&mut self, operand: Self::ExprRef<'a>, op: AbsUnaryOp) -> Self::Expr<'a> {
            if operand.is_symbolic() {
                self.sym_builder
                    .unary_op(SymValueRef::new(operand).into(), op)
                    .into()
            } else {
                self.conc_builder
                    .unary_op(ConcreteValueRef::new(operand).into(), op)
            }
        }

        impl_singular_unary_ops_through_general!();
    }

    impl TernaryExprBuilder for TopLevelBuilder {
        type ExprRefTriple<'a> = (ValueRef, ValueRef, ValueRef);
        type Expr<'a> = ValueRef;

        fn ternary_op<'a>(
            &mut self,
            operands: Self::ExprRefTriple<'a>,
            op: AbsTernaryOp,
        ) -> Self::Expr<'a> {
            if operands.0.is_symbolic() || operands.1.is_symbolic() || operands.2.is_symbolic() {
                self.sym_builder
                    .ternary_op(
                        SymTernaryOperands::new(operands.0, operands.1, operands.2),
                        op,
                    )
                    .into()
            } else {
                self.conc_builder.ternary_op(
                    (
                        ConcreteValueRef::new(operands.0),
                        ConcreteValueRef::new(operands.1),
                        ConcreteValueRef::new(operands.2),
                    ),
                    op,
                )
            }
        }

        impl_singular_ternary_ops_through_general!();
    }

    impl CastExprBuilder for TopLevelBuilder {
        type ExprRef<'a> = ValueRef;
        type Expr<'a> = ValueRef;
        type Metadata<'a> = CastMetadata;

        fn cast<'a, 'b>(
            &mut self,
            operand: Self::ExprRef<'a>,
            target: CastKind<Self::IntType, Self::FloatType, Self::PtrType, Self::GenericType>,
            metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            if operand.is_symbolic() {
                self.sym_builder
                    .cast(SymValueRef::new(operand).into(), target, metadata)
                    .into()
            } else {
                self.conc_builder
                    .cast(ConcreteValueRef::new(operand).into(), target, metadata)
            }
        }

        impl_singular_casts_through_general!();
    }
}

mod symbolic {
    use crate::backends::basic::alias::SymValueRefExprBuilder;

    use super::{
        adapters::{ConstFolder, ConstSimplifier, CoreBuilder},
        *,
    };

    pub(crate) type BaseSymbolicBuilder = Composite<
        /*Binary:*/
        Chained<ConstSimplifier, Chained<ConstFolder, CoreBuilder>>,
        /*Unary:*/
        CoreBuilder,
        /*Ternary:*/
        Chained<ConstSimplifier, CoreBuilder>,
        /*Cast:*/
        CoreBuilder,
    >;

    impl SymValueRefExprBuilder for Logger<BaseSymbolicBuilder> {}

    pub(crate) type SymbolicBuilder = Logger<
        Chained<UnevaluatedResolverBuilder<Logger<BaseSymbolicBuilder>>, BaseSymbolicBuilder>,
    >;

    impl SymbolicBuilder {
        pub(crate) fn new(type_manager: Rc<dyn TypeManager>) -> Self {
            let uneval_resolver = UnevaluatedResolverBuilder {
                type_manager,
                sym_builder: Logger::<BaseSymbolicBuilder>::default(),
            };
            Logger {
                builder: Chained::new(uneval_resolver, BaseSymbolicBuilder::default()),
            }
        }
    }

    #[derive(Clone)]
    pub(crate) struct UnevaluatedResolverBuilder<EB: SymValueRefExprBuilder> {
        type_manager: Rc<dyn TypeManager>,
        sym_builder: EB,
    }

    impl<EB: SymValueRefExprBuilder> UnevaluatedResolverBuilder<EB> {
        fn resolve(&mut self, value: &mut ValueRef, expect_scalar: bool) {
            match value.as_ref() {
                Value::Concrete(_) => self.resolve_conc(value, expect_scalar),
                Value::Symbolic(..) => self.resolve_sym(value, true),
            }
        }

        fn resolve_conc(&mut self, value: &mut ValueRef, expect_scalar: bool) {
            match value.as_ref() {
                Value::Concrete(ConcreteValue::Unevaluated(UnevalValue::Lazy(lazy))) => {
                    match unsafe { lazy.try_retrieve_as_scalar(self.type_manager.as_ref()) } {
                        Ok(retrieved) => {
                            *value = retrieved.to_value_ref();
                        }
                        Err(type_info) if expect_scalar => {
                            panic!(
                                "Expected the value to be retrievable as a scalar: {:?}: {:?}",
                                value, type_info,
                            );
                        }
                        _ => (),
                    }
                }
                _ => {}
            }
        }

        fn resolve_sym(&mut self, value: &mut ValueRef, expect_scalar: bool) {
            match value.as_ref() {
                Value::Symbolic(SymValue::Expression(Expr::Partial(porter))) => {
                    match porter
                        .try_to_masked_value(self.type_manager.as_ref(), &mut self.sym_builder)
                    {
                        Ok(resolved) => {
                            *value = resolved.into();
                        }
                        Err(type_info) if expect_scalar => {
                            panic!(
                                "Porter value participating in an expression is expected to be convertible to masked value {:?}: {:?}",
                                value, type_info,
                            );
                        }
                        _ => (),
                    }
                }
                _ => (),
            }
        }
    }

    impl<EB: SymValueRefExprBuilder> BinaryExprBuilder for UnevaluatedResolverBuilder<EB> {
        type ExprRefPair<'a> = SymBinaryOperands;
        type Expr<'a> = Result<ValueRef, SymBinaryOperands>;

        fn binary_op<'a>(
            &mut self,
            mut operands: Self::ExprRefPair<'a>,
            _op: AbsBinaryOp,
        ) -> Self::Expr<'a> {
            let other = operands.as_flat_mut().1;
            self.resolve(other, true);

            let sym = operands.as_flat_mut().0;
            self.resolve_sym(sym.as_mut(), true);

            Err(operands)
        }

        impl_singular_binary_ops_through_general!();
    }

    impl<EB: SymValueRefExprBuilder> UnaryExprBuilder for UnevaluatedResolverBuilder<EB> {
        type ExprRef<'a> = SymValueRef;
        type Expr<'a> = Result<ValueRef, SymValueRef>;

        fn unary_op<'a>(
            &mut self,
            mut operand: Self::ExprRef<'a>,
            _op: AbsUnaryOp,
        ) -> Self::Expr<'a> {
            self.resolve_sym(operand.as_mut(), true);
            Err(operand)
        }

        impl_singular_unary_ops_through_general!();
    }

    impl<EB: SymValueRefExprBuilder> TernaryExprBuilder for UnevaluatedResolverBuilder<EB> {
        type ExprRefTriple<'a> = SymTernaryOperands;
        type Expr<'a> = Result<ValueRef, SymTernaryOperands>;

        fn ternary_op<'a>(
            &mut self,
            mut operands: Self::ExprRefTriple<'a>,
            op: AbsTernaryOp,
        ) -> Self::Expr<'a> {
            let expect_scalar = match op {
                AbsTernaryOp::IfThenElse => (true, false, false),
            };
            self.resolve(&mut operands.0, expect_scalar.0);
            self.resolve(&mut operands.1, expect_scalar.1);
            self.resolve(&mut operands.2, expect_scalar.2);

            Err(operands)
        }

        impl_singular_ternary_ops_through_general!();
    }

    impl<EB: SymValueRefExprBuilder> CastExprBuilder for UnevaluatedResolverBuilder<EB> {
        type ExprRef<'a> = SymValueRef;
        type Expr<'a> = Result<ValueRef, SymValueRef>;
        type Metadata<'a> = CastMetadata;

        fn cast<'a, 'b>(
            &mut self,
            mut operand: Self::ExprRef<'a>,
            target: CastKind<Self::IntType, Self::FloatType, Self::PtrType, Self::GenericType>,
            metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            if let SymValue::Expression(Expr::Partial(..)) = operand.as_ref() {
                match target {
                    CastKind::Transmute(_dst_ty) => {
                        let SymValue::Expression(Expr::Partial(porter)) =
                            SymValueRef::make_mut(&mut operand)
                        else {
                            unreachable!()
                        };
                        porter.as_concrete.1 = metadata;
                        return Ok(operand.into());
                    }
                    _ if ValueType::try_from(&metadata).is_ok() => {
                        self.resolve_sym(operand.as_mut(), true);
                    }
                    _ => {}
                }
            }
            Err(operand)
        }

        impl_singular_casts_through_general!();
    }
}

mod adapters {
    use derive_more::{Deref, DerefMut};
    use guards::SymTernaryOperands;

    use crate::abs::expr::variance::*;

    use super::*;

    use BinaryExprBuilder as BEB;
    use CastExprBuilder as CEB;
    use TernaryExprBuilder as TEB;
    use UnaryExprBuilder as UEB;

    #[derive(Default, Clone, Deref, DerefMut)]
    pub(crate) struct CoreBuilder(core::CoreBuilder);

    impl BinaryExprBuilderAdapter for CoreBuilder {
        type TargetExprRefPair<'a> = SymBinaryOperands;
        type TargetExpr<'a> = ValueRef;

        #[inline]
        fn adapt<'t, F>(operands: Self::TargetExprRefPair<'t>, build: F) -> Self::TargetExpr<'t>
        where
            F: for<'s> FnH<<Self::Target as BEB>::ExprRefPair<'s>, <Self::Target as BEB>::Expr<'s>>,
        {
            build(operands).to_value_ref()
        }
    }

    impl UnaryExprBuilderAdapter for CoreBuilder {
        type TargetExprRef<'a> = SymValueRef;
        type TargetExpr<'a> = ValueRef;

        #[inline]
        fn adapt<'t, F>(operand: Self::TargetExprRef<'t>, build: F) -> Self::TargetExpr<'t>
        where
            F: for<'s> FnH<<Self::Target as UEB>::ExprRef<'s>, <Self::Target as UEB>::Expr<'s>>,
        {
            build(operand).into()
        }
    }

    impl TernaryExprBuilderAdapter for CoreBuilder {
        type TargetExprRefTriple<'a> = (SymValueRef, ValueRef, ValueRef);
        type TargetExpr<'a> = ValueRef;

        #[inline]
        fn adapt<'t, F>(operands: Self::TargetExprRefTriple<'t>, build: F) -> Self::TargetExpr<'t>
        where
            F: for<'s> FnH<
                    <Self::Target as TEB>::ExprRefTriple<'s>,
                    <Self::Target as TEB>::Expr<'s>,
                >,
        {
            Value::from(build(operands)).to_value_ref()
        }
    }

    impl CastExprBuilderAdapter for CoreBuilder {
        type TargetExprRef<'a> = SymValueRef;
        type TargetExpr<'a> = SymValueRef;

        #[inline]
        fn adapt<'t, F>(operand: Self::TargetExprRef<'t>, build: F) -> Self::TargetExpr<'t>
        where
            F: for<'s> FnH<<Self::Target as CEB>::ExprRef<'s>, <Self::Target as CEB>::Expr<'s>>,
        {
            build(operand)
        }
    }

    /// This expression builder is an optimization step that skips generating expressions
    /// when the answer is deterministic based on arithmetics and logics
    ///
    /// For example, when generating an expression for `x * 1`, `mul(x, 1)` will be called,
    /// and ConstSimplifier will return only `x`, not `Expr::Binary { ... }`.
    #[derive(Default, Clone, Deref, DerefMut)]
    pub(crate) struct ConstSimplifier(simp::ConstSimplifier);

    impl BinaryExprBuilderAdapter for ConstSimplifier {
        type TargetExprRefPair<'a> = SymBinaryOperands;
        type TargetExpr<'a> = Result<ValueRef, SymBinaryOperands>;

        fn adapt<'t, F>(operands: Self::TargetExprRefPair<'t>, build: F) -> Self::TargetExpr<'t>
        where
            F: for<'s> FnH<<Self::Target as BEB>::ExprRefPair<'s>, <Self::Target as BEB>::Expr<'s>>,
        {
            let (first, second, is_reversed) = operands.flatten();
            let first = match second.as_ref() {
                Value::Concrete(ConcreteValue::Const(value)) => {
                    match build((first, value, is_reversed).into()) {
                        Ok(result) => Ok(result),
                        Err(operands) => Err(operands.flatten().0),
                    }
                }
                _ => Err(first),
            };

            first.map_err(|first| (first, second, is_reversed).into())
        }
    }

    impl TernaryExprBuilderAdapter for ConstSimplifier {
        type TargetExprRefTriple<'a> = SymTernaryOperands;
        type TargetExpr<'a> = Result<ValueRef, (SymValueRef, ValueRef, ValueRef)>;

        fn adapt<'t, F>(operands: Self::TargetExprRefTriple<'t>, build: F) -> Self::TargetExpr<'t>
        where
            F: for<'s> FnH<
                    <Self::Target as TernaryExprBuilder>::ExprRefTriple<'s>,
                    <Self::Target as TernaryExprBuilder>::Expr<'s>,
                >,
        {
            let SymTernaryOperands(cond, if_target, else_target) = operands;
            match cond.as_ref() {
                Value::Concrete(cond) => match cond {
                    ConcreteValue::Const(cond) => Ok(build((cond, if_target, else_target))),
                    _ => panic!("Unexpected non-constant value for the condition {:?}", cond),
                },
                _ => Err((SymValueRef::new(cond), if_target, else_target)),
            }
        }
    }

    /// This expression builder is an optimization step that folds constants in
    /// the expressions if possible and avoids creating a new expression.
    ///
    /// For example, when generating an expression for `x * 2 * 3`,
    /// `mul(mul(x, 1), 2)` will be called, and ConstFolder will return the
    /// result equivalent to `x * 6` instead of ((x * 2) * 3).
    #[derive(Default, Clone, Deref, DerefMut)]
    pub(crate) struct ConstFolder(simp::ConstFolder);

    impl BinaryExprBuilderAdapter for ConstFolder {
        type TargetExprRefPair<'a> = SymBinaryOperands;
        type TargetExpr<'a> = Result<ValueRef, SymBinaryOperands>;

        fn adapt<'t, F>(operands: Self::TargetExprRefPair<'t>, build: F) -> Self::TargetExpr<'t>
        where
            F: for<'s> FnH<<Self::Target as BEB>::ExprRefPair<'s>, <Self::Target as BEB>::Expr<'s>>,
        {
            /* If this is a binary operation between a constant and a binary
             * expression with a constant operand.
             */
            let (first, second, is_reversed) = operands.as_flat();
            if let Value::Concrete(ConcreteValue::Const(b)) = second.as_ref() {
                if let SymValue::Expression(Expr::Binary(first)) = first.as_ref() {
                    let (x, a, is_inner_reversed) = first.operands.as_flat();
                    if let Value::Concrete(ConcreteValue::Const(a)) = a.as_ref() {
                        if let Ok(result) = build(
                            (
                                BinaryExpr {
                                    operands: (x, a, is_inner_reversed).into(),
                                    operator: first.operator,
                                },
                                b,
                                is_reversed,
                            )
                                .into(),
                        ) {
                            return Ok(result.to_value_ref().0);
                        }
                    }
                }
            }

            Err(operands)
        }
    }

    #[derive(Default, Clone, Deref, DerefMut)]
    pub(crate) struct SymValueRefExprBuilderAdapter<T: ValueRefExprBuilder>(pub(super) RRef<T>);

    impl<T: ValueRefExprBuilder> BinaryExprBuilder for SymValueRefExprBuilderAdapter<T> {
        type ExprRefPair<'a> = SymBinaryOperands;
        type Expr<'a> = ValueRef;

        fn binary_op<'a>(
            &mut self,
            operands: Self::ExprRefPair<'a>,
            op: AbsBinaryOp,
        ) -> Self::Expr<'a> {
            let operands = match operands {
                BinaryOperands::Orig { first, second } => (first.into(), second),
                BinaryOperands::Rev { first, second } => (first, second.into()),
            };
            self.0.borrow_mut().binary_op(operands, op)
        }

        impl_singular_binary_ops_through_general!();
    }

    impl<T: ValueRefExprBuilder> UnaryExprBuilder for SymValueRefExprBuilderAdapter<T> {
        type ExprRef<'a> = SymValueRef;
        type Expr<'a> = ValueRef;

        #[inline]
        fn unary_op<'a>(&mut self, operand: Self::ExprRef<'a>, op: AbsUnaryOp) -> Self::Expr<'a> {
            self.0.borrow_mut().unary_op(operand.into(), op)
        }

        impl_singular_unary_ops_through_general!();
    }

    impl<T: ValueRefExprBuilder> TernaryExprBuilder for SymValueRefExprBuilderAdapter<T> {
        type ExprRefTriple<'a> = SymTernaryOperands;
        type Expr<'a> = ValueRef;

        #[inline]
        fn ternary_op<'a>(
            &mut self,
            operands: Self::ExprRefTriple<'a>,
            op: AbsTernaryOp,
        ) -> Self::Expr<'a> {
            self.0.borrow_mut().ternary_op(operands.into(), op)
        }

        impl_singular_ternary_ops_through_general!();
    }

    impl<T: ValueRefExprBuilder> CastExprBuilder for SymValueRefExprBuilderAdapter<T> {
        type ExprRef<'a> = SymValueRef;
        type Expr<'a> = SymValueRef;
        type Metadata<'a> = CastMetadata;

        fn cast<'a, 'b>(
            &mut self,
            operand: Self::ExprRef<'a>,
            target: CastKind<Self::IntType, Self::FloatType, Self::PtrType, Self::GenericType>,
            metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            SymValueRef::new(self.0.borrow_mut().cast(operand.into(), target, metadata))
        }

        impl_singular_casts_through_general!();
    }

    impl<T: ValueRefExprBuilder> SymValueRefExprBuilder for SymValueRefExprBuilderAdapter<T> {}
}

mod core {
    use abs::expr::CastExprBuilder;
    use common::utils::type_id_of;

    use simp::CastSimplifier;

    use super::*;
    use std::ops::Not;

    /// This is the base expression builder. It implements the lowest level for
    /// all the binary and unary functions. At this point all optimizations are
    /// considered to be done, so now actual symbolic expressions can be built
    #[derive(Default, Clone)]
    pub(crate) struct CoreBuilder;

    impl BinaryExprBuilder for CoreBuilder {
        type ExprRefPair<'a> = SymBinaryOperands;
        // NOTE: We have to generalize it to value because of overflow checks which generate a tuple.
        type Expr<'a> = Value;

        fn binary_op<'a>(
            &mut self,
            operands: Self::ExprRefPair<'a>,
            op: AbsBinaryOp,
        ) -> Self::Expr<'a> {
            if op.is_with_overflow() {
                let wrapping_op = match op {
                    AbsBinaryOp::AddWithOverflow => BasicBinaryOp::Add,
                    AbsBinaryOp::SubWithOverflow => BasicBinaryOp::Sub,
                    AbsBinaryOp::MulWithOverflow => BasicBinaryOp::Mul,
                    _ => unreachable!(),
                };
                self.with_overflow_op(operands, wrapping_op).into()
            } else if op.is_unchecked() {
                let wrapping_op = match op {
                    AbsBinaryOp::AddUnchecked => AbsBinaryOp::Add,
                    AbsBinaryOp::SubUnchecked => AbsBinaryOp::Sub,
                    AbsBinaryOp::MulUnchecked => AbsBinaryOp::Mul,
                    AbsBinaryOp::DivExact => AbsBinaryOp::Div,
                    _ => unreachable!(),
                };
                // FIXME: #197
                self.binary_op(operands, wrapping_op)
            } else if op.is_saturating() {
                let wrapping_op = match op {
                    AbsBinaryOp::AddSaturating => BasicBinaryOp::Add,
                    AbsBinaryOp::SubSaturating => BasicBinaryOp::Sub,
                    _ => unreachable!(),
                };
                self.saturating_op(operands, wrapping_op).into()
            } else {
                Expr::Binary(BinaryExpr {
                    operator: op.try_into().unwrap(),
                    operands,
                })
                .into()
            }
        }

        impl_singular_binary_ops_through_general!();
    }

    impl CoreBuilder {
        fn with_overflow_op(
            &mut self,
            operands: SymBinaryOperands,
            wrapping_op: BasicBinaryOp,
        ) -> AdtValue {
            let (make_check_expr_if_possible, wrapping_value, _) =
                self.break_down_for_overflow(operands, wrapping_op);

            let overflow_expr = make_check_expr_if_possible(true);
            let underflow_expr = make_check_expr_if_possible(false);

            let check_expr = match (overflow_expr, underflow_expr) {
                (Some(overflow), Some(underflow)) => self.binary_op(
                    (overflow.to_value_ref(), underflow.to_value_ref().0).into(),
                    AbsBinaryOp::BitOr,
                ),
                (Some(overflow), None) => overflow.into(),
                (None, Some(underflow)) => underflow.into(),
                (None, None) => unreachable!(),
            };

            AdtValue {
                kind: AdtKind::Struct,
                fields: vec![
                    Some(wrapping_value.to_value_ref().into()).into(),
                    Some(check_expr.to_value_ref()).into(),
                ],
            }
        }

        fn saturating_op(
            &mut self,
            operands: SymBinaryOperands,
            wrapping_op: BasicBinaryOp,
        ) -> Expr {
            let (make_check_expr_if_possible, wrapping_value, ty) =
                self.break_down_for_overflow(operands, wrapping_op);
            // (Sign-extended bit representation)
            let min_of_ty = Wrapping(
                u128::MAX.wrapping_shl(ty.bit_size as u32 - (if ty.is_signed { 1 } else { 0 })),
            );

            let value = wrapping_value;
            let value = if let Some(overflow_expr) = make_check_expr_if_possible(true) {
                let max_value = ConstValue::Int {
                    bit_rep: min_of_ty.not(),
                    ty,
                };
                Expr::Ite {
                    condition: overflow_expr.to_value_ref(),
                    if_target: max_value.to_value_ref(),
                    else_target: value.to_value_ref().into(),
                }
            } else {
                value
            };

            let value = if let Some(underflow_expr) = make_check_expr_if_possible(false) {
                let min_value = ConstValue::Int {
                    bit_rep: min_of_ty,
                    ty,
                };
                Expr::Ite {
                    condition: underflow_expr.to_value_ref(),
                    if_target: min_value.to_value_ref(),
                    else_target: value.to_value_ref().into(),
                }
            } else {
                value
            };

            value
        }

        fn break_down_for_overflow(
            &mut self,
            operands: SymBinaryOperands,
            wrapping_op: BasicBinaryOp,
        ) -> (impl Fn(bool) -> Option<Expr>, Expr, IntType) {
            let op: OverflowingBinaryOp = wrapping_op.try_into().unwrap();
            let Ok(ValueType::Int(ty)) = ValueType::try_from(operands.as_flat().0.value()) else {
                unreachable!("Only integer types are expected for overflowing operations.")
            };
            let Value::Symbolic(SymValue::Expression(wrapping_expr)) =
                self.binary_op(operands.clone(), wrapping_op.into())
            else {
                unreachable!()
            };
            let make_check_expr_if_possible = move |is_overflow| {
                if op.is_possible(is_overflow, ty.is_signed) {
                    Some(Expr::BinaryBoundCheck {
                        bin_expr: BinaryExpr {
                            operator: op,
                            operands: operands.clone(),
                        },
                        is_overflow,
                    })
                } else {
                    None
                }
            };
            (make_check_expr_if_possible, wrapping_expr, ty)
        }
    }

    impl UnaryExprBuilder for CoreBuilder {
        type ExprRef<'a> = SymValueRef;
        type Expr<'a> = SymValueRef;

        fn unary_op<'a>(&mut self, operand: Self::ExprRef<'a>, op: AbsUnaryOp) -> Self::Expr<'a> {
            use AbsUnaryOp::*;
            match op {
                NoOp => operand,
                Neg | Not | BitReverse | TrailingZeros | NonZeroTrailingZeros | LeadingZeros
                | NonZeroLeadingZeros | CountOnes => Expr::Unary {
                    operator: op.try_into().unwrap(),
                    operand: operand,
                }
                .to_value_ref(),
                PtrMetadata => Expr::PtrMetadata(operand.into()).to_value_ref(),
            }
        }

        impl_singular_unary_ops_through_general!();
    }

    impl TernaryExprBuilder for CoreBuilder {
        type ExprRefTriple<'a> = (SymValueRef, ValueRef, ValueRef);
        type Expr<'a> = Expr;

        fn if_then_else<'a>(&mut self, operands: Self::ExprRefTriple<'a>) -> Self::Expr<'a> {
            Expr::Ite {
                condition: operands.0,
                if_target: operands.1,
                else_target: operands.2,
            }
        }

        impl_general_ternary_op_through_singulars!();
    }

    impl CastExprBuilder for CoreBuilder {
        type ExprRef<'a> = SymValueRef;
        type Expr<'a> = SymValueRef;
        type Metadata<'a> = CastMetadata;

        type IntType = IntType;
        type FloatType = FloatType;
        type PtrType = TypeId;
        type GenericType = TypeId;

        impl_general_cast_through_singulars!();

        fn to_char<'a, 'b>(
            &mut self,
            operand: Self::ExprRef<'a>,
            _metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            debug_assert!(
                ValueType::try_from(operand.value()).map_or(true, |ty| ty == IntType::U8.into(),),
                // https://doc.rust-lang.org/reference/expressions/operator-expr.html#type-cast-expressions
                "Cast to char is only expected from u8."
            );
            self.extend(operand, IntType::U8, ValueType::Char)
        }

        fn to_int<'a, 'b>(
            &mut self,
            operand: Self::ExprRef<'a>,
            ty @ IntType {
                bit_size,
                is_signed,
            }: Self::IntType,
            metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            let from_type = ValueType::try_from(operand.value());
            if let Ok(from_type) = from_type {
                match from_type {
                    ValueType::Bool => Expr::Ite {
                        condition: operand,
                        if_target: ConstValue::new_int(1 as u128, IntType {
                            bit_size,
                            is_signed,
                        })
                        .to_value_ref(),
                        else_target: ConstValue::new_int(0 as u128, IntType {
                            bit_size,
                            is_signed,
                        })
                        .to_value_ref(),
                    }
                    .to_value_ref(),
                    ValueType::Char => {
                        let code_point = self.transmute(
                            operand,
                            // FIXME: This is not necessarily the correct type id as the runtime library is built separately.
                            type_id_of::<u32>(),
                            LazyTypeInfo::IdPrimitive(type_id_of::<u32>(), IntType::U32.into()),
                        );
                        self.to_int(code_point, ty, metadata)
                    }
                    ValueType::Int(
                        from_ty @ IntType {
                            bit_size: from_bit_size,
                            ..
                        },
                    ) => {
                        if bit_size == from_bit_size {
                            if ::core::intrinsics::likely(from_ty != ty) {
                                self.transmute(operand, metadata.id().unwrap(), metadata)
                            } else {
                                operand
                            }
                        } else if bit_size > from_bit_size {
                            self.extend(operand, from_ty, ty.into())
                        } else {
                            self.truncate(operand, ty)
                        }
                    }
                    ValueType::Float { .. } => todo!(),
                }
            }
            // Special case for u8 as we don't really need the source type for it.
            else if ty == IntType::U8 {
                self.truncate(operand, ty)
            } else {
                panic!(
                    "Could not determine the type of the operand for int cast: {:?}",
                    operand,
                )
            }
        }

        fn to_float<'a, 'b>(
            &mut self,
            _operand: Self::ExprRef<'a>,
            _ty: Self::FloatType,
            _metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            todo!()
        }

        fn to_ptr<'a, 'b>(
            &mut self,
            _operand: Self::ExprRef<'a>,
            _ty: Self::PtrType,
            _metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            todo!()
        }

        fn ptr_unsize<'a, 'b>(
            &mut self,
            _operand: Self::ExprRef<'a>,
            _metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            todo!("#331: Add support for casting symbolic pointers")
        }

        fn expose_prov<'a, 'b>(
            &mut self,
            _operand: Self::ExprRef<'a>,
            _metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            todo!("#331: Add support for casting symbolic pointers")
        }

        fn sized_dyn<'a, 'b>(
            &mut self,
            _operand: Self::ExprRef<'a>,
            _metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            todo!("#331: Add support for casting symbolic pointers")
        }

        fn transmute<'a, 'b>(
            &mut self,
            operand: Self::ExprRef<'a>,
            _ty: Self::GenericType,
            metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            self.trans(operand, metadata)
        }
    }

    impl CoreBuilder {
        fn extend(
            &mut self,
            operand: SymValueRef,
            from_ty: IntType,
            dst_ty: ValueType,
        ) -> SymValueRef {
            let is_zero_extension = !from_ty.is_signed;
            let bits_to_add =
                NonZeroU32::new((dst_ty.bit_size().unwrap().get() - from_ty.bit_size) as u32)
                    .unwrap();

            CastSimplifier::extend(
                CastSimplifier::peel_transmute(&operand),
                is_zero_extension,
                bits_to_add,
                dst_ty,
            )
            .to_value_ref()
        }

        fn truncate(&mut self, operand: SymValueRef, dst_ty: IntType) -> SymValueRef {
            CastSimplifier::truncate(CastSimplifier::peel_transmute(&operand), dst_ty)
        }

        fn trans(&mut self, operand: SymValueRef, dst_ty: LazyTypeInfo) -> SymValueRef {
            if Option::zip(
                ValueType::try_from(&dst_ty).ok(),
                ValueType::try_from(operand.value()).ok(),
            )
            .is_some_and(|(dst_ty, src_ty)| dst_ty == src_ty)
            {
                return operand;
            }

            let peeled = CastSimplifier::peel_transmute(&operand).clone();
            // NOTE: Non-equality is more efficient than equality.
            if peeled.ne(&operand) {
                self.trans(peeled, dst_ty)
            } else {
                Expr::Transmutation {
                    source: peeled,
                    dst_ty,
                }
                .to_value_ref()
            }
        }
    }
}

mod concrete {
    use super::*;

    pub(super) type ConcreteBuilder = ConcreteAbstractorBuilder;

    #[derive(Default)]
    pub(crate) struct ConcreteAbstractorBuilder;

    impl BinaryExprBuilder for ConcreteAbstractorBuilder {
        type ExprRefPair<'a> = (ConcreteValueRef, ConcreteValueRef);
        type Expr<'a> = ValueRef;

        #[inline]
        fn binary_op<'a>(
            &mut self,
            _operands: Self::ExprRefPair<'a>,
            _op: AbsBinaryOp,
        ) -> Self::Expr<'a> {
            UnevalValue::Some.to_value_ref()
        }

        impl_singular_binary_ops_through_general!();
    }

    impl UnaryExprBuilder for ConcreteAbstractorBuilder {
        type ExprRef<'a> = ConcreteValueRef;
        type Expr<'a> = ValueRef;

        #[inline]
        fn unary_op<'a>(&mut self, _operand: Self::ExprRef<'a>, _op: AbsUnaryOp) -> Self::Expr<'a> {
            UnevalValue::Some.to_value_ref()
        }

        impl_singular_unary_ops_through_general!();
    }

    impl CastExprBuilder for ConcreteAbstractorBuilder {
        type ExprRef<'a> = ConcreteValueRef;
        type Expr<'a> = ValueRef;
        type Metadata<'a> = CastMetadata;

        #[inline]
        fn cast<'a, 'b>(
            &mut self,
            _operand: Self::ExprRef<'a>,
            _target: CastKind<Self::IntType, Self::FloatType, Self::PtrType, Self::GenericType>,
            _metadata: Self::Metadata<'b>,
        ) -> Self::Expr<'a> {
            UnevalValue::Some.to_value_ref()
        }

        impl_singular_casts_through_general!();
    }

    impl TernaryExprBuilder for ConcreteAbstractorBuilder {
        type ExprRefTriple<'a> = (ConcreteValueRef, ConcreteValueRef, ConcreteValueRef);
        type Expr<'a> = ValueRef;

        #[inline]
        fn if_then_else<'a>(
            &mut self,
            (_condition, _if_target, _else_target): Self::ExprRefTriple<'a>,
        ) -> Self::Expr<'a> {
            UnevalValue::Some.to_value_ref()
        }

        impl_general_ternary_op_through_singulars!();
    }
}

mod simp {
    use super::BasicBinaryOp::*;
    use super::*;

    /// # Generic Parameters
    /// * `Other`: The type of the other operand in an operation with a constant.
    /// * `ResultExpr`: The type of the result expression which effectively will
    ///   be either a constant or the other expression itself.
    #[derive(Clone, Default)]
    pub(crate) struct ConstSimplifier;

    /// A pair of operands where one of them is a constant.
    type WithConstOperand<'a, T> = BinaryOperands<T, &'a ConstValue>;

    impl<'a, T> WithConstOperand<'a, T> {
        /// `konst` is used in place of the word const, since it's a keyword :)
        #[inline]
        fn konst(&self) -> &ConstValue {
            self.as_flat().1
        }

        #[inline]
        fn is_first_zero(&self) -> bool {
            match self {
                Self::Rev { first, .. } => first.is_zero(),
                _ => false,
            }
        }

        #[inline]
        fn is_second_zero(&self) -> bool {
            match self {
                Self::Orig { second, .. } => second.is_zero(),
                _ => false,
            }
        }

        #[inline]
        fn is_second_one(&self) -> bool {
            match self {
                Self::Orig { second, .. } => second.is_one(),
                _ => false,
            }
        }

        #[inline]
        fn is_first_unsigned_zero(&self) -> bool {
            matches!(self, BinaryOperands::Rev {
                first: ConstValue::Int {
                    bit_rep: Wrapping(0),
                    ty: IntType {
                        is_signed: false,
                        ..
                    }
                },
                ..
            })
        }

        #[inline]
        fn is_second_unsigned_zero(&self) -> bool {
            matches!(self, BinaryOperands::Orig {
                second: ConstValue::Int {
                    bit_rep: Wrapping(0),
                    ty: IntType {
                        is_signed: false,
                        ..
                    }
                },
                ..
            })
        }

        #[inline]
        fn other_into<U>(self) -> U
        where
            T: Into<U>,
        {
            self.flatten().0.into()
        }

        #[inline]
        fn konst_into<U>(self) -> U
        where
            &'a ConstValue: Into<U>,
        {
            self.flatten().1.into()
        }
    }

    impl BinaryExprBuilder for ConstSimplifier {
        type ExprRefPair<'a> = WithConstOperand<'a, SymValueRef>;
        type Expr<'a> = Result<ValueRef, Self::ExprRefPair<'a>>;

        impl_general_binary_op_through_singulars!();

        /* NOTE: Most of the implementations cause the match expression to be
         * evaluated multiple times (unless the compiler performs a great job!).
         * Optimize if it becomes a bottleneck.
         */

        fn add<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            // x + 0 = 0 + x = x
            if operands.konst().is_zero() {
                Ok(operands.other_into())
            } else {
                Err(operands)
            }
        }

        fn add_unchecked<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            self.add(operands)
        }

        fn add_with_overflow<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn add_saturating<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn sub<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            // x - 0 = x
            if operands.is_second_zero() {
                Ok(operands.other_into())
            } else {
                Err(operands)
            }
        }

        fn sub_unchecked<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            self.sub(operands)
        }

        fn sub_with_overflow<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn sub_saturating<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn mul<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            // x * 0 = 0 * x = 0
            if operands.konst().is_zero() {
                Ok(operands.konst().into())
            }
            // x * 1 = 1 * x = x
            else if operands.konst().is_one() {
                Ok(operands.other_into())
            } else {
                Err(operands)
            }
        }

        fn mul_unchecked<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            self.mul(operands)
        }

        fn mul_with_overflow<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            self.mul(operands)
        }

        fn div<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            // 0 / x = 0
            if operands.is_first_zero() {
                Ok(operands.konst_into())
            }
            // x / 1 = x
            else if operands.is_second_one() {
                Ok(operands.other_into())
            } else {
                Err(operands)
            }
        }

        fn div_exact<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            self.div(operands)
        }

        fn rem<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            // 0 % x = 0
            if operands.is_first_zero() {
                Ok(operands.konst_into())
            }
            // x % 1 = 0
            else if operands.is_second_one() {
                Ok((&match operands.konst() {
                    ConstValue::Int { ty, .. } => ConstValue::new_int(0_u128, *ty),
                    ConstValue::Float { .. } => todo!(),
                    _ => unreachable!("The second operand should be numeric."),
                })
                    .into())
            } else {
                Err(operands)
            }
        }

        fn xor<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            // x ^ 0 = 0 ^ x = x
            if operands.konst().is_zero() {
                Ok(operands.other_into())
            } else {
                Err(operands)
            }
        }

        fn and<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            match operands.konst() {
                // x & false = false & x = false
                ConstValue::Bool(false) => Ok(operands.konst_into()),
                // x & true = true & x = x
                ConstValue::Bool(true) => Ok(operands.other_into()),
                // x & 0 = 0 & x = 0
                ConstValue::Int {
                    bit_rep: Wrapping(0),
                    ..
                } => Ok(operands.konst_into()),
                // x & 1...1 = 1...1 & x = x
                ConstValue::Int { bit_rep, ty } if ty.all_one() == ty.masked(bit_rep.0) => {
                    Ok(operands.other_into())
                }
                _ => Err(operands),
            }
        }

        fn or<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            match operands.konst() {
                // x | false = false | x = x
                ConstValue::Bool(false) => Ok(operands.other_into()),
                // x | true = true | x = true
                ConstValue::Bool(true) => Ok(operands.konst_into()),
                // x | 0 = 0 | x = x
                ConstValue::Int {
                    bit_rep: Wrapping(0),
                    ..
                } => Ok(operands.other_into()),
                // x | 1...1 = 1...1 | x = 1...1
                ConstValue::Int { bit_rep, ty } if ty.all_one() == ty.masked(bit_rep.0) => {
                    Ok(operands.konst_into())
                }
                _ => Err(operands),
            }
        }

        fn shl<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            match operands {
                // x << 0 = x
                BinaryOperands::Orig { first, second } if second.is_zero() => Ok(first.into()),
                // 0 << x = 0
                BinaryOperands::Rev { first, .. } if first.is_zero() => Ok(first.into()),
                _ => Err(operands),
            }
        }

        fn shl_unchecked<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            self.shl(operands)
        }

        fn shr<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            match operands {
                // x >> 0 = x
                BinaryOperands::Orig { first, second } if second.is_zero() => Ok(first.into()),
                // 0 >> x = 0
                BinaryOperands::Rev { first, .. } if first.is_zero() => Ok(first.into()),
                _ => Err(operands),
            }
        }

        fn shr_unchecked<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            self.shr(operands)
        }

        fn rotate_left<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            match operands {
                // x _<_ 0 = x
                BinaryOperands::Orig { first, second } if second.is_zero() => Ok(first.into()),
                // 0 _<_ x = 0
                BinaryOperands::Rev { first, .. } if first.is_zero() => Ok(first.into()),
                _ => Err(operands),
            }
        }

        fn rotate_right<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            match operands {
                // x _>_ 0 = x
                BinaryOperands::Orig { first, second } if second.is_zero() => Ok(first.into()),
                // 0 _>_ x = 0
                BinaryOperands::Rev { first, .. } if first.is_zero() => Ok(first.into()),
                _ => Err(operands),
            }
        }

        fn eq<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            if operands.konst() == &ConstValue::Bool(true) {
                Ok(operands.other_into())
            } else if operands.konst() == &ConstValue::Bool(false) {
                Ok(Expr::Unary {
                    operator: UnaryOp::Not,
                    operand: operands.other_into(),
                }
                .to_value_ref()
                .into())
            } else {
                Err(operands)
            }
        }

        fn lt<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            // For unsigned integers, nothing is less than zero.
            if operands.is_second_unsigned_zero() {
                Ok((&ConstValue::Bool(false)).into())
            } else {
                Err(operands)
            }
        }

        fn le<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            // For unsigned integers, zero is less than or equal to everything.
            if operands.is_first_unsigned_zero() {
                Ok((&ConstValue::Bool(true)).into())
            } else {
                Err(operands)
            }
        }

        fn ne<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            if operands.konst() == &ConstValue::Bool(true) {
                Ok(Expr::Unary {
                    operator: UnaryOp::Not,
                    operand: operands.other_into(),
                }
                .to_value_ref()
                .into())
            } else if operands.konst() == &ConstValue::Bool(false) {
                Ok(operands.other_into())
            } else {
                Err(operands)
            }
        }

        fn ge<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            // For unsigned integers, everything is greater than or equal to zero.
            if operands.is_second_unsigned_zero() {
                Ok((&ConstValue::Bool(true)).into())
            } else {
                Err(operands)
            }
        }

        fn gt<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            // For unsigned integers, zero is not greater than anything.
            if operands.is_first_unsigned_zero() {
                Ok((&ConstValue::Bool(false)).into())
            } else {
                Err(operands)
            }
        }

        fn cmp<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn offset<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }
    }

    impl TernaryExprBuilder for ConstSimplifier {
        type ExprRefTriple<'a> = (&'a ConstValue, ValueRef, ValueRef);
        type Expr<'a> = ValueRef;

        fn if_then_else<'a>(
            &mut self,
            (condition, if_target, else_target): Self::ExprRefTriple<'a>,
        ) -> Self::Expr<'a> {
            let ConstValue::Bool(condition) = condition else {
                unreachable!("Condition must be a boolean constant. {:?}", condition)
            };
            if *condition { if_target } else { else_target }
        }

        impl_general_ternary_op_through_singulars!();
    }

    impl From<&ConstValue> for ValueRef {
        fn from(value: &ConstValue) -> Self {
            value.clone().to_value_ref()
        }
    }

    #[derive(Clone, Default)]
    pub(crate) struct ConstFolder;

    type FoldableOperands<'a> =
        WithConstOperand<'a, BinaryExpr<BasicBinaryOp, WithConstOperand<'a, &'a SymValueRef>>>;

    impl<'a> FoldableOperands<'a> {
        #[inline]
        /// Returns the constant of the inner expression
        fn a(&self) -> &ConstValue {
            self.as_flat().0.operands.as_flat().1
        }

        #[inline]
        /// Returns the constant of the outer expression
        fn b(&self) -> &ConstValue {
            self.as_flat().1
        }

        #[inline]
        fn expr(&self) -> &BinaryExpr<BasicBinaryOp, WithConstOperand<'a, &'a SymValueRef>> {
            self.as_flat().0
        }

        /// Creates a new BinaryExpr from the inner expression and the folded value.
        /// Uses the original operator of the inner expression and uses the folded value
        /// for the constant.
        fn fold_expr(self, folded_value: ConstValue) -> BinaryExpr {
            let expr = self.flatten().0;
            let (x, _, is_reversed) = expr.operands.flatten();
            BinaryExpr {
                operands: SymBinaryOperands::from((
                    x.clone(),
                    folded_value.to_value_ref(),
                    is_reversed,
                )),
                operator: expr.operator,
            }
        }

        /// Creates a new BinaryExpr from the existing symbolic value and the newly folded
        /// constant. Accepts a new operator and the `is_reversed` flag (true means x on the
        /// right).
        fn new_expr(
            self,
            folded_value: ConstValue,
            op: BasicBinaryOp,
            is_reversed: bool,
        ) -> BinaryExpr {
            let expr = self.flatten().0;
            let x = expr.operands.flatten().0;
            BinaryExpr {
                operands: SymBinaryOperands::from((
                    x.clone(),
                    folded_value.to_value_ref(),
                    is_reversed,
                )),
                operator: op,
            }
        }
    }

    impl BinaryExprBuilder for ConstFolder {
        // In general, we don't need to worry about if the constants are signed. Since the value
        // is stored in a u128, the result will be correct once the value is converted back to
        // the original type. The same goes for overflow and underflow since this code is only
        // reached when the source is compiled with optimizations in which case overflow and
        // underflow are performed anyway.

        type ExprRefPair<'a> = FoldableOperands<'a>;
        type Expr<'a> = Result<BinaryExpr, Self::ExprRefPair<'a>>;

        impl_general_binary_op_through_singulars!();

        fn add<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            let (a, b) = (operands.a(), operands.b());

            match operands.expr().operator {
                // (x + a) + b = x + (a + b)
                Add => {
                    let folded_value = ConstValue::binary_op_arithmetic(a, b, Add);
                    Ok(operands.fold_expr(folded_value))
                }
                Sub => {
                    match &operands.expr().operands {
                        // (x - a) + b = x - (a - b)
                        BinaryOperands::Orig { .. } => {
                            let folded_value = ConstValue::binary_op_arithmetic(a, b, Sub);
                            Ok(operands.fold_expr(folded_value))
                        }
                        // (a - x) + b = (a + b) - x
                        BinaryOperands::Rev { .. } => {
                            let folded_value = ConstValue::binary_op_arithmetic(a, b, Add);
                            Ok(operands.fold_expr(folded_value))
                        }
                    }
                }
                _ => Err(operands),
            }
        }

        fn add_unchecked<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            self.add(operands)
        }

        fn sub<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            let (a, b) = (operands.a(), operands.b());

            match operands {
                BinaryOperands::Orig { .. } => {
                    match operands.expr().operator {
                        // (x + a) - b = x + (a - b)
                        Add => {
                            let folded_value = ConstValue::binary_op_arithmetic(a, b, Sub);
                            Ok(operands.fold_expr(folded_value))
                        }
                        Sub => {
                            match operands.expr().operands {
                                // (x - a) - b = x - (a + b)
                                BinaryOperands::Orig { .. } => {
                                    let folded_value = ConstValue::binary_op_arithmetic(a, b, Add);
                                    Ok(operands.fold_expr(folded_value))
                                }
                                // (a - x) - b = (a - b) - x
                                BinaryOperands::Rev { .. } => {
                                    let folded_value = ConstValue::binary_op_arithmetic(a, b, Sub);
                                    Ok(operands.fold_expr(folded_value))
                                }
                            }
                        }
                        _ => Err(operands),
                    }
                }
                BinaryOperands::Rev { .. } => match operands.expr().operator {
                    // b - (x + a) = (b - a) - x
                    Add => {
                        let folded_value = ConstValue::binary_op_arithmetic(b, a, Sub);
                        Ok(operands.new_expr(folded_value, Sub, true))
                    }
                    Sub => {
                        match operands.expr().operands {
                            // b - (x - a) = (b + a) - x
                            BinaryOperands::Orig { .. } => {
                                let folded_value = ConstValue::binary_op_arithmetic(b, a, Add);
                                Ok(operands.new_expr(folded_value, Sub, true))
                            }
                            // b - (a - x) = x + (b - a)
                            BinaryOperands::Rev { .. } => {
                                let folded_value = ConstValue::binary_op_arithmetic(b, a, Sub);
                                Ok(operands.new_expr(folded_value, Add, false))
                            }
                        }
                    }
                    _ => Err(operands),
                },
            }
        }

        fn sub_unchecked<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            self.sub(operands)
        }

        fn mul<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            let (a, b) = (operands.a(), operands.b());

            match operands.expr().operator {
                // (x * a) * b = x * (a * b)
                Mul => {
                    let folded_value = ConstValue::binary_op_arithmetic(a, b, Mul);
                    Ok(operands.fold_expr(folded_value))
                }
                _ => Err(operands),
            }
        }

        fn mul_unchecked<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            self.mul(operands)
        }

        fn div<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn div_exact<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn rem<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn and<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            let (a, b) = (operands.a(), operands.b());

            match operands.expr().operator {
                // (x & a) & b = x & (a & b)
                BitAnd => {
                    let folded_value = ConstValue::binary_op_arithmetic(a, b, BitAnd);
                    Ok(operands.fold_expr(folded_value))
                }
                _ => Err(operands),
            }
        }

        fn or<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            let (a, b) = (operands.a(), operands.b());

            match operands.expr().operator {
                // (x | a) | b = x | (a | b)
                BitOr => {
                    let folded_value = ConstValue::binary_op_arithmetic(a, b, BitOr);
                    Ok(operands.fold_expr(folded_value))
                }
                _ => Err(operands),
            }
        }

        fn xor<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            let (a, b) = (operands.a(), operands.b());

            match operands.expr().operator {
                // (x ^ a) ^ b = x ^ (a ^ b)
                BitXor => {
                    let folded_value = ConstValue::binary_op_arithmetic(a, b, BitXor);
                    Ok(operands.fold_expr(folded_value))
                }
                _ => Err(operands),
            }
        }

        fn shl<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            match operands.expr().operator {
                // (x << a) << b = x << (a + b)
                Shl => {
                    // TODO
                    Err(operands)
                }
                _ => Err(operands),
            }
        }

        fn shl_unchecked<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            self.shl(operands)
        }

        fn shr<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            match operands.expr().operator {
                // (x >> a) >> b = x >> (a + b)
                Shr => {
                    // TODO
                    Err(operands)
                }
                _ => Err(operands),
            }
        }

        fn shr_unchecked<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            self.shr(operands)
        }

        fn rotate_left<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            match operands.expr().operator {
                // (x _<_ a) _<_ b = x _<_ (a + b)
                RotateL => {
                    // TODO
                    Err(operands)
                }
                // (x _>_ a) _<_ b = x _<_ (b - a)
                RotateR => {
                    // TODO
                    Err(operands)
                }
                _ => Err(operands),
            }
        }

        fn rotate_right<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            match operands.expr().operator {
                // (x _>_ a) _>_ b = x _>_ (a + b)
                RotateR => {
                    // TODO
                    Err(operands)
                }
                // (x _<_ a) _>_ b = x _>_ (b - a)
                RotateL => {
                    // TODO
                    Err(operands)
                }
                _ => Err(operands),
            }
        }

        fn add_with_overflow<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn add_saturating<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn sub_with_overflow<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn sub_saturating<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn mul_with_overflow<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn eq<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn ne<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn lt<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn le<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn gt<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn ge<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn cmp<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }

        fn offset<'a>(&mut self, operands: Self::ExprRefPair<'a>) -> Self::Expr<'a> {
            Err(operands)
        }
    }

    pub(crate) struct CastSimplifier;

    impl CastSimplifier {
        #[inline]
        pub(crate) fn peel_transmute<'a>(source: &SymValueRef) -> &SymValueRef {
            // NOTE: Only a single layer is expected and nested ones should not be constructed.
            match source.as_ref() {
                SymValue::Expression(Expr::Transmutation { source, .. }) => &source,
                _ => source,
            }
        }

        pub(crate) fn truncate(source: &SymValueRef, dst_ty: IntType) -> SymValueRef {
            match source.as_ref() {
                SymValue::Expression(Expr::Extension(ext)) => {
                    let bits_to_truncate: u32 =
                        (ext.ty.bit_size().unwrap().get() - dst_ty.bit_size) as u32;
                    let bits_to_add = ext.bits_to_add.get();
                    if bits_to_truncate == bits_to_add {
                        ext.source.clone()
                    } else if bits_to_truncate < bits_to_add {
                        ExtensionExpr {
                            source: ext.source.clone(),
                            is_zero_ext: ext.is_zero_ext,
                            bits_to_add: NonZeroU32::new(bits_to_add - bits_to_truncate).unwrap(),
                            ty: dst_ty.into(),
                        }
                        .to_value_ref()
                    } else {
                        TruncationExpr {
                            source: ext.source.clone(),
                            ty: dst_ty.into(),
                        }
                        .to_value_ref()
                    }
                }
                SymValue::Expression(Expr::Truncation(truncation)) => TruncationExpr {
                    source: truncation.source.clone(),
                    ty: dst_ty.into(),
                }
                .to_value_ref(),
                _ => TruncationExpr {
                    source: source.clone(),
                    ty: dst_ty.into(),
                }
                .to_value_ref(),
            }
        }

        pub(crate) fn extend(
            source: &SymValueRef,
            is_zero_extension: bool,
            bits_to_add: NonZeroU32,
            dst_ty: ValueType,
        ) -> ExtensionExpr {
            let (source, bits_to_add) = match source.as_ref() {
                SymValue::Expression(Expr::Extension(ext))
                    if ext.is_zero_ext == is_zero_extension =>
                {
                    (
                        &ext.source,
                        NonZeroU32::new(ext.bits_to_add.get() + bits_to_add.get()).unwrap(),
                    )
                }
                _ => (source, bits_to_add),
            };

            ExtensionExpr {
                source: source.clone(),
                is_zero_ext: is_zero_extension,
                bits_to_add,
                ty: dst_ty,
            }
        }
    }
}
