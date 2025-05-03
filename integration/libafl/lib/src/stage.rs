use std::{borrow::Cow, marker::PhantomData, sync::mpsc, thread};

use libafl::{
    Error, Evaluator, ExecuteInputResult, HasMetadata, HasNamedMetadata,
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

#[derive(Debug, Clone)]
struct Stats {
    generated_name: Cow<'static, str>,
    generated: usize,
    in_corpus_name: Cow<'static, str>,
    in_corpus: usize,
    solution_name: Cow<'static, str>,
    solution: usize,
}

impl Stats {
    fn new(base_name: Cow<'static, str>) -> Self {
        Self {
            generated_name: base_name.clone() + "_generated",
            generated: 0,
            in_corpus_name: base_name.clone() + "_in_corpus",
            in_corpus: 0,
            solution_name: base_name.clone() + "_solution",
            solution: 0,
        }
    }

    fn notify_mutant(&mut self, exe_result: ExecuteInputResult) {
        self.generated += 1;

        if exe_result.is_corpus() {
            self.in_corpus += 1;
        }
        if exe_result.is_solution() {
            self.solution += 1;
        }
    }

    fn report<EM: EventFirer<I, S>, I, S>(&self, manager: &mut EM, state: &mut S) {
        let mut fire = |name: &Cow<'static, str>, value| {
            let _ = manager.fire(state, Event::UpdateUserStats {
                name: name.clone(),
                value: UserStats::new(UserStatsValue::Number(value as u64), AggregatorOps::Sum),
                phantom: Default::default(),
            });
        };

        fire(&self.generated_name, self.generated);
        fire(&self.in_corpus_name, self.in_corpus);
        fire(&self.solution_name, self.solution);
    }
}

/// A copy of `MultiMutationalStage` with stats added in it.
#[derive(Clone, Debug)]
pub struct MultiMutationalStageWithStats<E, EM, I, M, S, Z> {
    name: Cow<'static, str>,
    stats: Stats,
    mutator: M,
    phantom: PhantomData<(E, EM, I, S, Z)>,
}

impl<E, EM, I, M, S, Z> MultiMutationalStageWithStats<E, EM, I, M, S, Z> {
    /// Creates a new [`MultiMutationalStageWithStats`]
    pub fn new(name: Cow<'static, str>, stats_name: Cow<'static, str>, mutator: M) -> Self {
        Self {
            name,
            stats: Stats::new(stats_name),
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

        for new_input in generated {
            let (untransformed, post) = new_input.try_transform_into(state)?;
            let (exe_result, corpus_id) =
                fuzzer.evaluate_filtered(state, executor, manager, &untransformed)?;
            self.mutator.multi_post_exec(state, corpus_id)?;
            post.post_exec(state, corpus_id)?;
            self.stats.notify_mutant(exe_result);
        }

        self.stats.report(manager, state);

        Ok(())
    }
}

/// Performs mutations concurrently in a separate thread and offers the ones
/// ready to be evaluated to the fuzzer.
#[derive(Debug)]
pub struct NonBlockingMultiMutationalStage<E, EM, I, M, S, Z> {
    name: Cow<'static, str>,
    stats: Stats,
    inputs: mpsc::SyncSender<I>,
    mutants: mpsc::Receiver<I>,
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
            stats: Stats::new(stats_name),
            inputs: inputs_sender,
            mutants: mutants_receiver,
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
                let (untransformed, post) = new_input.try_transform_into(state)?;
                let (exe_result, corpus_id) =
                    fuzzer.evaluate_filtered(state, executor, manager, &untransformed)?;
                // FIXME: Call MultiMutator::multi_post_exec(state, corpus_id)?;
                post.post_exec(state, corpus_id)?;

                self.stats.notify_mutant(exe_result);
            }

            self.stats.report(manager, state);
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
