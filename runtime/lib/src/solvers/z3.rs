use z3::SatResult;

use std::{collections::HashMap, hash::Hash};

pub use common::z3::set_global_params;
use common::z3::*;

use crate::abs::{Constraint, backend};

use self::backend::SolveResult;

pub(crate) type Z3Solver<'ctx, I> = common::z3::WrappedSolver<'ctx, I>;

impl<'a, 'ctx: 'a, I> backend::Solver for Z3Solver<'ctx, I>
where
    I: Eq + Hash + Clone,
    Self: 'ctx,
{
    type Value = AstAndVars<'ctx, I>;
    type Case = AstNode<'ctx>;
    type Model = HashMap<I, AstNode<'ctx>>;

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
