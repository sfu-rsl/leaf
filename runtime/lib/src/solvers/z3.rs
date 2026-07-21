use z3::SatResult;

use std::{collections::HashMap, hash::Hash};

pub use common::z3::set_global_params;
use common::z3::*;

use crate::abs::{Constraint, IntType, ValueType, backend};

use self::backend::SolveResult;

pub type Z3Solver<I> = common::z3::WrappedSolver<I>;

impl<'a, I> backend::Solver for Z3Solver<I>
where
    I: Eq + Hash + Clone,
    Self:,
{
    type Value = AstAndVars<I>;
    type Case = AstNode;
    type Model = HashMap<I, AstNode>;

    fn check(
        &mut self,
        constraints: impl Iterator<Item = Constraint<Self::Value, Self::Case>>,
    ) -> SolveResult<Self::Model> {
        match Z3Solver::check(self, constraints) {
            (SatResult::Sat, model) => SolveResult::Sat(model),
            (SatResult::Unsat, _) => SolveResult::Unsat,
            (SatResult::Unknown, _) => SolveResult::Unknown,
        }
    }
}

impl TryFrom<ValueType> for BVSort {
    type Error = ValueType;

    fn try_from(value_type: ValueType) -> Result<Self, Self::Error> {
        match value_type {
            ValueType::Int(IntType {
                bit_size: _,
                is_signed,
            }) => Ok(Self { is_signed }),
            _ => Err(value_type),
        }
    }
}
