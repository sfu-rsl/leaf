use derive_more as dm;

use core::fmt::Display;
use std::collections::HashMap;

use common::{log_debug, pri::BasicBlockLocation};

use crate::{
    abs::{
        backend::{Model, TraceManager},
        Constraint, HasTags, Tag,
    },
    solvers::{z3::Z3Solver, MapSolverExt},
    trace::{
        divergence::{DivergenceFilter, ImmediateDivergingAnswerFinder},
        inspect::CompoundTraceInspector,
        sanity_check::ConstraintSanityChecker,
        AdapterTraceManagerExt, AggregatorTraceManager, LoggerTraceManagerExt, TraceInspector,
    },
    utils::alias::RRef,
};

use super::{
    config::{
        ConstraintSanityCheckLevel, ExecutionTraceConfig, OutputConfig, SolverImpl,
        TraceInspectorType,
    },
    expr::translators::z3::Z3ValueTranslator,
    sym_vars::SymVariablesManager,
    ConstValue, Solver, SymVarId, ValueRef,
};

pub(super) type Step = BasicBlockLocation;

type CurrentSolver<'ctx> = Z3Solver<'ctx, SymVarId>;
type CurrentSolverValue<'ctx> = <Z3Solver<'ctx, SymVarId> as Solver>::Value;
type CurrentSolverCase<'ctx> = <Z3Solver<'ctx, SymVarId> as Solver>::Case;
type CurrentSolverTranslator<'ctx> = Z3ValueTranslator<'ctx>;

#[derive(Clone, Debug, dm::Deref)]
struct Tagged<T> {
    #[deref]
    value: T,
    tags: Vec<Tag>,
}

impl HasTags for Tagged<Step> {
    fn tags(&self) -> &[Tag] {
        &self.tags
    }
}

impl<T: Display> Display for Tagged<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.tags.is_empty() {
            return self.value.fmt(f);
        } else {
            write!(f, "{} #[{}]", self.value, self.tags.join(", "))
        }
    }
}

pub(super) fn new_trace_manager(
    tags: RRef<Vec<Tag>>,
    sym_var_manager: RRef<impl SymVariablesManager + 'static>,
    trace_config: &ExecutionTraceConfig,
    output_config: &Vec<OutputConfig>,
    solver_config: &SolverImpl,
) -> impl TraceManager<Step, ValueRef, ConstValue> {
    let (solver, translator) = match solver_config {
        SolverImpl::Z3 { config } => {
            crate::solvers::z3::set_global_params(
                config.global_params.iter().map(|(k, v)| (k, v.to_string())),
            );
            let solver: CurrentSolver = Z3Solver::<SymVarId>::new_in_global_context();
            let translator = Z3ValueTranslator::new(solver.context);
            (solver, translator)
        }
    };
    let sym_var_manager_ref = sym_var_manager;

    let inspectors = trace_config
        .inspectors
        .iter()
        .map(|t| match t {
            TraceInspectorType::SanityChecker { level } => create_sanity_checker(
                sym_var_manager_ref.clone(),
                translator.clone(),
                *level,
                solver.clone(),
            ),
            TraceInspectorType::DivergingInput { check_optimistic } => {
                Box::new(create_imm_diverging_ans_finder(
                    sym_var_manager_ref.clone(),
                    solver.clone(),
                    *check_optimistic,
                    output_config,
                ))
            }
        })
        .collect();

    let inspector = CompoundTraceInspector::new(inspectors);
    AggregatorTraceManager::new(inspector)
        .adapt(|s| s, translator.clone(), translator.clone())
        .logged()
        .adapt_step(move |s| Tagged {
            value: s,
            tags: tags.borrow().clone(),
        })
}

fn create_sanity_checker<'ctx, S: 'ctx>(
    sym_var_manager: RRef<impl SymVariablesManager + 'ctx>,
    translator: CurrentSolverTranslator<'ctx>,
    config: ConstraintSanityCheckLevel,
    solver: CurrentSolver<'ctx>,
) -> Box<dyn TraceInspector<S, CurrentSolverValue<'ctx>, CurrentSolverCase<'ctx>> + 'ctx> {
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

fn create_imm_diverging_ans_finder<'ctx>(
    sym_var_manager: RRef<impl SymVariablesManager + 'static>,
    solver: CurrentSolver<'ctx>,
    check_optimistic: bool,
    output_config: &Vec<OutputConfig>,
) -> impl TraceInspector<Tagged<Step>, CurrentSolverValue<'ctx>, CurrentSolverCase<'ctx>> + 'ctx {
    let mut output_generator = super::outgen::BasicOutputGenerator::new(output_config);
    let model_consumer = move |mut model: Model<SymVarId, ValueRef>| {
        // Add missing answers.
        // FIXME: Performance can be improved.
        let all_sym_values = sym_var_manager.borrow();
        let missing_answers = all_sym_values
            .iter_variables()
            .filter(|(id, _, _)| !model.contains_key(id))
            .map(|(id, _, conc)| (*id, conc.clone().0))
            .collect::<Vec<_>>();
        model.extend(missing_answers);

        output_generator.generate(&model)
    };
    let divergence_filter = DivergenceTagFilter {
        exclude_with_any_of: vec![common::pri::tags::NO_DIVERGE],
    };

    ImmediateDivergingAnswerFinder::new(
        solver.clone().map_answers(ValueRef::from),
        divergence_filter,
        check_optimistic.then(|| solver.clone().map_answers(ValueRef::from)),
        Box::new(model_consumer),
    )
}

struct DivergenceTagFilter {
    exclude_with_any_of: Vec<Tag>,
}

impl<S: HasTags> DivergenceFilter<S> for DivergenceTagFilter {
    fn should_find(&mut self, trace: &[S]) -> bool {
        let latest = trace.last().unwrap();
        let exclude = self
            .exclude_with_any_of
            .iter()
            .any(|tag| latest.has_tag(tag));

        if exclude {
            log_debug!(
                "Filtering out step with tags {:?} from divergence",
                latest.tags()
            );
        }
        !exclude
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
// FIXME: Could not make generics work here.
{
    type Item = &'a Constraint<CurrentSolverValue<'ctx>, CurrentSolverCase<'ctx>>;

    type IntoIter = std::collections::hash_map::Values<
        'a,
        SymVarId,
        Constraint<CurrentSolverValue<'ctx>, CurrentSolverCase<'ctx>>,
    >;

    fn into_iter(self) -> Self::IntoIter {
        let manager: std::cell::Ref<'a, M> = self.manager.borrow();
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

        self.constraints.values()
    }
}
