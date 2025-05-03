use std::{borrow::Cow, marker::PhantomData, sync::mpsc, thread};

use libafl::{
    Error, Evaluator, HasMetadata, HasNamedMetadata,
    corpus::HasCurrentCorpusId,
    events::{Event, EventFirer},
    monitors::stats::{AggregatorOps, UserStats, UserStatsValue},
    mutators::MultiMutator,
    stages::{
        Restartable, RetryCountRestartHelper, Stage,
        mutational::{MutatedTransform, MutatedTransformPost},
    },
    state::{HasCurrentTestcase, HasRand, NopState},
};
use libafl_bolts::Named;

/// A copy of `MultiMutationalStage` with stats added in it.
#[derive(Clone, Debug)]
pub struct MultiMutationalStageWithStats<E, EM, I, M, S, Z> {
    name: Cow<'static, str>,
    stats_name: Cow<'static, str>,
    total_generated: usize,
    mutator: M,
    phantom: PhantomData<(E, EM, I, S, Z)>,
}

impl<E, EM, I, M, S, Z> MultiMutationalStageWithStats<E, EM, I, M, S, Z> {
    /// Creates a new [`MultiMutationalStageWithStats`]
    pub fn new(name: Cow<'static, str>, stats_name: Cow<'static, str>, mutator: M) -> Self {
        Self {
            name,
            stats_name,
            total_generated: 0,
            mutator,
            phantom: PhantomData,
        }
    }
}

impl<E, EM, I, M, S, Z> Named for MultiMutationalStageWithStats<E, EM, I, M, S, Z> {
    fn name(&self) -> &Cow<'static, str> {
        &self.name
    }
}

impl<E, EM, I, M, S, Z> Stage<E, EM, S, Z> for MultiMutationalStageWithStats<E, EM, I, M, S, Z>
where
    I: Clone + MutatedTransform<I, S>,
    M: MultiMutator<I, S>,
    EM: EventFirer<I, S>,
    S: HasRand + HasNamedMetadata + HasCurrentTestcase<I> + HasCurrentCorpusId,
    Z: Evaluator<E, EM, I, S>,
{
    #[inline]
    fn perform(
        &mut self,
        fuzzer: &mut Z,
        executor: &mut E,
        state: &mut S,
        manager: &mut EM,
    ) -> Result<(), Error> {
        let mut testcase = state.current_testcase_mut()?;
        let Ok(input) = I::try_transform_from(&mut testcase, state) else {
            return Ok(());
        };
        drop(testcase);

        let generated = self.mutator.multi_mutate(state, &input, None)?;

        self.total_generated += generated.len();
        report_new_inputs(
            manager,
            state,
            self.stats_name.clone(),
            self.total_generated,
        );

        for new_input in generated {
            let (untransformed, post) = new_input.try_transform_into(state)?;
            let (_, corpus_id) =
                fuzzer.evaluate_filtered(state, executor, manager, &untransformed)?;
            self.mutator.multi_post_exec(state, corpus_id)?;
            post.post_exec(state, corpus_id)?;
        }

        Ok(())
    }
}

/// Performs mutations concurrently in a separate thread and offers the ones
/// ready to be evaluated to the fuzzer.
#[derive(Debug)]
pub struct NonBlockingMultiMutationalStage<E, EM, I, M, S, Z> {
    name: Cow<'static, str>,
    stats_name: Cow<'static, str>,
    inputs: mpsc::SyncSender<I>,
    mutants: mpsc::Receiver<I>,
    total_generated: usize,
    _phantom: PhantomData<(E, EM, I, M, S, Z)>,
}

