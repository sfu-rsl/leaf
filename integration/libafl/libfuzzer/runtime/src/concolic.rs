use std::path::PathBuf;

use crate::options::LibfuzzerOptions;

use libafl::{
    Evaluator, HasMetadata, HasNamedMetadata,
    corpus::HasCurrentCorpusId,
    events::SendExiting,
    inputs::{HasMutatorBytes, Input},
    stages::{OptionalStage, Restartable, Stage},
    state::{HasCorpus, HasNestedStage, HasRand, Stoppable},
};
use libafl_bolts::tuples::tuple_list;
use libafl_leaf::{DivergingMutator, NonBlockingMultiMutationalStage};

const NAME_DEFAULT_ORCHESTRATOR: &str = "leafo_onetime";

pub(super) fn make_concolic_stage<I, E, EM, S, Z>(
    options: &LibfuzzerOptions,
) -> impl Stage<E, EM, S, Z> + Restartable<S>
where
    I: Input + HasMutatorBytes + Send + Clone + 'static,
    EM: SendExiting,
    S: HasCorpus<I>
        + HasCurrentCorpusId
        + HasRand
        + HasNamedMetadata
        + HasMetadata
        + HasNestedStage
        + Stoppable,
    Z: Evaluator<E, EM, I, S>,
{
    let orchestrator_path = options
        .leaf_orch()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(NAME_DEFAULT_ORCHESTRATOR));
    let workdir = std::env::temp_dir()
        .join("leaf")
        .join("fuzz")
        .join(options.fuzzer_name())
        .join("work");
    std::fs::create_dir_all(&workdir).expect("Failed to create the work directory for mutator");
    let opt_stage = options
        .conc_program()
        .map(|p| {
            DivergingMutator::new(
                &orchestrator_path,
                options.leaf_orch_args().into(),
                p,
                options.conc_program_args().into(),
                &workdir,
            )
        })
        .map(|m| NonBlockingMultiMutationalStage::new(std::borrow::Cow::Borrowed("Concolic"), m))
        .map(|s| tuple_list!(s));
    OptionalStage::new(opt_stage)
}
