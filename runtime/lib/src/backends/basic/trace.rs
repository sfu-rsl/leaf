use derive_more as dm;

use core::fmt::Display;
use std::collections::HashMap;

use common::{log_debug, pri::BasicBlockLocation};

use crate::{
    abs::{backend::Model, Constraint, HasTags, Tag},
    solvers::{z3::Z3Solver, MapSolverExt},
    trace::{
        divergence::{DivergenceFilter, ImmediateDivergingAnswerFinder},
        sanity_check::ConstraintSanityChecker,
        AdapterTraceManagerExt, AggregatorTraceManager, BranchCoverageStepInspector,
        InspectionTraceManagerExt, LoggerTraceManagerExt, StepInspector, TraceInspector,
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
    ConstValue, Solver, SymVarId, TraceManager, ValueRef,
};

#[derive(PartialEq, Eq, Hash, Clone, Debug, Default, dm::Deref, dm::From, dm::Display)]
pub(super) struct Step(BasicBlockLocation);

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
) -> impl TraceManager {
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
        .filter(|t| is_inner_inspector(t))
        .map(|t| match t {
            TraceInspectorType::SanityChecker { level } => sanity_check::create_sanity_checker(
                sym_var_manager_ref.clone(),
                translator.clone(),
                *level,
                solver.clone(),
            ),
            TraceInspectorType::DivergingInput { check_optimistic } => {
                Box::new(divergence::create_imm_diverging_ans_finder(
                    sym_var_manager_ref.clone(),
                    solver.clone(),
                    *check_optimistic,
                    output_config,
                ))
            }
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();

    let core_manager = AggregatorTraceManager::new(inspectors)
        .adapt(|s| s, translator.clone(), translator.clone())
        .logged()
        .adapt_step(move |s| Tagged {
            value: s,
            tags: tags.borrow().clone(),
        });

    let manager = core_manager;

    let mut cov_dumper = None;
    let inspectors = trace_config
        .inspectors
        .iter()
        .filter(|t| !is_inner_inspector(t))
        .map(|t| match t {
            TraceInspectorType::BranchCoverage { output } => {
                let (cov_inspector, dumper) = coverage::create_branch_coverage_collector(output);
                cov_dumper = dumper;
                Box::new(cov_inspector) as Box<dyn StepInspector<_, _, _>>
            }
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();
    manager.inspected_by(inspectors).on_shutdown(move || {
        if let Some(ref mut dumper) = cov_dumper {
            dumper().expect("Could not dump branch coverage");
        }
    })
}

fn is_inner_inspector(t: &TraceInspectorType) -> bool {
    use TraceInspectorType::*;
    match t {
        SanityChecker { .. } | DivergingInput { .. } => true,
        BranchCoverage { .. } => false,
    }
}

mod sanity_check {
    use super::*;

    pub(super) fn create_sanity_checker<'ctx, S: 'ctx>(
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
}

mod divergence {
    use super::super::outgen::BasicOutputGenerator;

    use super::*;

    pub(super) fn create_imm_diverging_ans_finder<'ctx>(
        sym_var_manager: RRef<impl SymVariablesManager + 'static>,
        solver: CurrentSolver<'ctx>,
        check_optimistic: bool,
        output_config: &Vec<OutputConfig>,
    ) -> impl TraceInspector<Tagged<Step>, CurrentSolverValue<'ctx>, CurrentSolverCase<'ctx>> + 'ctx
    {
        let mut output_generator = BasicOutputGenerator::new(output_config);
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
}

mod coverage {
    use std::{cell::RefCell, fs, rc::Rc};

    use serde::{ser::SerializeStruct, Serialize, Serializer};

    use crate::{abs::IntType, backends::basic::config::OutputFileFormat};

    use super::*;

    type Dumper = Box<dyn FnMut() -> Result<(), String>>;

    type Inspector = BranchCoverageStepInspector<Step, ValueRef, ConstValue>;

    pub(super) fn create_branch_coverage_collector(
        output_config: &Option<OutputConfig>,
    ) -> (RRef<Inspector>, Option<Dumper>) {
        let inspector_ref = Rc::new(RefCell::new(BranchCoverageStepInspector::new()));
        let dumper = output_config
            .as_ref()
            .map(|cfg| create_serializer(cfg, inspector_ref.clone()));
        (inspector_ref.clone(), dumper)
    }

    impl Serialize for Step {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut s = serializer.serialize_struct(stringify!(BasicBlockLocation), 2)?;
            s.serialize_field("body", &(self.body.0, self.body.1))?;
            s.serialize_field("index", &self.index)?;
            s.end()
        }
    }

    impl Serialize for ConstValue {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            #[derive(Serialize)]
            enum PossibleCase {
                Bool(bool),
                Char(char),
                Int { bit_rep: u128, ty: IntType },
            }

            let case = match self {
                ConstValue::Bool(b) => PossibleCase::Bool(*b),
                ConstValue::Char(c) => PossibleCase::Char(*c),
                ConstValue::Int { bit_rep, ty } => PossibleCase::Int {
                    bit_rep: bit_rep.0,
                    ty: *ty,
                },
                _ => unreachable!("Unexpected constant value for case"),
            };

            case.serialize(serializer)
        }
    }

    fn create_serializer(config: &OutputConfig, inspector: RRef<Inspector>) -> Dumper {
        match config {
            OutputConfig::File(file_config) => match file_config.format {
                OutputFileFormat::Json => {
                    let file = fs::File::create(
                        file_config.directory.join(
                            file_config
                                .prefix
                                .clone()
                                .unwrap_or("branch_cov".to_owned())
                                + &file_config.extension.clone().unwrap_or(".json".to_owned()),
                        ),
                    )
                    .expect("Could not create file for branch coverage");
                    let mut serializer = serde_json::Serializer::new(file);
                    Box::new(move || {
                        log_debug!("Dumping branch coverage to file");
                        inspector
                            .borrow()
                            .get_coverage()
                            .iter()
                            .collect::<Vec<_>>()
                            .serialize(&mut serializer)
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    })
                }
                OutputFileFormat::Binary => {
                    unimplemented!("Binary output format is not supported yet")
                }
            },
        }
    }
}

mod shutdown {
    use delegate::delegate;

    use crate::abs::backend::{Shutdown, TraceManager as AbsTraceManager};

    use super::*;

    pub(super) struct ShutdownWrapper<T, F> {
        inner: T,
        f: F,
    }

    impl<T, F> ShutdownWrapper<T, F> {
        pub(super) fn new(inner: T, f: F) -> Self {
            Self { inner, f }
        }
    }

    impl<T, F> Shutdown for ShutdownWrapper<T, F>
    where
        F: FnMut(),
    {
        fn shutdown(&mut self) {
            (self.f)();
        }
    }

    impl<T, F, S, V, C> AbsTraceManager<S, V, C> for ShutdownWrapper<T, F>
    where
        T: AbsTraceManager<S, V, C>,
    {
        delegate! {
            to self.inner {
                fn notify_step(&mut self, step: S, constraint: Constraint<V, C>);
            }
        }
    }

    pub(super) trait TraceManagerExt<S, V, C>: AbsTraceManager<S, V, C> {
        fn on_shutdown(self, f: impl FnMut()) -> impl AbsTraceManager<S, V, C> + Shutdown;
    }
    impl<S, V, C, M: AbsTraceManager<S, V, C>> TraceManagerExt<S, V, C> for M {
        fn on_shutdown(self, f: impl FnMut()) -> impl AbsTraceManager<S, V, C> + Shutdown {
            ShutdownWrapper::new(self, f)
        }
    }
}
use shutdown::TraceManagerExt as ShutdownTraceManagerExt;
