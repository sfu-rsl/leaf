use derive_more as dm;
use serde::Serialize;

use core::{
    borrow::Borrow,
    fmt::{Debug, Display},
    time::Duration,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

use common::{log_debug, pri::BasicBlockLocation};

use crate::{
    abs::{Constraint, HasTags, Tag, backend::Model},
    solvers::{MapSolverExt, z3::Z3Solver},
    trace::{
        AdapterTraceManagerExt, AggregatorStepInspector, AggregatorTraceManager,
        BranchCoverageStepInspector, FilterStepInspectorExt, FilterTraceManagerExt,
        InspectionTraceManagerExt, LoggerTraceManagerExt, StepInspector, TraceInspector,
        divergence::{DivergenceFilter, ImmediateDivergingAnswerFinder},
        sanity_check::ConstraintSanityChecker,
    },
    utils::{RefView, alias::RRef},
};

use super::{
    ConstValue, Solver, SymVarId, TraceManager, ValueRef,
    config::{self, ExecutionTraceConfig, OutputConfig, SolverImpl, TraceInspectorType},
    expr::translators::z3::Z3ValueTranslator,
    sym_vars::SymVariablesManager,
};

use dumping::*;
pub(crate) use helpers::Step;
use helpers::*;

type CurrentSolver<'ctx> = Z3Solver<'ctx, SymVarId>;
type CurrentSolverValue<'ctx> = <CurrentSolver<'ctx> as Solver>::Value;
type CurrentSolverCase<'ctx> = <CurrentSolver<'ctx> as Solver>::Case;
type CurrentSolverTranslator<'ctx> = Z3ValueTranslator<'ctx>;

// These are the types of steps, values, and cases for the inner managers.
// You can use them to give explicit types as opposed to generics.
type IStep = Tagged<Indexed<Step>>;
type IValue<'ctx> = Translation<ValueRef, CurrentSolverValue<'ctx>>;
type ICase<'ctx> = Translation<ConstValue, CurrentSolverCase<'ctx>>;

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

    let mut dumpers: Vec<Box<dyn Dumper>> = vec![];

    let mut cov_inspector = None;
    let inspectors = trace_config
        .inspectors
        .iter()
        .filter(|t| !is_inner_inspector(t))
        .map(|t| match t {
            TraceInspectorType::BranchCoverage { output, .. } => {
                let (inspector, dumper) =
                    coverage::create_branch_coverage_collector::<ValueRef>(output);
                cov_inspector = Some(inspector.clone());
                dumpers.extend_opt(dumper);
                Box::new(inspector) as Box<dyn StepInspector<_, _, _>>
            }
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();

    let inner_inspectors = trace_config
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
            TraceInspectorType::DivergingInput {
                check_optimistic,
                filters,
            } => {
                let (inspector, dumper) = divergence::create_imm_diverging_ans_finder(
                    sym_var_manager_ref.clone(),
                    solver.clone(),
                    *check_optimistic,
                    filters,
                    cov_inspector.clone(),
                    output_config,
                );
                dumpers.push(Box::new(dumper));
                Box::new(inspector)
            }
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();

    let mut value_translator = translator.clone();
    let mut case_translator = translator.clone();

    let agg_manager = AggregatorTraceManager::new(inner_inspectors);
    dumpers.extend_opt(trace_config.constraints_dump.as_ref().map(|cfg| {
        dumper::create_solver_constraints_dumper(
            cfg,
            agg_manager.steps(),
            agg_manager.constraints(),
        )
    }));

    let core_manager = agg_manager.adapt(
        |s| s,
        move |v: ValueRef| Translation::of(v, &mut value_translator),
        move |c: ConstValue| Translation::of(c, &mut case_translator),
    );
    let manager = core_manager;

    let outer_agg_inspector = AggregatorStepInspector::default();
    dumpers.extend_opt(trace_config.decisions_dump.as_ref().map(|cfg| {
        dumper::create_decisions_dumper(
            cfg,
            outer_agg_inspector.steps(),
            outer_agg_inspector.constraints(),
        )
    }));
    let dumpers_ref = Rc::new(RefCell::new(dumpers));

    let mut counter = 0;

    manager
        .logged()
        .adapt_step(move |s| Tagged {
            value: s,
            tags: RefCell::borrow(&tags).clone(),
        })
        .inspected_by(inspectors)
        .filtered_by(|_, c| c.discr.is_symbolic())
        .inspected_by(outer_agg_inspector)
        .adapt_step(move |s: Step| {
            counter += 1;
            Indexed {
                index: counter,
                value: s,
            }
        })
        .inspected_by(dumping::create_timer_dumper_inspector(
            dumpers_ref.clone(),
            trace_config
                .dump_interval
                .map(|i| Duration::from_secs(i.into())),
        ))
        .on_shutdown(move || dump(&dumpers_ref))
}

fn is_inner_inspector(t: &TraceInspectorType) -> bool {
    use TraceInspectorType::*;
    match t {
        SanityChecker { .. } | DivergingInput { .. } => true,
        BranchCoverage { .. } => false,
    }
}

mod sanity_check {
    use std::cell::RefCell;

    use super::{config::ConstraintSanityCheckLevel, *};

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
}

mod divergence {
    use std::rc::Rc;

    use common::log_info;

    use crate::trace::divergence::{
        BranchCoverageDepthDivergenceFilter, DepthProvider, filter::all,
    };

    use super::*;
    use super::{super::outgen::BasicOutputGenerator, config::DivergenceFilterType};

    pub(super) fn create_imm_diverging_ans_finder<'ctx, V: 'ctx, C: 'ctx>(
        sym_var_manager: RRef<impl SymVariablesManager + 'static>,
        solver: CurrentSolver<'ctx>,
        check_optimistic: bool,
        filters_config: &Vec<DivergenceFilterType>,
        branch_depth_provider: Option<RRef<impl DepthProvider<Step, ConstValue> + 'ctx>>,
        output_config: &Vec<OutputConfig>,
    ) -> (impl TraceInspector<IStep, V, C> + 'ctx, impl Dumper + 'ctx)
    where
        V: Borrow<CurrentSolverValue<'ctx>>,
        C: Borrow<CurrentSolverCase<'ctx>>,
        C: Borrow<ConstValue>,
    {
        let mut output_generator = BasicOutputGenerator::new(output_config);
        let model_consumer = move |mut model: Model<SymVarId, ValueRef>| {
            // Add missing answers.
            // FIXME: Performance can be improved.
            let all_sym_values = RefCell::borrow(&sym_var_manager);
            let missing_answers = all_sym_values
                .iter_variables()
                .filter(|(id, _, _)| !model.contains_key(id))
                .map(|(id, _, conc)| (*id, conc.clone().0))
                .collect::<Vec<_>>();
            model.extend(missing_answers);

            output_generator.generate(&model)
        };

        let mut filters: Vec<Box<dyn DivergenceFilter<IStep, V, C> + '_>> = vec![];
        let mut dumpers: Vec<Box<dyn Dumper>> = vec![];

        // This filter is builtin and not overridable.
        filters.push(Box::new(DivergenceTagFilter::new(&[
            common::pri::tags::NO_DIVERGE.to_owned(),
        ])));

        filters.extend(
            filters_config
                .iter()
                .map::<Box<dyn DivergenceFilter<_, _, _>>, _>(|f| match f {
                    DivergenceFilterType::Tags { exclude_any_of } => {
                        Box::new(DivergenceTagFilter::new(&exclude_any_of))
                    }
                    DivergenceFilterType::BranchDepthDistance {
                        distance_threshold_factor,
                        persistence,
                    } => {
                        let (filter, dumper) = create_branch_depth_filter(
                            branch_depth_provider.clone().expect(
                                "Branch coverage info is required. Check if the inspector is added correctly.",
                            ),
                            *distance_threshold_factor,
                            persistence.as_ref(),
                        );
                        dumpers.extend_opt(dumper);
                        Box::new(filter)
                    }
                }),
        );

        let inspector = ImmediateDivergingAnswerFinder::new(
            solver.clone().map_answers(ValueRef::from),
            all(filters),
            check_optimistic.then(|| solver.clone().map_answers(ValueRef::from)),
            Box::new(model_consumer),
        );
        (inspector, dumpers)
    }

    struct DivergenceTagFilter {
        exclude_with_any_of: Vec<String>,
    }

    impl DivergenceTagFilter {
        fn new(exclude_with_any_of: &[String]) -> Self {
            Self {
                exclude_with_any_of: exclude_with_any_of.to_vec(),
            }
        }
    }

    impl<S: HasTags, V, C> DivergenceFilter<S, V, C> for DivergenceTagFilter {
        fn should_find(&mut self, trace: &[S], _constraints: &[Constraint<V, C>]) -> bool {
            let latest = trace.last().unwrap();
            let exclude = self
                .exclude_with_any_of
                .iter()
                .filter(|t| latest.has_tag(t))
                .inspect(|t| {
                    log_debug!(
                        "Filtering out step with tags {:?} by `{}` from divergence",
                        latest.tags(),
                        t,
                    )
                })
                .next()
                .is_some();
            !exclude
        }
    }

    fn create_branch_depth_filter<'ctx, S: 'ctx, V: 'ctx, C: 'ctx>(
        branch_depth_provider: RRef<impl DepthProvider<Step, ConstValue> + 'ctx>,
        distance_threshold_factor: f32,
        persistence: Option<&OutputConfig>,
    ) -> (impl DivergenceFilter<S, V, C> + 'ctx, Option<impl Dumper>)
    where
        S: Borrow<Step>,
        V: Borrow<CurrentSolverValue<'ctx>>,
        C: Borrow<ConstValue>,
    {
        const FILENAME_DEFAULT: &str = "branch_cov_depth";

        let persistence = persistence.map(|cfg| match cfg {
            OutputConfig::File(cfg) => cfg,
        });

        let snapshot = persistence.and_then(|cfg| {
            deserialize_snapshot::<Vec<((Step, Vec<SymVarId>), usize)>>(cfg, FILENAME_DEFAULT)
                .map(|r| r.expect("Problem in loading branch coverage depth snapshot"))
                .inspect(|s| {
                    log_info!(
                        "Previous branch coverage depth snapshot loaded with {} entries",
                        s.len()
                    )
                })
                .map(|s| HashMap::from_iter(s))
                .or_else(|| {
                    log_debug!("No previous branch coverage depth snapshot found");
                    None
                })
        });

        let filter = BranchCoverageDepthDivergenceFilter::new(
            snapshot,
            branch_depth_provider,
            distance_threshold_factor,
            |s: &S| -> Step { s.borrow().clone() },
            |v: &V| {
                let mut vars = v
                    .borrow()
                    .variables
                    .keys()
                    .cloned()
                    .collect::<Vec<SymVarId>>();
                vars.sort();
                vars
            },
            |c: &C| -> &ConstValue { c.borrow() },
        );
        let filter_ref = Rc::new(RefCell::new(filter));
        let filter = filter_ref.clone();

        let dumper = persistence.map(|cfg| {
            create_ser_dumper!(cfg, "Coverage Depth".to_owned(), FILENAME_DEFAULT, || {
                filter_ref
                    .as_ref()
                    .borrow()
                    .get_last_depths()
                    .iter()
                    .collect::<Vec<_>>()
            })
        });
        (filter, dumper)
    }
}

mod coverage {
    use std::{cell::RefCell, rc::Rc};

    use serde::{Serialize, Serializer};

    use crate::abs::IntType;

    use super::*;

    pub(super) type Inspector = BranchCoverageStepInspector<Step, ConstValue>;

    pub(super) fn create_branch_coverage_collector<V: Display>(
        output_config: &Option<OutputConfig>,
    ) -> (RRef<Inspector>, Option<impl Dumper>)
    where
        Inspector: StepInspector<IStep, V, ConstValue>,
    {
        let inspector_ref = Rc::new(RefCell::new(BranchCoverageStepInspector::new()));
        let dumper = output_config
            .as_ref()
            .map(|cfg| create_serializer(cfg, inspector_ref.clone()));
        (inspector_ref.clone(), dumper)
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

    fn create_serializer(config: &OutputConfig, inspector: RRef<Inspector>) -> impl Dumper {
        let config = match config {
            OutputConfig::File(cfg) => cfg,
        };
        create_ser_dumper!(config, "Branch Coverage".to_owned(), "branch_cov", || {
            inspector
                .as_ref()
                .borrow()
                .get_coverage()
                .iter()
                .collect::<Vec<_>>()
        })
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

mod dumper {
    use crate::abs::ConstraintKind;

    use super::*;

    pub(super) fn create_decisions_dumper<S, V, C, SList, CList>(
        output_config: &OutputConfig,
        steps_view: RefView<SList>,
        constraints_view: RefView<CList>,
    ) -> impl Dumper
    where
        S: Borrow<Step> + HasIndex,
        C: Borrow<ConstValue>,
        SList: AsRef<[S]>,
        CList: AsRef<[Constraint<V, C>]>,
    {
        #[derive(dm::From, Serialize)]
        struct TraceItem<'a, 'b> {
            step: Indexed<&'a Step>,
            decision: ConstraintKind<&'b ConstValue>,
        }
        match output_config {
            OutputConfig::File(cfg) => {
                const FILENAME_DEFAULT: &str = "decisions";
                create_ser_dumper!(cfg, "Full Trace".to_owned(), FILENAME_DEFAULT, || {
                    core::iter::zip(
                        steps_view
                            .borrow()
                            .as_ref()
                            .iter()
                            .map(|s| (s.borrow(), s.index()).into()),
                        constraints_view
                            .borrow()
                            .as_ref()
                            .iter()
                            .map(|c| c.kind.as_ref().map(|c| c.borrow())),
                    )
                    .map(TraceItem::from)
                    .collect::<Vec<_>>()
                })
            }
        }
    }

    pub(super) fn create_solver_constraints_dumper<'ctx, S, V, C, SList, CList>(
        output_config: &OutputConfig,
        steps_view: RefView<SList>,
        constraints_view: RefView<CList>,
    ) -> impl Dumper
    where
        S: Borrow<Step> + HasIndex,
        V: Borrow<CurrentSolverValue<'ctx>>,
        C: Borrow<CurrentSolverCase<'ctx>>,
        SList: AsRef<[S]>,
        CList: AsRef<[Constraint<V, C>]>,
    {
        #[derive(dm::From, Serialize)]
        struct TraceItem<'a> {
            step: Indexed<&'a Step>,
            constraint: Constraint<String, String>,
        }
        match output_config {
            OutputConfig::File(cfg) => {
                const FILENAME_DEFAULT: &str = "sym_decisions";
                create_ser_dumper!(cfg, "Sym Trace".to_owned(), FILENAME_DEFAULT, || {
                    core::iter::zip(
                        steps_view
                            .borrow()
                            .as_ref()
                            .iter()
                            .map(|s| (s.borrow(), s.index()).into()),
                        constraints_view.borrow().as_ref().iter().map(|c| {
                            c.as_ref().map(
                                |v| v.borrow().value.to_smtlib2(),
                                |c| c.borrow().to_smtlib2(),
                            )
                        }),
                    )
                    .map(TraceItem::from)
                    .collect::<Vec<_>>()
                })
            }
        }
    }
}

mod dumping {
    use std::fs;

    use common::pri::{BasicBlockIndex, DefId};
    use serde::{Deserialize, de::DeserializeOwned};

    use crate::utils::file::{FileFormat, FileGenConfig};

    use super::*;

    pub(super) trait Dumper {
        fn dump(&mut self) -> Result<(), String>;
    }

    impl Dumper for Box<dyn Dumper + '_> {
        fn dump(&mut self) -> Result<(), String> {
            self.as_mut().dump()
        }
    }

    impl<F: FnMut() -> Result<(), String>> Dumper for F {
        fn dump(&mut self) -> Result<(), String> {
            self.call_mut(())
        }
    }

    impl<D: Dumper> Dumper for Vec<D> {
        fn dump(&mut self) -> Result<(), String> {
            for dumper in self.iter_mut() {
                dumper.dump()?
            }
            Ok(())
        }
    }

    pub(super) trait DumperListExt {
        fn extend_opt<'a>(&mut self, dumper: Option<impl Dumper + 'a>)
        where
            Self: Extend<Box<dyn Dumper + 'a>>;
    }
    impl<'b> DumperListExt for Vec<Box<dyn Dumper + 'b>> {
        fn extend_opt<'a>(&mut self, dumper: Option<impl Dumper + 'a>)
        where
            Self: Extend<Box<dyn Dumper + 'a>>,
        {
            Extend::extend(
                self,
                dumper
                    .into_iter()
                    .map(Box::new)
                    .map(|d| d as Box<dyn Dumper>),
            )
        }
    }

    macro_rules! create_ser_dumper {
        ($config: expr, $name: expr, $default_filename: expr, || {$data: expr}) => {{
            use std::io::{Seek, Write};

            use crate::utils::file::{FileFormat, FileGenConfig};

            let create_serializer = |config: &FileGenConfig,
                                     name: String,
                                     default_filename: &str| {
                let mut file = config
                    .open_or_create_single(&default_filename)
                    .unwrap_or_else(|e| panic!("Could not create file for {name}: {e}"));

                match config.format {
                    FileFormat::Json => {
                        // FIXME: Introduce a naive journaling to prevent corruption
                        let mut serializer = serde_json::Serializer::new(file.try_clone().unwrap());
                        Box::new(move || {
                            file.rewind().and_then(|_| file.set_len(0)).map_err(|e| {
                                format!("{}: Could not truncate file: {}", name, e.to_string())
                            })?;
                            $data
                                .serialize(&mut serializer)
                                .map(|_| ())
                                .map_err(|e| format!("{}: {}", name, e.to_string()))?;
                            file.flush().map_err(|e| {
                                format!("{}: Could not flush file: {}", name, e.to_string(),)
                            })?;
                            Ok(())
                        })
                    }
                    FileFormat::Binary => {
                        unimplemented!("Binary output format is not supported yet")
                    }
                }
            };

            create_serializer($config, $name, $default_filename)
        }};
    }
    pub(super) use create_ser_dumper;

    pub(super) fn deserialize_snapshot<T: DeserializeOwned + Default>(
        config: &FileGenConfig,
        default_filename: &str,
    ) -> Option<Result<T, String>> {
        use std::io::Seek;

        let file_path = config.single_file_path(&default_filename);
        if !file_path.exists() {
            return None;
        }

        let deserialize = || -> Result<T, String> {
            let mut file = fs::File::open(&file_path).map_err(|e| {
                format!(
                    "Problem in opening the file `{}`:{}",
                    file_path.display(),
                    e.to_string()
                )
            })?;

            if file.stream_len().is_ok_and(|l| l == 0) {
                return Ok(T::default());
            }

            let snapshot = match config.format {
                FileFormat::Json => serde_json::from_reader(file)
                    .map_err(|e| format!("Problem in parsing the file: {}", e.to_string())),
                FileFormat::Binary => {
                    unimplemented!("Binary output format is not supported yet")
                }
            };
            snapshot
        };

        Some(deserialize())
    }

    pub(super) fn dump(dumper: &RRef<impl Dumper>) {
        log_debug!("Dumped trace managing data");
        dumper
            .as_ref()
            .borrow_mut()
            .dump()
            .expect("Problem with dumping information")
    }

    pub(super) fn create_timer_dumper_inspector<'a, S: 'a, V: 'a, C: 'a>(
        dumper: RRef<impl Dumper + 'a>,
        interval: Option<Duration>,
    ) -> Box<dyn StepInspector<S, V, C> + 'a> {
        if let Some(interval) = interval {
            Box::new(create_dumper_inspector(dumper).timer_freq_filtered(interval))
        } else {
            Box::new(()) // NoopInspector
        }
    }

    fn create_dumper_inspector<'a, S, V, C>(
        dumper: RRef<impl Dumper + 'a>,
    ) -> impl StepInspector<S, V, C> {
        move |_: &S, _: Constraint<&V, &C>| dump(&dumper)
    }
}

