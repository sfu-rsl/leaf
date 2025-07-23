use core::{borrow::Borrow, fmt::Debug};
use std::{cell::RefCell, collections::HashMap, fmt::Display};

use common::log_info;

use crate::{
    abs::Constraint,
    backends::basic::config::OutputConfig,
    trace::{
        TraceInspector,
        sanity_check::{FullTraceSanityChecker, StepSanityChecker},
    },
    utils::alias::RRef,
};

use super::{
    CurrentSolver, CurrentSolverCase, CurrentSolverTranslator, CurrentSolverValue,
    backend::{SymVarId, SymVariablesManager, config::ConstraintSanityCheckLevel},
};

const TAG: &str = crate::trace::sanity_check::TAG;

pub(super) fn create_trace_inspector<'ctx, S: 'ctx, V: 'ctx, C: 'ctx>(
    sym_var_manager: RRef<impl SymVariablesManager + 'ctx>,
    translator: CurrentSolverTranslator<'ctx>,
    level: ConstraintSanityCheckLevel,
    output: Option<&OutputConfig>,
    solver: CurrentSolver<'ctx>,
) -> Box<dyn TraceInspector<S, V, C> + 'ctx>
where
    V: Borrow<CurrentSolverValue<'ctx>>,
    C: Borrow<CurrentSolverCase<'ctx>>,
    S: Debug + Display,
    V: Debug + Display,
    C: Debug + Display,
{
    log_info!(target: TAG, "Trace satisfiability sanity checking will be performed for this run");

    let assumptions = ConcretizationConstraintsCache::new(sym_var_manager, translator);
    let dumper = dumping::trace_dumper(output);
    match level {
        ConstraintSanityCheckLevel::Warn => Box::new(FullTraceSanityChecker::new::<false>(
            solver,
            assumptions,
            Some(dumper),
        )),
        ConstraintSanityCheckLevel::Panic => Box::new(FullTraceSanityChecker::new::<true>(
            solver,
            assumptions,
            Some(dumper),
        )),
    }
}

pub(super) fn create_step_filter<'o, 'ctx, S: 'ctx, V: 'ctx, C: 'ctx>(
    sym_var_manager: RRef<impl SymVariablesManager + 'ctx>,
    translator: CurrentSolverTranslator<'ctx>,
    output: Option<&'o OutputConfig>,
    solver: CurrentSolver<'ctx>,
) -> impl FnMut(&S, Constraint<&V, &C>) -> bool + 'ctx
where
    V: Borrow<CurrentSolverValue<'ctx>>,
    C: Borrow<CurrentSolverCase<'ctx>>,
    S: Debug + Display,
    V: Debug + Display,
    C: Debug + Display,
{
    log_info!(target: TAG, "Constraint satisfiability sanity checking will be performed for this run");

    let assumptions = ConcretizationConstraintsCache::new(sym_var_manager, translator);

    StepSanityChecker::new(
        solver,
        assumptions,
        Some(dumping::constraint_dumper(output)),
    )
    .into_filter()
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

mod dumping {
    use core::fmt::{Debug, Display};
    use std::{
        fs::File,
        io::Write,
        path::{Path, PathBuf},
    };

    use itertools::Either;

    use common::{log_info, log_warn};

    use crate::utils::file::FileGenConfig;

    use super::{Constraint, OutputConfig, TAG};

    const MAX_CONSTRAINT_LENGTH: usize = 100;

    pub(super) fn trace_dumper<'ctx, S: 'ctx, V: 'ctx, C: 'ctx>(
        output: Option<&OutputConfig>,
    ) -> impl FnOnce(&[S], &[Constraint<V, C>])
    where
        S: Debug + Display,
        V: Debug + Display,
        C: Debug + Display,
    {
        let output = output.map(|cfg| match cfg {
            OutputConfig::File(cfg) => cfg.clone(),
        });

        move |steps, constraints| {
            log_info!(
                target: TAG,
                "Unsatisfiable trace with {} steps and {} constraints, with last step: {}{}",
                steps.len(),
                constraints.len(),
                steps.last().unwrap(),
                if output.is_some() {
                    format!(", full trace will be dumped to file")
                } else {
                    ", to see the full unsatisfiable trace, enable the output".to_owned()
                },
            );

            let Some(cfg) = output else {
                return;
            };

            let Ok((path, mut file)) = create_file(&cfg, "unsat_trace_") else {
                return;
            };

            writeln!(
                file,
                "{:#?}",
                steps
                    .into_iter()
                    .zip(constraints.into_iter())
                    .map(|(step, constraint)| {
                        UnsatConstraint {
                            step,
                            constraint: constraint.as_ref(),
                        }
                    })
            )
            .unwrap_or_else(|e| log_problem_in_writing(e, &path));
        }
    }

    pub(super) fn constraint_dumper<'ctx, S: 'ctx, V: 'ctx, C: 'ctx>(
        output: Option<&OutputConfig>,
    ) -> impl FnMut(&S, Constraint<&V, &C>)
    where
        S: Debug + Display,
        V: Debug + Display,
        C: Debug + Display,
    {
        let output = output.map(|cfg| match cfg {
            OutputConfig::File(cfg) => cfg.clone(),
        });

        let mut output: Option<Either<_, (PathBuf, File)>> = output.map(|cfg| Either::Left(cfg));

        move |step, constraint| {
            log_info!(target: TAG,
                "Unsatisfiable constraint @ {}, {:.*}{}",
                step,
                MAX_CONSTRAINT_LENGTH,
                constraint,
                if output.is_some() {
                    format!(", full constraint will be dumped to file")
                } else {
                    ", to see the unsatisfiable constraint, enable the output".to_owned()
                }
            );

            let Some(output) = &mut output else {
                return;
            };

            match output {
                Either::Left(cfg) => {
                    let Ok(file) = create_file(cfg, "unsat_constraints_") else {
                        return;
                    };
                    *output = Either::Right(file);
                }
                _ => {}
            }
            let (path, file) = output.as_mut().unwrap_right();

            writeln!(file, "{:#?}", UnsatConstraint { step, constraint })
                .unwrap_or_else(|e| log_problem_in_writing(e, path));
        }
    }

    fn create_file(cfg: &FileGenConfig, default_prefix: &str) -> std::io::Result<(PathBuf, File)> {
        cfg.open_or_create_single_with_path(
            default_prefix,
            common::utils::current_instant_millis().to_string().into(),
            true,
        )
        .inspect_err(|e| log_warn!("Could not create file for unsat constraints dumping: {e}"))
    }

    fn log_problem_in_writing(err: std::io::Error, path: &Path) {
        log_warn!(
            target: TAG,
            "Problem in writing to the file `{}`: {}",
            path.display(),
            err.to_string()
        );
    }

    // Helps with structured logging of unsatisfiable constraints.
    #[allow(unused)]
    #[derive(Debug)]
    struct UnsatConstraint<S, V, C> {
        step: S,
        constraint: Constraint<V, C>,
    }
}
