use std::fmt::{Display, Formatter, Result};

use common::utils::comma_separated;

use super::{
    place::{
        DerefSymHostPlace, DeterministicPlaceValue, DeterministicProjection, PlaceValue,
        SymIndexedPlace, SymbolicPlaceBase, SymbolicPlaceValue,
    },
    *,
};

impl Display for Value {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Value::Concrete(value) => write!(f, "{value}"),
            Value::Symbolic(value) => write!(f, "{value}"),
        }
    }
}

impl Display for ConcreteValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            ConcreteValue::Const(value) => write!(f, "{value}"),
            ConcreteValue::Adt(value) => write!(f, "{value}"),
            ConcreteValue::Array(value) => write!(f, "{value}"),
            ConcreteValue::FatPointer(value) => write!(f, "{value}"),
            ConcreteValue::Unevaluated(value) => write!(f, "{value}"),
        }
    }
}

impl Display for ConstValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            ConstValue::Bool(value) => write!(f, "{value}"),
            ConstValue::Char(value) => write!(f, "'{value}'"),
            ConstValue::Int {
                bit_rep,
                ty: ty @ IntType { is_signed, .. },
            } => {
                if *is_signed {
                    write!(f, "{}", ty.signed_masked(bit_rep.0))
                } else {
                    write!(f, "{}", ty.masked(bit_rep.0))
                }?;
                write!(f, "{ty}")
            }
            ConstValue::Float { .. } => write!(f, "{self:?}"),
            ConstValue::Addr(addr) => write!(f, "ᴬ{:p}", *addr),
        }
    }
}

impl Display for AdtValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self.kind {
            AdtKind::Struct => write!(f, "{{"),
            AdtKind::Enum { variant } => write!(f, "V#{}{{", variant),
        }?;
        self.fmt_fields(f)?;
        match self.kind {
            AdtKind::Struct | AdtKind::Enum { .. } => write!(f, "}}"),
        }?;
        Ok(())
    }
}
impl AdtValue {
    fn fmt_fields(&self, f: &mut Formatter<'_>) -> Result {
        for (index, field) in self.fields.iter().enumerate() {
            write!(f, "{}", index)?;
            write!(f, ": ")?;
            match &field.value {
                Some(value) => write!(f, "{}", value)?,
                None => write!(f, "_")?,
            };
            write!(f, ", ")?;
        }
        Ok(())
    }
}

impl Display for ArrayValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "[")?;
        for value in self.elements.iter() {
            write!(f, "{}, ", value)?;
        }
        write!(f, "]")
    }
}

impl Display for FatPtrValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "({}, {})", self.address, self.metadata)
    }
}

impl Display for UnevalValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            UnevalValue::Some => write!(f, ".C."),
            UnevalValue::Lazy(raw) => write!(f, "{raw}"),
        }
    }
}

impl Display for RawConcreteValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "@({:p}:{})", self.0, self.1)
    }
}

impl Display for LazyTypeInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            LazyTypeInfo::None => write!(f, "?"),
            LazyTypeInfo::Id(id) | LazyTypeInfo::IdSize(id, _) => write!(f, "T#{}", id),
            LazyTypeInfo::IdPrimitive(_id, ty) => write!(f, "{}", ty),
            LazyTypeInfo::Fetched(ty) => write!(f, "T#{}", ty.id),
            LazyTypeInfo::Forced(ty) => write!(f, "T#ᶠ{}", ty.id),
        }
    }
}

impl Display for SymValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            SymValue::Variable(var) => write!(f, "{var}"),
            SymValue::Expression(expr) => write!(f, "{expr}"),
        }
    }
}

impl Display for SymbolicVar {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "<Var{}: {}>", self.id, self.ty)
    }
}

