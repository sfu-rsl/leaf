use core::ops::DerefMut;

use super::{
    ConstValue, ExeTraceStorage, GenericTraceQuerier, GenericVariablesState, LazyTypeInfo,
    SymExConstraint, SymExConstraintDecisionCase, SymExPlaceInfo, SymExPlaceValue, SymExValue,
    TraceIndicesProvider, TraceViewProvider,
    expr::{SymBinaryOperands, SymTernaryOperands, SymValueRef, ValueRef},
    implication::Implied,
    trace::SymExExeTraceRecorder,
};

use crate::{
    abs::{
        self, FloatType, IntType, TypeId,
        backend::*,
        expr::{BinaryExprBuilder, CastExprBuilder, TernaryExprBuilder, UnaryExprBuilder},
    },
    utils::Indexed,
};
use common::type_info::TypeInfo;

pub(super) trait SymValueRefExprBuilder
where
    Self: for<'a> BinaryExprBuilder<ExprRefPair<'a> = SymBinaryOperands, Expr<'a> = ValueRef>
        + for<'a> UnaryExprBuilder<ExprRef<'a> = SymValueRef, Expr<'a> = ValueRef>
        + for<'a> TernaryExprBuilder<ExprRefTriple<'a> = SymTernaryOperands, Expr<'a> = ValueRef>
        + for<'a> CastExprBuilder<
            ExprRef<'a> = SymValueRef,
            Expr<'a> = ValueRef,
            Metadata<'a> = LazyTypeInfo,
            IntType = IntType,
            FloatType = FloatType,
            PtrType = TypeId,
            GenericType = TypeId,
        >,
{
}

pub(super) trait ValueRefExprBuilder
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

pub(super) trait ValueRefBinaryExprBuilder
where
    Self: for<'a> BinaryExprBuilder<ExprRefPair<'a> = (ValueRef, ValueRef), Expr<'a> = ValueRef>,
{
}

pub(super) trait ValueRefExprBuilderWrapper {
    fn inner<'a>(&'a mut self) -> impl DerefMut<Target = impl ValueRefExprBuilder + 'a>;
}

pub(super) trait ImpliedValueRefExprBuilder
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

pub(super) trait ImpliedValueRefUnaryExprBuilder
where
    Self: for<'a> UnaryExprBuilder<ExprRef<'a> = Implied<ValueRef>, Expr<'a> = Implied<ValueRef>>,
{
}

pub(super) trait SymExValueExprBuilder: ImpliedValueRefExprBuilder {}
impl<T: ImpliedValueRefExprBuilder> SymExValueExprBuilder for T {}

pub(super) trait SymExValueUnaryExprBuilder: ImpliedValueRefUnaryExprBuilder {}
impl<T: ImpliedValueRefUnaryExprBuilder> SymExValueUnaryExprBuilder for T {}

pub(super) use super::expr::builders::DefaultImpliedExprBuilder as DefaultExprBuilder;
pub(super) use super::expr::builders::DefaultSymExprBuilder;

pub(super) trait TypeDatabase:
    abs::backend::TypeDatabase<'static>
    + for<'t> CoreTypeProvider<&'t TypeInfo>
    + CoreTypeProvider<LazyTypeInfo>
{
}
impl<T> TypeDatabase for T where
    T: abs::backend::TypeDatabase<'static>
        + for<'t> CoreTypeProvider<&'t TypeInfo>
        + CoreTypeProvider<LazyTypeInfo>
{
}

pub(super) trait VariablesState:
    GenericVariablesState<PlaceInfo = SymExPlaceInfo, PlaceValue = SymExPlaceValue, Value = SymExValue>
{
}
impl<T> VariablesState for T where
    T: GenericVariablesState<
            PlaceInfo = SymExPlaceInfo,
            PlaceValue = SymExPlaceValue,
            Value = SymExValue,
        >
{
}

pub(super) type SymExSymPlaceHandler = dyn super::state::SymPlaceHandler<
        SymEntity = super::state::SymPlaceSymEntity,
        ConcEntity = super::ConcreteValueRef,
        Entity = ValueRef,
    >;

pub(super) type SymExVariablesState = super::state::RawPointerVariableState<DefaultSymExprBuilder>;

pub(super) type DynDecisionTraceRecorder =
    dyn DecisionTraceRecorder<Case = SymExConstraintDecisionCase>;

pub(crate) trait TraceManager:
    abs::backend::TraceManager<super::trace::Step, SymExValue, ConstValue> + Shutdown
{
}
impl<T> TraceManager for T where
    T: abs::backend::TraceManager<super::trace::Step, SymExValue, ConstValue> + Shutdown
{
}

pub(super) trait TraceManagerWithViews:
    TraceManager
    + TraceViewProvider<Indexed<super::trace::Step>>
    + TraceViewProvider<SymExConstraint>
    + TraceIndicesProvider<super::trace::SymDependentMarker>
{
}
impl<T> TraceManagerWithViews for T where
    T: TraceManager
        + TraceViewProvider<Indexed<super::trace::Step>>
        + TraceViewProvider<SymExConstraint>
        + TraceIndicesProvider<super::trace::SymDependentMarker>
{
}

pub(super) trait ExeTraceRecorder:
    PhasedCallTraceRecorder + DecisionTraceRecorder + ExeTraceStorage
{
}
impl<T> ExeTraceRecorder for T where
    T: PhasedCallTraceRecorder + DecisionTraceRecorder + ExeTraceStorage
{
}

pub(super) trait TraceQuerier:
    GenericTraceQuerier<
        Record = <SymExExeTraceRecorder as ExeTraceStorage>::Record,
        Constraint = SymExConstraint,
    >
{
}
impl<T> TraceQuerier for T where
    T: GenericTraceQuerier<
            Record = <SymExExeTraceRecorder as ExeTraceStorage>::Record,
            Constraint = SymExConstraint,
        >
{
}