mod helpers {
    use delegate::delegate;

    use super::*;

    #[derive(
        PartialEq,
        Eq,
        Hash,
        Clone,
        Copy,
        Debug,
        Default,
        dm::Deref,
        dm::From,
        dm::Into,
        dm::Display,
        Serialize,
        Deserialize,
    )]
    pub(crate) struct Step(BasicBlockLocation);

    #[derive(Clone, Debug, dm::Deref)]
    pub(super) struct Tagged<T> {
        #[deref]
        pub value: T,
        pub tags: Vec<Tag>,
    }

    impl<T> Borrow<T> for Tagged<T> {
        fn borrow(&self) -> &T {
            &self.value
        }
    }

    impl<T> HasTags for Tagged<T> {
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

    impl<T: HasIndex> HasIndex for Tagged<T> {
        fn index(&self) -> usize {
            self.value.index()
        }
    }

    pub(super) trait HasIndex {
        fn index(&self) -> usize;
    }

    #[derive(Clone, Copy, Debug, dm::Deref, dm::From, Serialize)]
    pub(super) struct Indexed<T> {
        #[deref]
        pub value: T,
        pub index: usize,
    }

    impl<T> HasIndex for Indexed<T> {
        fn index(&self) -> usize {
            self.index
        }
    }

    impl<T> Borrow<T> for Indexed<T> {
        fn borrow(&self) -> &T {
            &self.value
        }
    }

    impl<T: Display> Display for Indexed<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}: {}", self.index, self.value)
        }
    }

    impl Borrow<Step> for IStep {
        fn borrow(&self) -> &Step {
            &self.value.borrow()
        }
    }

    impl<T: HasTags> HasTags for Indexed<T> {
        delegate! {
            to self.value {
                fn tags(&self) -> &[Tag];
            }
        }
    }

    #[derive(Debug)]
    pub(super) struct Translation<V, T>(V, T);

    impl<V, T> Translation<V, T> {
        pub fn of(value: V, translator: impl FnOnce(&V) -> T) -> Self {
            let translated = translator(&value);
            Self(value, translated)
        }
    }

    impl<T> Borrow<ValueRef> for Translation<ValueRef, T> {
        fn borrow(&self) -> &ValueRef {
            &self.0
        }
    }

    impl<T> Borrow<ConstValue> for Translation<ConstValue, T> {
        fn borrow(&self) -> &ConstValue {
            &self.0
        }
    }

    impl<'ctx, V> Borrow<CurrentSolverValue<'ctx>> for Translation<V, CurrentSolverValue<'ctx>> {
        fn borrow(&self) -> &CurrentSolverValue<'ctx> {
            &self.1
        }
    }

    impl<'ctx, V> Borrow<CurrentSolverCase<'ctx>> for Translation<V, CurrentSolverCase<'ctx>> {
        fn borrow(&self) -> &CurrentSolverCase<'ctx> {
            &self.1
        }
    }
}
