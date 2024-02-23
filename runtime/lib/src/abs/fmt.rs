use std::fmt::{Display, Formatter, Result};

use crate::backends::basic::logger::comma_separated;

use super::{
    expr::sym_place::{Select, SelectTarget},
    *,
};

impl<L, P> Display for Place<L, P>
where
    L: Display,
    for<'a> &'a P: Into<&'a Projection<L>>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        PlaceFormatter::format(f, self)
    }
}

struct PlaceFormatter;
impl PlaceFormatter {
    fn format<L, P>(f: &mut std::fmt::Formatter, place: &Place<L, P>) -> std::fmt::Result
    where
        L: Display,
        for<'a> &'a P: Into<&'a Projection<L>>,
    {
        place
            .projections()
            .iter()
            .try_for_each(|proj| Self::pre(proj.into(), f))
            .and_then(|_| write!(f, "{}", place.local()))
            .and_then(|_| {
                place
                    .projections()
                    .iter()
                    .rev()
                    .try_for_each(|proj| Self::post(proj.into(), f))
            })
    }

    fn pre<L>(proj: &Projection<L>, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match proj {
            Projection::Deref => f.write_str("*"),
            _ => Result::Ok(()),
        }
    }

    fn post<L>(proj: &Projection<L>, f: &mut std::fmt::Formatter) -> std::fmt::Result
    where
        L: Display,
    {
        match proj {
            Projection::Field(field) => write!(f, ".{field}"),
            Projection::Index(index) => write!(f, "[{}]", index),
            Projection::Subslice { from, to, from_end } => {
                write!(f, "[{}..{}{}]", from, to, if *from_end { "^" } else { "" })
            }
            Projection::ConstantIndex {
                offset,
                min_length,
                from_end,
            } => {
                write!(
                    f,
                    "{{>{}}}[{}{}]",
                    min_length,
                    offset,
                    if *from_end { "^" } else { "" }
                )
            }
            Projection::Downcast(variant) => write!(f, " as V#{variant}"),
            _ => Result::Ok(()),
        }
    }
}

impl Display for BinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
            BinaryOp::BitAnd => "&",
            BinaryOp::BitOr => "|",
            BinaryOp::BitXor => "^",
            BinaryOp::Shl => "<<",
            BinaryOp::Shr => ">>",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::Offset => "->",
        })
    }
}

impl Display for UnaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            UnaryOp::Not => "!",
            UnaryOp::Neg => "-",
        })
    }
}

impl<V> Display for Constraint<V>
where
    V: Display,
{
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            Constraint::Bool(value) => write!(f, "({})", value),
            Constraint::Not(value) => write!(f, "!({})", value),
        }
    }
}

impl Display for ValueType {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            Self::Bool => write!(f, "bool"),
            Self::Char => write!(f, "char"),
            Self::Int(int) => write!(f, "{}", int),
            Self::Float(float) => write!(f, "{}", float),
        }
    }
}

impl Display for IntType {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(
            f,
            "{}{}",
            if self.is_signed { 'i' } else { 'u' },
            self.bit_size
        )
    }
}

impl Display for FloatType {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(f, "f{}", self.e_bits + self.s_bits)
    }
}

impl Display for Local {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            Self::ReturnValue => write!(f, "ReturnValue"),
            Self::Argument(local) => write!(f, "Arg({})", local),
            Self::Normal(local) => write!(f, "Var({})", local),
        }
    }
}

impl<I, V> Display for Select<I, V>
where
    I: Display,
    V: Display,
{
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(f, "{}[{}]", self.target, self.index)
    }
}

impl<V, S> Display for SelectTarget<V, S>
where
    V: Display,
    S: Display,
{
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            SelectTarget::Array(values) => write!(f, "{}", comma_separated(values.iter())),
            SelectTarget::Nested(box select) => write!(f, "{select}"),
        }
    }
}