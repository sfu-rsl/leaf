use std::path::PathBuf;

use crate::options::LibfuzzerOptions;

use libafl::{
    Evaluator, HasNamedMetadata,
    corpus::Corpus,
    events::EventProcessor,
    inputs::{HasMutatorBytes, Input, UsesInput},
    stages::{HasNestedStageStatus, OptionalStage, Stage},
    state::{HasCorpus, HasRand, State, UsesState},
};
use libafl_bolts::tuples::tuple_list;
use libafl_leaf::{DivergingMutator, NonBlockingMultiMutationalStage};

const NAME_ORCHESTRATOR: &str = "leafo_onetime";

pub(super) fn make_concolic_stage<I, E, EM, S, Z>(
    options: &LibfuzzerOptions,
) -> impl Stage<E, EM, Z, State = S>
where
    I: Input + HasMutatorBytes + Send + Clone + 'static,
    S: State<Input = I> + HasCorpus + HasRand + HasNamedMetadata + HasNestedStageStatus,
    <S as HasCorpus>::Corpus: Corpus<Input = I>,
    E: UsesState<State = S>,
    EM: EventProcessor<E, Z> + UsesState<State = S>,
    Z: Evaluator<E, EM> + UsesState<State = S>,
{
    let orchestrator_path = options
        .leaf_orch()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(NAME_ORCHESTRATOR));
    let workdir = std::env::temp_dir()
        .join("leaf")
        .join("fuzz")
        .join(options.fuzzer_name())
        .join("work");
    std::fs::create_dir_all(&workdir).expect("Failed to create the work directory for mutator");
    let opt_stage = options
        .conc_program()
        .map(|p| DivergingMutator::new(&orchestrator_path, p, &workdir))
        .map(|m| NonBlockingMultiMutationalStage::new(std::borrow::Cow::Borrowed("Concolic"), m))
        .map(|s| tuple_list!(s));
    OptionalStage::new(opt_stage)
}
