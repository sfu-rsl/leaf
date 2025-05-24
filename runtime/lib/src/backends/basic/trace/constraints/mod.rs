mod branch_cov;
mod divergence;
mod dumpers;
mod sanity_check;
mod utils;

use std::cell::RefCell;
use std::rc::Rc;

use crate::{
    abs::{Tag, backend::Solver},
    backends::basic::config::SolverImpl,
    solvers::z3::Z3Solver,
    trace::{
        AdapterTraceManagerExt, AggregatorStepInspector, AggregatorTraceManager,
        FilterTraceManagerExt, InspectionTraceManagerExt, LoggerTraceManagerExt, StepInspector,
    },
    utils::{Indexed, RefView, Tagged, alias::RRef},
};

use super::backend;
use backend::{
    ConstValue, Implied, SymVarId, SymVariablesManager, ValueRef,
    config::{ExecutionTraceConfig, OutputConfig, TraceInspectorType},
    expr::translators::z3::Z3ValueTranslator,
};

use super::{Step, StepCounter};

use utils::{
    ShutdownTraceManagerExt, Translation,
    dumping::{Dumper, DumperListExt, dump},
};

type CurrentSolver<'ctx> = Z3Solver<'ctx, SymVarId>;
type CurrentSolverValue<'ctx> = <CurrentSolver<'ctx> as Solver>::Value;
type CurrentSolverCase<'ctx> = <CurrentSolver<'ctx> as Solver>::Case;
type CurrentSolverTranslator<'ctx> = Z3ValueTranslator<'ctx>;

// These are the types of steps, values, and cases for the inner managers.
// You can use them to give explicit types as opposed to generics.
type IStep = Tagged<Indexed<Step>>;
type IValue<'ctx> = Translation<ValueRef, CurrentSolverValue<'ctx>>;
type ICase<'ctx> = Translation<ConstValue, CurrentSolverCase<'ctx>>;

pub(crate) fn create_trace_components(
    step_counter: StepCounter,
    tags: RRef<Vec<Tag>>,
    sym_var_manager: RRef<impl SymVariablesManager + 'static>,
    trace_config: &ExecutionTraceConfig,
    output_config: &Vec<OutputConfig>,
    solver_config: &SolverImpl,
) -> (
    impl backend::TraceManager,
    RefView<Vec<Indexed<Step>>>,
    RefView<Vec<backend::Constraint>>,
) {
    let (solver, translator) = match solver_config {
        SolverImpl::Z3 { config } => {
            crate::solvers::z3::set_global_params(
                config.global_params.iter().map(|(k, v)| (k, v.to_string())),
            );
            let solver: CurrentSolver = Z3Solver::<SymVarId>::new_in_global_context();
            let translator = Z3ValueTranslator::new(solver.context());
            (solver, translator)
        }
    };
    let sym_var_manager_ref = sym_var_manager;

    let mut dumpers: Vec<Box<dyn Dumper>> = vec![];

    let mut cov_inspector = None;
    let sym_discr_inspectors = trace_config
        .inspectors
        .iter()
        .filter(|t| !is_inner_inspector(t))
        .map(|t| match t {
            TraceInspectorType::BranchCoverage { output, .. } => {
                let (inspector, dumper) =
                    branch_cov::create_branch_coverage_collector::<ValueRef>(output);
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

    let inner_step_inspectors = trace_config
        .constraints_dump
        .as_ref()
        .map(|cfg| dumpers::create_solver_constraints_dumper(cfg))
        .map(|inspector| Box::new(inspector) as Box<dyn StepInspector<_, _, _>>)
        .into_iter()
        .collect::<Vec<_>>();

    let core_manager = agg_manager.inspected_by(inner_step_inspectors).adapt(
        |s| s,
        move |v: ValueRef| Translation::of(v, &mut value_translator),
        move |c: ConstValue| Translation::of(c, &mut case_translator),
    );
    let manager = core_manager;

    let all_constraints_inspectors = trace_config
        .preconditions_dump
        .as_ref()
        .map(|cfg| dumpers::create_preconditions_dumper(cfg))
        .map(|inspector| Box::new(inspector) as Box<dyn StepInspector<_, _, _>>)
        .into_iter()
        .collect::<Vec<_>>();

    let outer_agg_inspector = AggregatorStepInspector::default();

    let steps_view = outer_agg_inspector.steps();
    let constraints_view = outer_agg_inspector.constraints();

    let dumpers_ref = Rc::new(RefCell::new(dumpers));

    let manager = manager
        .logged()
        .adapt_step(move |s| Tagged {
            value: s,
            tags: RefCell::borrow(&tags).clone(),
        })
        .inspected_by(sym_discr_inspectors)
        .filtered_by(|_, c| c.discr.is_symbolic())
        .adapt_value(|discr: Implied<ValueRef>| discr.value)
        .inspected_by(outer_agg_inspector)
        .inspected_by(all_constraints_inspectors)
        .adapt_step(move |s: Step| Indexed {
            index: *step_counter.as_ref().borrow(),
            value: s,
        })
        .inspected_by(utils::dumping::create_timer_dumper_inspector(
            dumpers_ref.clone(),
            trace_config
                .dump_interval
                .map(|i| core::time::Duration::from_secs(i.into())),
        ))
        .on_shutdown(move || dump(&dumpers_ref));

    (manager, steps_view, constraints_view)
}

fn is_inner_inspector(t: &TraceInspectorType) -> bool {
    use TraceInspectorType::*;
    match t {
        SanityChecker { .. } | DivergingInput { .. } => true,
        BranchCoverage { .. } => false,
    }
}
