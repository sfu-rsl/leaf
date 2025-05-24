use core::{borrow::Borrow, fmt::Debug};
use std::{cell::RefCell, collections::HashMap};

use crate::{
    abs::Constraint,
    trace::{TraceInspector, sanity_check::ConstraintSanityChecker},
    utils::alias::RRef,
};

use super::{
    CurrentSolver, CurrentSolverCase, CurrentSolverTranslator, CurrentSolverValue,
    backend::{SymVarId, SymVariablesManager, config::ConstraintSanityCheckLevel},
};

pub(super) fn create_sanity_checker<'ctx, S: 'ctx, V: 'ctx, C: 'ctx>(
    sym_var_manager: RRef<impl SymVariablesManager + 'ctx>,
    translator: CurrentSolverTranslator<'ctx>,
    config: ConstraintSanityCheckLevel,
    solver: CurrentSolver<'ctx>,
) -> Box<dyn TraceInspector<S, V, C> + 'ctx>
where
    V: Borrow<CurrentSolverValue<'ctx>>,
    C: Borrow<CurrentSolverCase<'ctx>>,
    V: Debug,
    C: Debug,
{
    let assumptions = ConcretizationConstraintsCache::new(sym_var_manager, translator);
    match config {
        ConstraintSanityCheckLevel::Warn => {
            Box::new(ConstraintSanityChecker::new::<false>(solver, assumptions))
        }
        ConstraintSanityCheckLevel::Panic => {
            Box::new(ConstraintSanityChecker::new::<true>(solver, assumptions))
        }
    }
}

struct ConcretizationConstraintsCache<M: SymVariablesManager, T, V, C> {
    manager: RRef<M>,
    translator: T,
    constraints: HashMap<SymVarId, Constraint<V, C>>,
}

impl<M: SymVariablesManager, T, V, C> ConcretizationConstraintsCache<M, T, V, C> {
    fn new(manager: RRef<M>, translator: T) -> Self {
        Self {
            manager,
            translator,
            constraints: HashMap::new(),
        }
    }
}

impl<'a, 'ctx, M: SymVariablesManager> IntoIterator
    for &'a mut ConcretizationConstraintsCache<
        M,
        CurrentSolverTranslator<'ctx>,
        CurrentSolverValue<'ctx>,
        CurrentSolverCase<'ctx>,
    >
{
    // FIXME: Could not make generics work here.
    type Item = Constraint<CurrentSolverValue<'ctx>, CurrentSolverCase<'ctx>>;

    type IntoIter = Box<
        dyn Iterator<Item = Constraint<CurrentSolverValue<'ctx>, CurrentSolverCase<'ctx>>> + 'a,
    >;

    fn into_iter(self) -> Self::IntoIter {
        let manager: std::cell::Ref<'a, M> = RefCell::borrow(&self.manager);
        let all_constraints = manager.iter_concretization_constraints();
        if all_constraints.len() > self.constraints.len() {
            self.constraints.extend(all_constraints.map(|(k, c)| {
                (
                    *k,
                    c.clone()
                        .map(&mut self.translator, &mut |c| c)
                        .map(&mut |d| d, &mut self.translator),
                )
            }));
        }

        Box::new(self.constraints.values().cloned())
    }
}