impl<E, EM, I, M, S, Z> NonBlockingMultiMutationalStage<E, EM, I, M, S, Z>
where
    M: MultiMutator<I, NopState<I>> + Send + 'static,
    I: Send + 'static,
{
    pub fn new(name: Cow<'static, str>, stats_name: Cow<'static, str>, mutator: M) -> Self {
        let (inputs_sender, mutants_receiver) = Self::spawn_mutator_thread(mutator);
        Self {
            name,
            stats_name,
            inputs: inputs_sender,
            mutants: mutants_receiver,
            total_generated: 0,
            _phantom: PhantomData,
        }
    }

    fn spawn_mutator_thread(mut mutator: M) -> (mpsc::SyncSender<I>, mpsc::Receiver<I>) {
        let (inputs_sender, inputs_receiver) = mpsc::sync_channel(0);
        let (mutants_sender, mutants_receiver) = mpsc::channel();

        thread::spawn(move || {
            let mut state = NopState::new();
            loop {
                let Ok(input) = inputs_receiver.recv() else {
                    break;
                };
                let mutants = mutator.multi_mutate(&mut state, &input, None).unwrap();
                for mutant in mutants.into_iter() {
                    mutants_sender.send(mutant).unwrap();
                }
            }
        });
        (inputs_sender, mutants_receiver)
    }
}

impl<E, EM, I, M, S, Z> Named for NonBlockingMultiMutationalStage<E, EM, I, M, S, Z> {
    fn name(&self) -> &Cow<'static, str> {
        &self.name
    }
}

// NOTE: Based on `MultiMutationalStage`
impl<E, EM, I, M, S, Z> Stage<E, EM, S, Z> for NonBlockingMultiMutationalStage<E, EM, I, M, S, Z>
where
    I: Clone + MutatedTransform<I, S>,
    M: MultiMutator<I, S>,
    S: HasRand + HasNamedMetadata + HasCurrentTestcase<I> + HasCurrentCorpusId,
    EM: EventFirer<I, S>,
    Z: Evaluator<E, EM, I, S>,
{
    #[inline]
    fn perform(
        &mut self,
        fuzzer: &mut Z,
        executor: &mut E,
        state: &mut S,
        manager: &mut EM,
    ) -> Result<(), Error> {
        // Evaluate the generated mutants.
        {
            // NOTE: We do it first to make the mutator less susceptible to concurrency issues.
            for new_input in self.mutants.try_iter() {
                self.total_generated += 1;
                report_new_inputs(
                    manager,
                    state,
                    self.stats_name.clone(),
                    self.total_generated,
                );

                let (untransformed, post) = new_input.try_transform_into(state)?;
                let (_, corpus_id) =
                    fuzzer.evaluate_filtered(state, executor, manager, &untransformed)?;
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

impl<E, EM, I, M, S, Z> Restartable<S> for NonBlockingMultiMutationalStage<E, EM, I, M, S, Z>
where
    S: HasMetadata + HasNamedMetadata + HasCurrentCorpusId,
{
    #[inline]
    fn should_restart(&mut self, state: &mut S) -> Result<bool, Error> {
        // Make sure we don't get stuck crashing on a single testcase
        RetryCountRestartHelper::should_restart(state, &self.name, 3)
    }

    #[inline]
    fn clear_progress(&mut self, state: &mut S) -> Result<(), Error> {
        RetryCountRestartHelper::clear_progress(state, &self.name)
    }
}

impl<E, EM, I, M, S, Z> Restartable<S> for MultiMutationalStageWithStats<E, EM, I, M, S, Z>
where
    S: HasMetadata + HasNamedMetadata + HasCurrentCorpusId,
{
    #[inline]
    fn should_restart(&mut self, state: &mut S) -> Result<bool, Error> {
        // Make sure we don't get stuck crashing on a single testcase
        RetryCountRestartHelper::should_restart(state, &self.name, 3)
    }

    #[inline]
    fn clear_progress(&mut self, state: &mut S) -> Result<(), Error> {
        RetryCountRestartHelper::clear_progress(state, &self.name)
    }
}

fn report_new_inputs<EM, I, S>(
    manager: &mut EM,
    state: &mut S,
    stats_name: Cow<'static, str>,
    count: usize,
) where
    EM: EventFirer<I, S>,
{
    let _ = manager.fire(state, Event::UpdateUserStats {
        name: stats_name.clone(),
        value: UserStats::new(UserStatsValue::Number(count as u64), AggregatorOps::Sum),
        phantom: Default::default(),
    });
}
