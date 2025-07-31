use core::ops::DerefMut;

use super::{
    ConstValue, ExeTraceStorage, GenericTraceQuerier, GenericVariablesState, LazyTypeInfo,
    TraceIndicesProvider, TraceViewProvider,
    expr::{SymBinaryOperands, SymTernaryOperands, SymValueRef, ValueRef, place::PlaceValueRef},
    implication::Implied,
    trace::BasicExeTraceRecorder,
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

pub(super) trait ValueRefUnaryExprBuilder
where
    Self: for<'a> UnaryExprBuilder<ExprRef<'a> = ValueRef, Expr<'a> = ValueRef>,
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

pub(super) trait BasicValueExprBuilder: ImpliedValueRefExprBuilder {}
impl<T: ImpliedValueRefExprBuilder> BasicValueExprBuilder for T {}

pub(super) trait BasicValueUnaryExprBuilder: ImpliedValueRefUnaryExprBuilder {}
impl<T: ImpliedValueRefUnaryExprBuilder> BasicValueUnaryExprBuilder for T {}

pub(super) use super::expr::builders::DefaultImpliedExprBuilder as BasicExprBuilder;
pub(super) use super::expr::builders::DefaultSymExprBuilder as BasicSymExprBuilder;

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
    GenericVariablesState<PlaceInfo = BasicPlaceInfo, PlaceValue = PlaceValueRef, Value = BasicValue>
{
}
impl<T> VariablesState for T where
    T: GenericVariablesState<
            PlaceInfo = BasicPlaceInfo,
            PlaceValue = PlaceValueRef,
            Value = BasicValue,
        >
{
}

pub(super) type BasicSymPlaceHandler = dyn super::state::SymPlaceHandler<
        SymEntity = super::state::SymPlaceSymEntity,
        ConcEntity = super::ConcreteValueRef,
        Entity = ValueRef,
    >;

pub(super) type BasicValue = Implied<ValueRef>;

pub(super) type BasicVariablesState = super::state::RawPointerVariableState<BasicSymExprBuilder>;

pub(super) type BasicPlaceInfo = super::place::PlaceWithMetadata;

pub(super) type BasicPlaceValue = PlaceValueRef;

pub(super) use super::constraint::Constraint as BasicConstraint;
pub(super) use super::constraint::DecisionCase as BasicConstraintDecisionCase;
pub(super) type BasicDecisionTraceRecorder =
    dyn DecisionTraceRecorder<Case = BasicConstraintDecisionCase>;

pub(super) type BasicCallStackManager = super::call::BasicCallStackManager<BasicVariablesState>;

pub(crate) trait TraceManager:
    abs::backend::TraceManager<super::trace::Step, BasicValue, ConstValue> + Shutdown
{
}
impl<T> TraceManager for T where
    T: abs::backend::TraceManager<super::trace::Step, BasicValue, ConstValue> + Shutdown
{
}
pub(super) trait TraceManagerWithViews:
    TraceManager
    + TraceViewProvider<Indexed<super::trace::Step>>
    + TraceViewProvider<BasicConstraint>
    + TraceIndicesProvider<super::trace::SymDependentMarker>
{
}
impl<T> TraceManagerWithViews for T where
    T: TraceManager
        + TraceViewProvider<Indexed<super::trace::Step>>
        + TraceViewProvider<BasicConstraint>
        + TraceIndicesProvider<super::trace::SymDependentMarker>
{
}

pub(super) trait ExeTraceRecorder:
    CallTraceRecorder + DecisionTraceRecorder + ExeTraceStorage
{
}
impl<T> ExeTraceRecorder for T where T: CallTraceRecorder + DecisionTraceRecorder + ExeTraceStorage {}

pub(super) trait TraceQuerier:
    GenericTraceQuerier<
        Record = <BasicExeTraceRecorder as ExeTraceStorage>::Record,
        Constraint = BasicConstraint,
    >
{
}
impl<T> TraceQuerier for T where
    T: GenericTraceQuerier<
            Record = <BasicExeTraceRecorder as ExeTraceStorage>::Record,
            Constraint = BasicConstraint,
        >
{
}
