use delegate::delegate;
use z3::{
    self, Model, Optimize, SatResult, Solver,
    ast::{self},
};

use std::prelude::rust_2024::*;
use std::{collections::HashMap, hash::Hash};

use super::super::{
    log_debug,
    types::trace::{Constraint, ConstraintKind},
};

use super::node::*;

enum SolverImpl {
    Solver(Solver),
    Optimize(Optimize),
}

/// An interface for both `Solver` and `Optimize`
trait Z3Solver {
    fn push(&self);
    fn pop(&self);

    fn assert(&self, ast: &ast::Bool);
    fn check(&self) -> SatResult;
    fn get_model(&self) -> Option<Model>;
}

impl Z3Solver for Solver {
    delegate! {
        to self {
            fn push(&self);

            fn assert(&self, ast: &ast::Bool);
            fn check(&self) -> SatResult;
            fn get_model(&self) -> Option<Model>;
        }
    }

    fn pop(&self) {
        self.pop(1);
    }
}

impl Z3Solver for Optimize {
    delegate! {
        to self {
            fn push(&self);
            fn pop(&self);

            fn assert(&self, ast: &ast::Bool);
            fn get_model(&self) -> Option<Model>;
        }
    }

    fn check(&self) -> SatResult {
        self.check(&[])
    }
}

impl Z3Solver for SolverImpl {
    delegate! {
        to match self {
            Self::Solver(solver) => solver,
            Self::Optimize(optimize) => optimize,
        } {
            #[through(Z3Solver)]
            fn push(&self);
            #[through(Z3Solver)]
            fn pop(&self);

            #[through(Z3Solver)]
            fn assert(&self, ast: &ast::Bool);
            #[through(Z3Solver)]
            fn check(&self) -> SatResult;
            #[through(Z3Solver)]
            fn get_model(&self) -> Option<Model>;
        }
    }
}

pub struct WrappedSolver<I> {
    solver: SolverImpl,
    _phantom: core::marker::PhantomData<(I,)>,
}

impl<I> WrappedSolver<I> {
    pub fn new_in_global_context() -> Self {
        Self::new()
    }

    pub fn new() -> Self {
        Self {
            solver: SolverImpl::Solver(Solver::new()),
            _phantom: Default::default(),
        }
    }

    // pub fn context(&self) -> &'ctx Context {
    //     self.context
    // }
}

impl<I> Default for WrappedSolver<I> {
    fn default() -> Self {
        Self::new_in_global_context()
    }
}

impl<I> Clone for WrappedSolver<I> {
    fn clone(&self) -> Self {
        // Prevent cloning the assumptions in the solver
        Self::new()
    }
}

impl<I> WrappedSolver<I>
where
    I: Eq + Hash,
{
    pub fn check(
        &self,
        constraints: impl Iterator<Item = Constraint<AstAndVars<I>, AstNode>>,
    ) -> (SatResult, HashMap<I, AstNode>) {
        let mut all_vars = HashMap::<I, AstNode>::new();
        let asts = constraints
            .map(|constraint| {
                let Constraint { discr, kind } = constraint;
                use ConstraintKind::*;
                let (kind, negated) = match kind {
                    True => (True, false),
                    False => (True, true),
                    OneOf(options) => (OneOf(options), false),
                    NoneOf(options) => (OneOf(options), true),
                };

                let ast = match kind {
                    True => discr.value.as_bool().clone(),
                    OneOf(cases) => {
                        let value_ast = ast::Dynamic::from_ast(discr.value.ast());
                        cases
                            .iter()
                            .map(|c| ast::Dynamic::from_ast(c.ast()))
                            .map(|c| ast::Dynamic::eq(&value_ast, &c))
                            .reduce(|all, m| all.xor(&m))
                            .unwrap()
                    }
                    _ => unreachable!(),
                };
                all_vars.extend(discr.variables.into_iter());
                if negated { ast.not() } else { ast }
            })
            .collect::<Vec<_>>();

        self.check_using(&self.solver, &asts, all_vars)
    }

    fn check_using(
        &self,
        solver: &(impl Z3Solver + ?Sized),
        constraints: &[ast::Bool],
        vars: HashMap<I, AstNode>,
    ) -> (SatResult, HashMap<I, AstNode>) {
        log_debug!("Sending constraints to Z3: {:#?}", constraints);

        solver.push();

        for constraint in constraints {
            solver.assert(constraint);
        }

        let result = match solver.check() {
            SatResult::Sat => {
                let model = solver.get_model().unwrap();
                let mut values = HashMap::new();
                for (id, node) in vars {
                    let value = match node {
                        AstNode::Bool(ast) => AstNode::Bool(model.eval(&ast, true).unwrap()),
                        AstNode::BitVector(BVNode(ast, is_signed)) => {
                            AstNode::BitVector(BVNode(model.eval(&ast, true).unwrap(), is_signed))
                        }
                        AstNode::Array(ArrayNode(ast, sort)) => {
                            AstNode::Array(ArrayNode(model.eval(&ast, true).unwrap(), sort))
                        }
                    };
                    values.insert(id, value.into());
                }
                (SatResult::Sat, values)
            }
            result @ (SatResult::Unsat | SatResult::Unknown) => (result, HashMap::new()),
        };

        solver.pop();
        result
    }
}

impl<I> WrappedSolver<I>
where
    I: Eq + Hash,
{
    pub fn consider_possible_answer(&mut self, var: AstNode, answer: AstNode) {
        if let SolverImpl::Solver(..) = self.solver {
            self.solver = SolverImpl::Optimize(Optimize::new());
        }
        let SolverImpl::Optimize(optimize) = &mut self.solver else {
            unreachable!();
        };

        optimize.assert_soft(
            &ast::Dynamic::eq(&var.dyn_ast(), &answer.dyn_ast()),
            1,
            None,
        );
    }
}

pub fn set_global_params<K: AsRef<str>, V: AsRef<str>>(params: impl Iterator<Item = (K, V)>) {
    for (k, v) in params {
        log_debug!("Setting global param: {} = {}", k.as_ref(), v.as_ref());
        z3::set_global_param(k.as_ref(), v.as_ref());
    }
}
