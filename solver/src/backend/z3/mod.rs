mod node;
#[cfg(feature = "serde")]
pub mod serdes;
mod solve;

use std::{collections::HashMap, hash::Hash};

use z3::ast::{self, Ast};

use crate::{solver::SolveResult, solver::Solver};
use common::types::trace::Constraint;

pub use node::*;
pub use solve::{WrappedSolver, set_global_params};

pub trait BVExt {
    fn as_u128(&self) -> Option<u128>;
}

impl<'ctx> BVExt for ast::BV<'ctx> {
    fn as_u128(&self) -> Option<u128> {
        if self.get_size() <= 128 {
            unsafe {
                use std::ffi::CStr;
                Some(z3_sys::Z3_get_numeral_string(
                    self.get_ctx().get_z3_context(),
                    self.get_z3_ast(),
                ))
                .filter(|x| !x.is_null())
                .map(|x| CStr::from_ptr(x))
                .and_then(|s| s.to_str().ok())
                .and_then(|s| u128::from_str_radix(s, 10).ok())
            }
        } else {
            None
        }
    }
}

/// Z3-based solver implementation
pub type Z3Solver<'ctx, I> = WrappedSolver<'ctx, I>;

impl<'a, 'ctx: 'a, I> Solver for Z3Solver<'ctx, I>
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
        Z3Solver::check(self, constraints)
    }
}
