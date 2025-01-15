use super::{
    expr::{SymBinaryOperands, SymTernaryOperands, SymValueRef, ValueRef},
    ConstValue, LazyTypeInfo,
};
use crate::abs::{
    self,
    backend::*,
    expr::{BinaryExprBuilder, CastExprBuilder, TernaryExprBuilder, UnaryExprBuilder},
    FloatType, IntType, TypeId,
};
use common::tyexp::TypeInfo;

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

pub(crate) use super::expr::builders::DefaultExprBuilder as BasicExprBuilder;
pub(crate) use super::expr::builders::DefaultSymExprBuilder as BasicSymExprBuilder;

pub(crate) trait TypeManager:
    abs::backend::TypeManager<Key = TypeId, Value = &'static TypeInfo>
    + CoreTypeProvider<&'static TypeInfo>
    + CoreTypeProvider<LazyTypeInfo>
{
}
impl<'t, T> TypeManager for T where
    T: abs::backend::TypeManager<Key = TypeId, Value = &'static TypeInfo>
        + CoreTypeProvider<&'static TypeInfo>
        + CoreTypeProvider<LazyTypeInfo>
{
}

pub(crate) trait TraceManager:
    abs::backend::TraceManager<super::trace::Step, ValueRef, ConstValue> + Shutdown
{
}
impl<T> TraceManager for T where
    T: abs::backend::TraceManager<super::trace::Step, ValueRef, ConstValue> + Shutdown
{
}