impl Display for Expr {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        // Prefix style
        self.fmt_operator(f)?;
        write!(f, "(")?;
        self.fmt_params(f)?;
        write!(f, ")")?;
        Ok(())
    }
}
impl Expr {
    fn fmt_operator(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Expr::Unary { operator, .. } => write!(f, "{operator}"),
            Expr::Binary(BinaryExpr { operator, .. }) => write!(f, "{operator}"),
            Expr::BinaryBoundCheck {
                bin_expr: BinaryExpr { operator, .. },
                is_overflow,
            } => write!(f, "{}{operator}?", if *is_overflow { "O" } else { "U" }),
            Expr::Offset { .. } => write!(f, "{}", crate::abs::BinaryOp::Offset),
            Expr::Extension(..) => write!(f, "Ext"),
            Expr::Truncation(..) => write!(f, "Trunc"),
            Expr::Ite { .. } => write!(f, "Ite"),
            Expr::Transmutation { .. } => write!(f, "Trans"),
            Expr::Multi(_) => write!(f, "Multi"),
            Expr::Ref(_) => write!(f, "&"),
            Expr::Partial(_) => write!(f, "Partial"),
            Expr::Concat(_) => write!(f, "||"),
            Expr::PtrMetadata(..) => write!(f, ""),
        }
    }

    fn fmt_params(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Expr::Unary { operand, .. } => write!(f, "{operand}"),
            Expr::Binary(BinaryExpr { operands, .. })
            | Expr::BinaryBoundCheck {
                bin_expr: BinaryExpr { operands, .. },
                is_overflow: _,
            }
            | Expr::Offset { operands, .. } => {
                write!(f, "{}, {}", operands.first(), operands.second())
            }
            Expr::Extension(ExtensionExpr {
                source,
                is_zero_ext,
                bits_to_add,
                ty,
            }) => write!(
                f,
                "{source}, {}{bits_to_add} as {ty}",
                if *is_zero_ext { "0" } else { "S" }
            ),
            Expr::Truncation(TruncationExpr { source, ty }) => write!(f, "{source}, |- as {ty}"),
            Expr::Ite {
                condition,
                if_target,
                else_target,
            } => write!(f, "{condition} ? {if_target} : {else_target}"),
            Expr::Transmutation { source, dst_ty } => write!(f, "{source} as {dst_ty}"),
            Expr::Multi(select) => write!(f, "{select}"),
            Expr::Ref(operand) => write!(f, "{operand}"),
            Expr::Partial(porter) => write!(f, "{porter}"),
            Expr::Concat(concat) => write!(f, "{concat}"),
            Expr::PtrMetadata(operand) => write!(f, "{operand}.meta"),
        }
    }
}

#[inline]
fn end_symbol(from_end: &bool) -> &str {
    if *from_end { "^" } else { "" }
}

impl Display for SliceIndex<SymValueRef> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}{}ˢ", self.index, if self.from_end { "^" } else { "" })
    }
}

impl Display for super::UnaryOp {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        abs::expr::UnaryOp::from(self).fmt(f)
    }
}

impl Display for super::BinaryOp {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        abs::expr::BinaryOp::from(self).fmt(f)
    }
}

impl Display for super::OverflowingBinaryOp {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let op: super::BinaryOp = (*self).into();
        op.fmt(f)
    }
}

impl Display for PorterValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{{S({})/{}}}", self.sym_values.len(), self.as_concrete)
    }
}

impl Display for ConcatExpr {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(
            f,
            "{{[{}]:{}}}",
            comma_separated(self.values.iter()),
            self.ty
        )
    }
}

impl Display for PlaceValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            PlaceValue::Deterministic(value) => write!(f, "{value}"),
            PlaceValue::Symbolic(value) => write!(f, "{value}"),
        }
    }
}

impl Display for DeterministicPlaceValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "@({:p}:{})", self.address(), self.type_info())
    }
}

impl Display for SymbolicPlaceValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.base)?;
        if let Some(proj) = &self.proj {
            write!(f, ".{{{}}}", proj)?;
        }
        Ok(())
    }
}
impl Display for SymbolicPlaceBase {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            SymbolicPlaceBase::Deref(host) => write!(f, "*{host}"),
            SymbolicPlaceBase::SymIndex(indexed) => write!(f, "{indexed}"),
        }
    }
}

impl Display for DerefSymHostPlace {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.host)
    }
}

impl Display for SymIndexedPlace {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}[{}]", self.host, self.index)
    }
}

impl Display for DeterministicProjection {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "↳{}T#{:x}", self.offset, self.ty_id)
    }
}

impl Display for SymTernaryOperands {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "({:?}, {:?}, {:?})", self.0, self.1, self.2)
    }
}
