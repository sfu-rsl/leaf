use std::{cell::RefCell, fmt::Display, rc::Rc};

use serde::{Serialize, Serializer};

use crate::{abs::IntType, trace::StepInspector, utils::alias::RRef};

use super::{
    IStep, Step, backend,
    utils::dumping::{Dumper, create_ser_dumper},
};
use backend::{ConstValue, config::OutputConfig};

type Inspector = crate::trace::BranchCoverageStepInspector<Step, ConstValue>;

pub(super) fn create_branch_coverage_collector<V: Display>(
    output_config: &Option<OutputConfig>,
) -> (RRef<Inspector>, Option<impl Dumper>)
where
    Inspector: StepInspector<IStep, V, ConstValue>,
{
    let inspector_ref = Rc::new(RefCell::new(Inspector::new()));
    let dumper = output_config
        .as_ref()
        .map(|cfg| create_dumper(cfg, inspector_ref.clone()));
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

fn create_dumper(config: &OutputConfig, inspector: RRef<Inspector>) -> impl Dumper {
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
