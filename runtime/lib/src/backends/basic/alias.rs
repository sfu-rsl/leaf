use core::ops::DerefMut;

use super::{
    ConstValue, ExeTraceStorage, Implied, LazyTypeInfo, TraceQuerier,
    expr::{SymBinaryOperands, SymTernaryOperands, SymValueRef, ValueRef},
    trace::BasicExeTraceRecorder,
};
use crate::abs::{
    self, FloatType, IntType, TypeId,
    backend::*,
    expr::{BinaryExprBuilder, CastExprBuilder, TernaryExprBuilder, UnaryExprBuilder},
};
use common::type_info::TypeInfo;

pub(crate) use crate::utils::alias::*;

pub(crate) trait SymValueRefExprBuilder
where
    Self: for<'a> BinaryExprBuilder<ExprRefPair<'a> = SymBinaryOperands, Expr<'a> = ValueRef>
        + for<'a> UnaryExprBuilder<ExprRef<'a> = SymValueRef, Expr<'a> = ValueRef>
        + for<'a> TernaryExprBuilder<ExprRefTriple<'a> = SymTernaryOperands, Expr<'a> = ValueRef>
        + for<'a> CastExprBuilder<
            ExprRef<'a> = SymValueRef,
            Expr<'a> = SymValueRef,
            Metadata<'a> = LazyTypeInfo,
            IntType = IntType,
            FloatType = FloatType,
            PtrType = TypeId,
            GenericType = TypeId,
        >,
{
}

pub(crate) trait ValueRefExprBuilder
where
    Self: for<'a> BinaryExprBuilder<ExprRefPair<'a> = (ValueRef, ValueRef), Expr<'a> = ValueRef>
        + for<'a> UnaryExprBuilder<ExprRef<'a> = ValueRef, Expr<'a> = ValueRef>
        + for<'a> TernaryExprBuilder<
            ExprRefTriple<'a> = (ValueRef, ValueRef, ValueRef),
            Expr<'a> = ValueRef,
        > + for<'a> CastExprBuilder<
            ExprRef<'a> = ValueRef,
            Expr<'a> = ValueRef,
            Metadata<'a> = LazyTypeInfo,
            IntType = IntType,
            FloatType = FloatType,
            PtrType = TypeId,
            GenericType = TypeId,
        >,
{
}

pub(crate) trait ValueRefBinaryExprBuilder
where
    Self: for<'a> BinaryExprBuilder<ExprRefPair<'a> = (ValueRef, ValueRef), Expr<'a> = ValueRef>,
{
}

pub(crate) trait ValueRefUnaryExprBuilder
where
    Self: for<'a> UnaryExprBuilder<ExprRef<'a> = ValueRef, Expr<'a> = ValueRef>,
{
}

pub(crate) trait ValueRefExprBuilderWrapper {
    fn inner<'a>(&'a mut self) -> impl DerefMut<Target = impl ValueRefExprBuilder + 'a>;
}

pub(crate) trait ImpliedValueRefExprBuilder
where
    Self: for<'a> BinaryExprBuilder<
            ExprRefPair<'a> = (Implied<ValueRef>, Implied<ValueRef>),
            Expr<'a> = Implied<ValueRef>,
        > + for<'a> UnaryExprBuilder<ExprRef<'a> = Implied<ValueRef>, Expr<'a> = Implied<ValueRef>>
        + for<'a> TernaryExprBuilder<
            ExprRefTriple<'a> = (Implied<ValueRef>, Implied<ValueRef>, Implied<ValueRef>),
            Expr<'a> = Implied<ValueRef>,
        > + for<'a> CastExprBuilder<
            ExprRef<'a> = Implied<ValueRef>,
            Expr<'a> = Implied<ValueRef>,
            Metadata<'a> = LazyTypeInfo,
            IntType = IntType,
            FloatType = FloatType,
            PtrType = TypeId,
            GenericType = TypeId,
        > + ValueRefExprBuilderWrapper,
{
}

pub(crate) trait ImpliedValueRefUnaryExprBuilder
where
    Self: for<'a> UnaryExprBuilder<ExprRef<'a> = Implied<ValueRef>, Expr<'a> = Implied<ValueRef>>,
{
}

pub(crate) use super::expr::builders::DefaultImpliedExprBuilder as BasicExprBuilder;
pub(crate) use super::expr::builders::DefaultSymExprBuilder as BasicSymExprBuilder;

pub(crate) trait TypeDatabase:
    abs::backend::TypeDatabase<'static>
    + CoreTypeProvider<&'static TypeInfo>
    + CoreTypeProvider<LazyTypeInfo>
{
}
impl<'t, T> TypeDatabase for T where
    T: abs::backend::TypeDatabase<'static>
        + CoreTypeProvider<&'static TypeInfo>
        + CoreTypeProvider<LazyTypeInfo>
{
}

pub(crate) trait TraceManager:
    abs::backend::TraceManager<super::trace::Step, Implied<ValueRef>, ConstValue> + Shutdown
{
}
impl<T> TraceManager for T where
    T: abs::backend::TraceManager<super::trace::Step, Implied<ValueRef>, ConstValue> + Shutdown
{
}

pub(super) trait BasicTraceQuerier:
    TraceQuerier<
        Record = <BasicExeTraceRecorder as ExeTraceStorage>::Record,
        Constraint = super::Constraint,
    >
{
}
impl<T> BasicTraceQuerier for T where
    T: TraceQuerier<
            Record = <BasicExeTraceRecorder as ExeTraceStorage>::Record,
            Constraint = super::Constraint,
        >
{
}

pub(super) type BasicPlaceInfo = super::place::PlaceWithMetadata;

pub(super) use super::constraint::Constraint as BasicConstraint;
pub(super) use super::constraint::DecisionCase as BasicConstraintDecisionCase;
pub(super) type BasicDecisionTraceRecorder =
    dyn DecisionTraceRecorder<Case = BasicConstraintDecisionCase>;
