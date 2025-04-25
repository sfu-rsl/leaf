use delegate::delegate;
use z3::{
    self, Context, Model, Optimize, SatResult, Solver,
    ast::{self, Ast},
};

use std::prelude::rust_2021::*;
use std::{collections::HashMap, hash::Hash};

use super::super::{
    log_debug,
    types::trace::{Constraint, ConstraintKind},
    utils,
};

use super::node::*;

enum SolverImpl<'ctx> {
    Solver(Solver<'ctx>),
    Optimize(Optimize<'ctx>),
}

/// An interface for both `Solver` and `Optimize`
trait Z3Solver<'ctx> {
    fn push(&self);
    fn pop(&self);

    fn assert(&self, ast: &ast::Bool<'ctx>);
    fn check(&self) -> SatResult;
    fn get_model(&self) -> Option<Model<'ctx>>;
}

impl<'ctx> Z3Solver<'ctx> for Solver<'ctx> {
    delegate! {
        to self {
            fn push(&self);

            fn assert(&self, ast: &ast::Bool<'ctx>);
            fn check(&self) -> SatResult;
            fn get_model(&self) -> Option<Model<'ctx>>;
        }
    }

    fn pop(&self) {
        self.pop(1);
    }
}

impl<'ctx> Z3Solver<'ctx> for Optimize<'ctx> {
    delegate! {
        to self {
            fn push(&self);
            fn pop(&self);

            fn assert(&self, ast: &ast::Bool<'ctx>);
            fn get_model(&self) -> Option<Model<'ctx>>;
        }
    }

    fn check(&self) -> SatResult {
        self.check(&[])
    }
}

impl<'ctx> Z3Solver<'ctx> for SolverImpl<'ctx> {
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
            fn assert(&self, ast: &ast::Bool<'ctx>);
            #[through(Z3Solver)]
            fn check(&self) -> SatResult;
            #[through(Z3Solver)]
            fn get_model(&self) -> Option<Model<'ctx>>;
        }
    }
}

pub struct WrappedSolver<'ctx, I> {
    context: &'ctx Context,
    solver: SolverImpl<'ctx>,
    _phantom: core::marker::PhantomData<(I,)>,
}

impl<'ctx, I> WrappedSolver<'ctx, I> {
    pub fn new_in_global_context() -> Self {
        Self::new(context::get_context_for_thread())
    }

    pub fn new(context: &'ctx Context) -> Self {
        Self {
            context,
            solver: SolverImpl::Solver(Solver::new(context)),
            _phantom: Default::default(),
        }
    }

    pub fn context(&self) -> &'ctx Context {
        self.context
    }
}

impl<I> Default for WrappedSolver<'_, I> {
    fn default() -> Self {
        Self::new_in_global_context()
    }
}

impl<'ctx, I> Clone for WrappedSolver<'ctx, I> {
    fn clone(&self) -> Self {
        // Prevent cloning the assumptions in the solver
        Self::new(self.context)
    }
}

impl<'ctx, I> WrappedSolver<'ctx, I>
where
    I: Eq + Hash,
{
    pub fn check(
        &self,
        constraints: impl Iterator<Item = Constraint<AstAndVars<'ctx, I>, AstNode<'ctx>>>,
    ) -> (SatResult, HashMap<I, AstNode<'ctx>>) {
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
                            .map(|c| value_ast._eq(&c))
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
        solver: &(impl Z3Solver<'ctx> + ?Sized),
        constraints: &[ast::Bool<'ctx>],
        vars: HashMap<I, AstNode<'ctx>>,
    ) -> (SatResult, HashMap<I, AstNode<'ctx>>) {
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

impl<'ctx, I> WrappedSolver<'ctx, I>
where
    I: Eq + Hash,
{
    pub fn consider_possible_answer(&mut self, var: AstNode<'ctx>, answer: AstNode<'ctx>) {
        if let SolverImpl::Solver(..) = self.solver {
            self.solver = SolverImpl::Optimize(Optimize::new(self.context));
        }
        let SolverImpl::Optimize(optimize) = &mut self.solver else {
            unreachable!();
        };

        optimize.assert_soft(&var.dyn_ast()._eq(&answer.dyn_ast()), 1, None);
    }
}

mod context {
    use std::{
        collections::HashMap,
        sync::{Mutex, OnceLock},
        thread::ThreadId,
    };

    use z3::Config;

    use super::*;
    use utils::{UnsafeSend, UnsafeSync};

    static CONTEXTS: OnceLock<Vec<UnsafeSync<UnsafeSend<Context>>>> = OnceLock::new();
    static THREAD_MAP: OnceLock<Mutex<HashMap<ThreadId, usize>>> = OnceLock::new();

    pub fn set_global_params<K: AsRef<str>, V: AsRef<str>>(params: impl Iterator<Item = (K, V)>) {
        for (k, v) in params {
            log_debug!("Setting global param: {} = {}", k.as_ref(), v.as_ref());
            z3::set_global_param(k.as_ref(), v.as_ref());
        }
    }

    fn init_contexts() -> Vec<UnsafeSync<UnsafeSend<Context>>> {
        // Statically allocate some in advance.
        const TOTAL_CONTEXTS: usize = 1;

        let mut list = Vec::with_capacity(TOTAL_CONTEXTS);
        for _ in 0..TOTAL_CONTEXTS {
            list.push(UnsafeSync::new(UnsafeSend::new(Context::new(
                &Config::new(),
            ))));
        }
        list
    }

    pub(super) fn get_context_for_thread() -> &'static Context {
        let contexts = CONTEXTS.get_or_init(init_contexts);
        let thread_id = std::thread::current().id();
        let mut thread_map = THREAD_MAP.get_or_init(Default::default).lock().unwrap();
        let accessor_count = thread_map.len();
        let index = *thread_map.entry(thread_id).or_insert(accessor_count);
        let context = &contexts
            .get(index)
            .expect("Unexpected number of threads to access Z3 context");
        *context
    }
}
pub use context::set_global_params;
