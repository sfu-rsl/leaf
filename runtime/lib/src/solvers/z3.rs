use derive_more as dm;
use z3::{
    self,
    ast::{self, Ast},
    Context, SatResult, Solver,
};

use std::{collections::HashMap, hash::Hash};

use common::log_debug;

use crate::abs::{backend, Constraint, ConstraintKind};

use self::backend::SolveResult;

/* NOTE: Why not using `Dynamic`?
 * In this way we have a little more freedom to include our information such
 * as whether the bit vector is signed or not.
 */
#[derive(Debug, Clone)]
pub(crate) enum AstNode<'ctx> {
    Bool(ast::Bool<'ctx>),
    BitVector(BVNode<'ctx>),
    Array(ArrayNode<'ctx>),
}

impl<'ctx> From<BVNode<'ctx>> for AstNode<'ctx> {
    fn from(node: BVNode<'ctx>) -> Self {
        Self::BitVector(node)
    }
}

impl<'ctx> From<ArrayNode<'ctx>> for AstNode<'ctx> {
    fn from(node: ArrayNode<'ctx>) -> Self {
        Self::Array(node)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BVNode<'ctx>(pub ast::BV<'ctx>, pub BVSort);

impl<'ctx> BVNode<'ctx> {
    pub fn new(ast: ast::BV<'ctx>, is_signed: bool) -> Self {
        Self(ast, BVSort { is_signed })
    }

    #[inline]
    pub(crate) fn map<F>(&self, f: F) -> Self
    where
        F: FnOnce(&ast::BV<'ctx>) -> ast::BV<'ctx>,
    {
        Self(f(&self.0), self.1)
    }

    #[inline(always)]
    pub(crate) fn is_signed(&self) -> bool {
        self.1.is_signed
    }

    #[inline(always)]
    pub(crate) fn size(&self) -> u32 {
        self.0.get_size()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ArrayNode<'ctx>(pub ast::Array<'ctx>, pub ArraySort);

#[derive(Debug, Clone, PartialEq, dm::From)]
pub(crate) enum AstNodeSort {
    Bool,
    BitVector(BVSort),
    Array(ArraySort),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BVSort {
    pub is_signed: bool,
}

#[derive(Debug, Clone, PartialEq, dm::From)]
pub(crate) struct ArraySort {
    pub range: Box<AstNodeSort>,
}

impl<'ctx> From<ast::Bool<'ctx>> for AstNode<'ctx> {
    fn from(ast: ast::Bool<'ctx>) -> Self {
        Self::Bool(ast)
    }
}

impl<'ctx> AstNode<'ctx> {
    pub fn from_ubv(ast: ast::BV<'ctx>) -> Self {
        BVNode::new(ast, false).into()
    }

    pub fn from_ast(ast: ast::Dynamic<'ctx>, sort: &AstNodeSort) -> Self {
        match sort {
            AstNodeSort::Bool => ast.as_bool().map(Self::Bool),
            AstNodeSort::BitVector(sort) => {
                ast.as_bv().map(|ast| Self::BitVector(BVNode(ast, *sort)))
            }
            AstNodeSort::Array(sort) => ast
                .as_array()
                .map(|ast| Self::Array(ArrayNode(ast, sort.clone()))),
        }
        .unwrap_or_else(|| {
            panic!(
                "Sort of ${:?} is not compatible with the expected one.",
                ast
            )
        })
    }
}

impl<'ctx> AstNode<'ctx> {
    pub fn as_bool(&self) -> &ast::Bool<'ctx> {
        match self {
            Self::Bool(ast) => ast,
            _ => panic!("Expected the value to be a boolean expression."),
        }
    }

    pub fn as_bit_vector(&self) -> &ast::BV<'ctx> {
        match self {
            Self::BitVector(BVNode(ast, _)) => ast,
            _ => panic!("Expected the value to be a bit vector."),
        }
    }
}

impl<'ctx> AstNode<'ctx> {
    pub fn ast(&self) -> ast::Dynamic<'ctx> {
        ast::Dynamic::from_ast(match self {
            Self::Bool(ast) => ast,
            Self::BitVector(BVNode(ast, _)) => ast,
            Self::Array(ArrayNode(ast, _)) => ast,
        })
    }

    pub fn sort(&self) -> AstNodeSort {
        match self {
            Self::Bool(_) => AstNodeSort::Bool,
            Self::BitVector(BVNode(_, sort)) => AstNodeSort::BitVector(*sort),
            Self::Array(ArrayNode(_, sort)) => AstNodeSort::Array(sort.clone()),
        }
    }

    pub fn z3_sort(&self) -> z3::Sort<'ctx> {
        match self {
            Self::Bool(ast) => ast.get_sort(),
            Self::BitVector(BVNode(ast, _)) => ast.get_sort(),
            Self::Array(ArrayNode(ast, _)) => ast.get_sort(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TranslatedValue<'ctx, I> {
    pub value: AstNode<'ctx>,
    pub variables: HashMap<I, AstNode<'ctx>>,
}

pub(crate) struct Z3Solver<'ctx, I> {
    pub(crate) context: &'ctx Context,
    solver: Solver<'ctx>,
    _phantom: core::marker::PhantomData<I>,
}

impl<'ctx, I> Z3Solver<'ctx, I> {
    pub fn new_in_global_context<'a>() -> Self {
        Self::new(context::get_context_for_thread())
    }

    pub fn new(context: &'ctx Context) -> Self {
        Self {
            context,
            solver: Solver::new(context),
            _phantom: Default::default(),
        }
    }
}

