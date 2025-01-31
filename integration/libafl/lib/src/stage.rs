use std::{borrow::Cow, marker::PhantomData, sync::mpsc, thread};

use libafl::{
    Error, Evaluator, HasNamedMetadata,
    corpus::Corpus,
    mutators::MultiMutator,
    stages::{
        RetryCountRestartHelper, Stage,
        mutational::{MutatedTransform, MutatedTransformPost},
    },
    state::{HasCorpus, HasCurrentTestcase, HasRand, UsesState},
};
use libafl_bolts::Named;

/// Performs mutations concurrently in a separate thread and offers the ones
/// ready to be evaluated to the fuzzer.
#[derive(Debug)]
pub struct NonBlockingMultiMutationalStage<E, EM, I, M, Z> {
    name: Cow<'static, str>,
    inputs: mpsc::SyncSender<I>,
    mutants: mpsc::Receiver<I>,
    #[allow(clippy::type_complexity)]
    _phantom: PhantomData<(E, EM, I, M, Z)>,
}

impl<E, EM, M, Z> NonBlockingMultiMutationalStage<E, EM, Z::Input, M, Z>
where
    Z: UsesState,
    M: MultiMutator<Z::Input, ()> + Send + 'static,
    Z::Input: Send + 'static,
{
    pub fn new(name: Cow<'static, str>, mutator: M) -> Self {
        let (inputs_sender, mutants_receiver) = Self::spawn_mutator_thread(mutator);
        Self {
            name,
            inputs: inputs_sender,
            mutants: mutants_receiver,
            _phantom: PhantomData,
        }
    }

    fn spawn_mutator_thread(
        mut mutator: M,
    ) -> (mpsc::SyncSender<Z::Input>, mpsc::Receiver<Z::Input>)
    where
        M: MultiMutator<Z::Input, ()> + Send + 'static,
    {
        let (inputs_sender, inputs_receiver) = mpsc::sync_channel(0);
        let (mutants_sender, mutants_receiver) = mpsc::channel();

        thread::spawn(move || {
            loop {
                let Ok(input) = inputs_receiver.recv() else {
                    break;
                };
                let mutants = mutator.multi_mutate(&mut (), &input, None).unwrap();
                for mutant in mutants.into_iter() {
                    mutants_sender.send(mutant).unwrap();
                }
            }
        });
        (inputs_sender, mutants_receiver)
    }
}

impl<E, EM, I, M, Z> UsesState for NonBlockingMultiMutationalStage<E, EM, I, M, Z>
where
    Z: UsesState,
{
    type State = Z::State;
}

impl<E, EM, I, M, Z> Named for NonBlockingMultiMutationalStage<E, EM, I, M, Z> {
    fn name(&self) -> &Cow<'static, str> {
        &self.name
    }
}

impl<E, EM, I, M, Z> Stage<E, EM, Z> for NonBlockingMultiMutationalStage<E, EM, I, M, Z>
where
    E: UsesState<State = Self::State>,
    EM: UsesState<State = Self::State>,
    M: MultiMutator<I, Self::State>,
    Z: Evaluator<E, EM>,
    Z::State: HasCorpus + HasRand + HasNamedMetadata + HasCurrentTestcase,
    I: MutatedTransform<Self::Input, Self::State> + Clone,
    <<Self as UsesState>::State as HasCorpus>::Corpus: Corpus<Input = Self::Input>, //delete me
{
    #[inline]
    fn should_restart(&mut self, state: &mut Self::State) -> Result<bool, Error> {
        // Make sure we don't get stuck crashing on a single testcase
        RetryCountRestartHelper::should_restart(state, self.name(), 3)
    }

    #[inline]
    fn clear_progress(&mut self, state: &mut Self::State) -> Result<(), Error> {
        RetryCountRestartHelper::clear_progress(state, self.name())
    }

    #[inline]
    #[allow(clippy::let_and_return)]
    #[allow(clippy::cast_possible_wrap)]
    fn perform(
        &mut self,
        fuzzer: &mut Z,
        executor: &mut E,
        state: &mut Self::State,
        manager: &mut EM,
    ) -> Result<(), Error> {
        // Evaluate the generated mutants.
        {
            // NOTE: We do it first to make the mutator less susceptible to concurrency issues.
            for new_input in self.mutants.try_iter() {
                let (untransformed, post) = new_input.try_transform_into(state)?;
                let (_, corpus_id) =
                    fuzzer.evaluate_input(state, executor, manager, untransformed)?;
                // FIXME: Call MultiMutator::multi_post_exec(state, corpus_id)?;
                post.post_exec(state, corpus_id)?;
            }
        }

        // Send the current input to the mutator.
        {
            let mut testcase = state.current_testcase_mut()?;
            let Ok(input) = I::try_transform_from(&mut testcase, state) else {
                return Ok(());
            };
            drop(testcase);
            // NOTE: We can also use an atomic storage to keep the input.
            self.inputs.try_send(input).or_else(|e| match e {
                mpsc::TrySendError::Full(_) => Ok(()),
                mpsc::TrySendError::Disconnected(_) => {
                    Err(Error::unknown("Disconnected mutator thread"))
                }
            })?;
        }

        Ok(())
    }
}
