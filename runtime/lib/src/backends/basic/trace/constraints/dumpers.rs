use core::borrow::Borrow;
use core::ops::Deref;

use crate::{
    abs::Constraint,
    trace::{StepInspector, StreamDumperStepInspector},
    utils::{HasIndex, Indexed, file::FileFormat},
};

use super::{CurrentSolverCase, CurrentSolverValue, OutputConfig, Step, backend};
use backend::Precondition;

pub(super) fn create_solver_constraints_dumper<'ctx, S, V, C>(
    config: &OutputConfig,
) -> impl StepInspector<S, V, C>
where
    S: Borrow<Step> + HasIndex,
    V: Borrow<CurrentSolverValue<'ctx>>,
    C: Borrow<CurrentSolverCase<'ctx>>,
{
    let mut dumper_inspector = match config {
        OutputConfig::File(cfg) => {
            assert!(
                cfg.format.is_streamable(),
                "Only streamable formats are expected for symbolic constraints dumping"
            );
            match cfg.format {
                FileFormat::JsonLines => {
                    const FILENAME_DEFAULT: &str = "sym_decisions";
                    let file = cfg
                        .open_or_create_single(FILENAME_DEFAULT, true)
                        .unwrap_or_else(|e| {
                            panic!("Could not create file for symbolic constraints dumping: {e}")
                        });
                    StreamDumperStepInspector::json_lines(file)
                }
                FileFormat::Binary | FileFormat::Json => unreachable!(),
            }
        }
    };

    let inspector = move |step: &S, constraint: Constraint<&V, &C>| {
        let step: Indexed<Step> = (step.borrow().clone(), step.index()).into();
        let constraint =
            constraint.map(|v| v.borrow().serializable(), |c| c.borrow().serializable());
        dumper_inspector.inspect(&step, constraint.as_ref());
    };
    inspector
}

pub(super) fn create_preconditions_dumper<'ctx, S, V, C>(
    config: &OutputConfig,
) -> impl StepInspector<S, V, C>
where
    S: Borrow<Step> + HasIndex,
    V: Borrow<Precondition>,
{
    impl serde::Serialize for Precondition {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            match self {
                Precondition::None | Precondition::Unknown => serializer.serialize_none(),
                Precondition::Constraints(constraints) => constraints.deref().serialize(serializer),
            }
        }
    }

    let mut serializer = match config {
        OutputConfig::File(cfg) => {
            assert!(
                cfg.format.is_streamable(),
                "Only streamable formats are expected for symbolic constraints dumping"
            );
            match cfg.format {
                FileFormat::JsonLines => {
                    const FILENAME_DEFAULT: &str = "discr_preconditions";
                    let file = cfg
                        .open_or_create_single(FILENAME_DEFAULT, true)
                        .unwrap_or_else(|e| {
                            panic!("Could not create file for symbolic constraints dumping: {e}")
                        });
                    serde_json::Serializer::with_formatter(
                        file,
                        crate::utils::file::JsonLinesFormatter::default(),
                    )
                }
                FileFormat::Binary | FileFormat::Json => unreachable!(),
            }
        }
    };

    let inspector = move |step: &S, constraint: Constraint<&V, &C>| {
        let step: Indexed<Step> = (step.borrow().clone(), step.index()).into();
        use serde::{Serializer, ser::SerializeStruct};
        serializer
            .serialize_struct("Record", 2)
            .and_then(|mut rec_ser| {
                rec_ser.serialize_field(stringify!(step), &step)?;
                rec_ser.serialize_field("preconditions", constraint.discr.borrow())?;
                rec_ser.end()
            })
            .unwrap_or_else(|e| panic!("Could not dump step: {e}"));
    };
    inspector
}