impl<'ctx, I> Clone for Z3Solver<'ctx, I> {
    fn clone(&self) -> Self {
        // Prevent cloning the assumptions in the solver
        Self::new(self.context)
    }
}

impl<'ctx, I> backend::Solver for Z3Solver<'ctx, I>
where
    I: Eq + Hash + Clone,
    Self: 'ctx,
{
    type Value = TranslatedValue<'ctx, I>;
    type Case = AstNode<'ctx>;
    type Model = HashMap<I, AstNode<'ctx>>;

    fn check<'a, 'b>(
        &'a mut self,
        constraints: impl Iterator<Item = &'b Constraint<Self::Value, Self::Case>>,
    ) -> SolveResult<Self::Model>
    where
        Self: 'b,
    {
        let mut all_vars = HashMap::<I, AstNode>::new();
        let asts = constraints
            .map(|constraint| {
                let Constraint { discr, kind } = constraint;
                use ConstraintKind::*;
                let (kind, negated) = match kind {
                    Bool => (Bool, false),
                    Not => (Bool, true),
                    OneOf(options) => (OneOf(options.iter().collect()), false),
                    NoneOf(options) => (OneOf(options.iter().collect()), true),
                };

                let ast = match kind {
                    Bool => discr.value.as_bool().clone(),
                    OneOf(cases) => {
                        let value_ast = discr.value.ast();
                        cases
                            .iter()
                            .map(|c| c.ast())
                            .map(|c| value_ast._eq(&c))
                            .reduce(|all, m| all.xor(&m))
                            .unwrap()
                    }
                    _ => unreachable!(),
                };
                all_vars.extend(
                    discr
                        .variables
                        .iter()
                        .map(|(id, node)| (id.clone(), node.clone())),
                );
                if negated { ast.not() } else { ast }
            })
            .collect::<Vec<_>>();

        let result = self.check_using(&self.solver, &asts, all_vars);
        result
    }
}

impl<'ctx, I> Z3Solver<'_, I>
where
    I: Eq + Hash,
{
    fn check_using(
        &self,
        solver: &Solver<'ctx>,
        constraints: &[ast::Bool<'ctx>],
        vars: HashMap<I, AstNode<'ctx>>,
    ) -> SolveResult<HashMap<I, AstNode<'ctx>>> {
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
                backend::SolveResult::Sat(values)
            }
            SatResult::Unsat => backend::SolveResult::Unsat,
            SatResult::Unknown => backend::SolveResult::Unknown,
        };

        solver.pop(1);
        result
    }
}

pub(crate) trait BVExt {
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

mod context {
    use std::{
        collections::HashMap,
        sync::{Mutex, OnceLock},
        thread::ThreadId,
    };

    use common::log_debug;
    use z3::Config;

    use crate::utils::{UnsafeSend, UnsafeSync};

    use super::Context;

    static CONTEXTS: OnceLock<Vec<UnsafeSync<UnsafeSend<Context>>>> = OnceLock::new();
    static THREAD_MAP: OnceLock<Mutex<HashMap<ThreadId, usize>>> = OnceLock::new();

    pub(crate) fn set_global_params<K: AsRef<str>, V: AsRef<str>>(
        params: impl Iterator<Item = (K, V)>,
    ) {
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
pub(crate) use context::set_global_params;
