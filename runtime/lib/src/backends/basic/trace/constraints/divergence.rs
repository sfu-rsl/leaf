use core::borrow::Borrow;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

use common::{log_debug, log_info, types::trace::Constraint};

use crate::{
    abs::{HasTags, backend::Model},
    solvers::MapSolverExt,
    trace::{
        TraceInspector,
        divergence::{
            BranchCoverageDepthDivergenceFilter, DepthProvider, DivergenceFilter,
            ImmediateDivergingAnswerFinder, filter::all,
        },
    },
    utils::{alias::RRef, file::FileGenConfig},
};

use super::{
    CurrentSolver, CurrentSolverCase, CurrentSolverValue, Dumper, IStep, OutputConfig, Step,
    backend,
    utils::dumping::{DumperListExt, create_ser_dumper, deserialize_snapshot},
};
use backend::{
    ConstValue, SymVarId, SymVariablesManager, ValueRef, config::DivergenceFilterType,
    outgen::BasicOutputGenerator,
};

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

const FILENAME_SNAPSHOT_DEFAULT: &str = "branch_cov_depth";

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
    let persistence = persistence.map(|cfg| match cfg {
        OutputConfig::File(cfg) => cfg,
    });

    let snapshot = persistence.and_then(load_snapshot);

    let filter = BranchCoverageDepthDivergenceFilter::new(
        snapshot,
        branch_depth_provider,
        distance_threshold_factor,
        |s: &S| -> Step { s.borrow().clone() },
        |v: &V| {
            let mut vars = v
                .borrow()
                .variables
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<SymVarId>>();
            vars.sort();
            vars
        },
        |c: &C| -> &ConstValue { c.borrow() },
    );
    let filter_ref = Rc::new(RefCell::new(filter));
    let filter = filter_ref.clone();

    let dumper = persistence.map(|cfg| {
        create_ser_dumper!(
            cfg,
            "Coverage Depth".to_owned(),
            FILENAME_SNAPSHOT_DEFAULT,
            || {
                filter_ref
                    .as_ref()
                    .borrow()
                    .get_last_depths()
                    .iter()
                    .collect::<Vec<_>>()
            }
        )
    });
    (filter, dumper)
}

fn load_snapshot(cfg: &FileGenConfig) -> Option<HashMap<(Step, Vec<u32>), usize>> {
    deserialize_snapshot::<Vec<((Step, Vec<SymVarId>), usize)>>(cfg, FILENAME_SNAPSHOT_DEFAULT)
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
}
