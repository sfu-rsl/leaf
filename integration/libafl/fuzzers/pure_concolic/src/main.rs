use core::num::NonZero;
use std::{
    borrow::Cow,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::Duration,
};

use clap::Parser;

use libafl::{
    corpus::ondisk::OnDiskMetadataFormat, executors::command::CommandConfigurator, prelude::*,
};
use libafl_bolts::{AsSlice, Named, current_nanos, nonzero, rands::StdRand, tuples::tuple_list};
use libafl_leaf::{DivergingInputTestcaseScore, DivergingMutator, MultiMutationalStageWithStats};

use ::common::log_debug;
use serde::{Deserialize, Serialize};

const NAME_ORCHESTRATOR: &str = "leafo_onetime";

const DIR_MUTATOR_WORK: &str = "mutator";

mod utils;

use self::mut_chain::*;

#[derive(Parser, Debug)]
struct Args {
    /// Leaf-instrumented program to perform concolic execution
    #[arg(short, long)]
    conc_program: PathBuf,
    /// Original program to test with the generated inputs, defaults to the conc_program
    #[arg(short, long)]
    program: Option<PathBuf>,
    /// Argument to pass to the orchestrator
    #[arg(long = "program-arg", alias = "parg", alias = "p-arg", alias = "pa")]
    program_args: Vec<String>,
    /// Leaf's One-time orchestrator, defaults to `leafo_onetime`
    #[arg(long)]
    orchestrator: Option<PathBuf>,
    /// Argument to pass to the orchestrator
    #[arg(
        long = "orchestrator-arg",
        alias = "oarg",
        alias = "o-arg",
        alias = "oa"
    )]
    orchestrator_args: Vec<String>,
    /// The working directory to store the temporary files, e.g., mutants.
    /// Defaults to a temporary directory
    #[arg(long)]
    workdir: Option<PathBuf>,
    /// The directory to store the artifacts, e.g., crashes
    #[arg(long, default_value = "./artifacts")]
    artifacts_dir: PathBuf,
    /// The directory to store the corpus.
    /// Defaults to `workdir/corpus`.
    #[arg(long)]
    corpus_dir: Option<PathBuf>,
    /// The seed used for random generation.
    /// Set it to make the process deterministic.
    #[arg(long)]
    rand_seed: Option<u64>,
    /// Directories to load initial inputs from
    #[arg(long = "initial-input-dir")]
    initial_input_dirs: Vec<PathBuf>,
    #[command(flatten)]
    initial_rand_input: InitialRandInputArgs,
    /// Timeout for the execution of the instrumented program (orchestrator) in seconds
    #[arg(long, default_value_t = nonzero!(60))]
    timeout: NonZero<u64>,
}

#[derive(Parser, Debug)]
struct InitialRandInputArgs {
    #[arg(long = "initial_rand_input_min_size", default_value_t = nonzero!(1))]
    min_size: NonZero<usize>,
    #[arg(long = "initial_rand_input_max_size", default_value_t = nonzero!(32))]
    max_size: NonZero<usize>,
    #[arg(long = "initial_rand_input_num", default_value_t = nonzero!(8))]
    num: NonZero<usize>,
}

pub fn main() {
    utils::init_logging();

    let args = process_args();

    let observer = ();
    // For pure concolic execution, all distinct inputs are interesting
    let mut feedback = feedback_and_fast!(ConstFeedback::True,);
    let mut objective = feedback_or!(
        feedback_and_fast!(CrashFeedback::new(), feedback.clone(),),
        MutationChainAdderFeedback::default()
    );

    let mut state = StdState::new(
        StdRand::with_seed(args.rand_seed.unwrap()),
        InMemoryOnDiskCorpus::with_meta_format_and_prefix(
            args.corpus_dir
                .clone()
                .unwrap_or_else(|| args.workdir.as_ref().unwrap().join("corpus")),
            Some(OnDiskMetadataFormat::JsonPretty),
            None,
            false,
        )
        .unwrap(),
        OnDiskCorpus::new(args.artifacts_dir.join("crashes")).unwrap(),
        &mut feedback,
        &mut objective,
    )
    .unwrap();

    let scheduler = ProbabilitySamplingScheduler::<DivergingInputTestcaseScore>::new();

    let mut fuzzer =
        StdFuzzer::with_bloom_input_filter(scheduler, feedback, objective, 1 << 20, 0.01);

    let mut executor = ProgramExecutionConfigurator {
        program: args.program.unwrap(),
        args: args.program_args.clone(),
        timeout: Duration::from_secs(args.timeout.into()),
    }
    .into_executor(observer);

    let monitor = make_monitor();
    let mut manager = SimpleEventManager::new(monitor);

    let load_initial_inputs = || {
        state
            .load_initial_inputs(
                &mut fuzzer,
                &mut executor,
                &mut manager,
                &args.initial_input_dirs,
            )
            .and_then(|_| {
                if state.corpus().is_empty() {
                    let params = args.initial_rand_input;
                    let mut generator =
                        RandBytesGenerator::with_min_size(params.min_size, params.max_size);
                    state.generate_initial_inputs(
                        &mut fuzzer,
                        &mut executor,
                        &mut generator,
                        &mut manager,
                        params.num.into(),
                    )
                } else {
                    Ok(())
                }
            })
            .expect("Failed to generate the initial corpus");
    };
    load_initial_inputs();

    let mutant_work_dir = get_mutator_workdir(args.workdir.as_ref().unwrap());
    let mutator = DivergingMutator::new(
        &args.orchestrator.unwrap(),
        args.orchestrator_args.clone().into_boxed_slice(),
        &args.conc_program,
        args.program_args.clone().into_boxed_slice(),
        &mutant_work_dir,
    );

    let mut stages = tuple_list!(
        IfStage::new(
            // Do not repeat inputs
            |f, _, s: &mut StdState<_, _, _, _>, _| { is_new_or_disable(f, s) },
            tuple_list!(MultiMutationalStageWithStats::new(
                "DivergingInputGen".into(),
                "diverging_input".into(),
                mutator
            )),
        ),
        LastChildStage::default()
    );

    fuzzer
        .fuzz_loop(&mut stages, &mut executor, &mut state, &mut manager)
        .unwrap_or_else(|e| match e {
            Error::Empty(..) => {
                println!("Finished covering all instrumented parts");
            }
            _ => {
                println!("Concolic loop stopped: {}", e);
            }
        });
    let _ = std::fs::remove_dir_all(mutant_work_dir).inspect_err(|e| {
        eprintln!("Failed to remove the work directory: {}", e);
    });
}

fn process_args() -> Args {
    let mut args = Args::parse();
    args.conc_program = args
        .conc_program
        .canonicalize()
        .expect("Could not get path");
    args.program.get_or_insert(args.conc_program.clone());
    args.program = Some(
        args.program
            .unwrap()
            .canonicalize()
            .expect("Could not get path"),
    );
    args.orchestrator
        .get_or_insert_with(|| PathBuf::from(NAME_ORCHESTRATOR));
    args.workdir
        .get_or_insert(std::env::temp_dir().join("leaf").join("pure_concolic"));
    args.rand_seed.get_or_insert_with(current_nanos);
    args
}

fn get_mutator_workdir(arg_workdir: &Path) -> PathBuf {
    let path = arg_workdir.join(DIR_MUTATOR_WORK);
    std::fs::create_dir_all(&path).expect("Failed to create the work directory for mutator");
    path
}

fn make_monitor() -> impl Monitor {
    #[cfg(feature = "tui")]
    {
        use libafl::monitors::tui::TuiMonitor;
        TuiMonitor::builder()
            .title("Leaf's Pure Concolic Fuzzer")
            .enhanced_graphics(false)
            .build()
    }
    #[cfg(not(feature = "tui"))]
    {
        use libafl::monitors::SimpleMonitor;

        #[cfg(not(feature = "tui"))]
        SimpleMonitor::with_user_monitor(|s| println!("{s}"))
    }
}

fn is_new_or_disable<I, S, Z>(fuzzer: &mut Z, state: &mut S) -> Result<bool, Error>
where
    I: Clone,
    S: HasCurrentCorpusId + HasCurrentTestcase<I> + HasCorpus<I>,
    Z: HasScheduler<I, S>,
    Z::Scheduler: RemovableScheduler<I, S>,
{
    let scheduled_count = state.current_testcase().ok().map(|t| t.scheduled_count());
    match scheduled_count {
        Some(0) => Ok(true),
        Some(_) => {
            let current_id = state.current_corpus_id().unwrap().unwrap();
            let mut testcase = state.current_testcase().unwrap().clone();
            // Load the file, so cloning happens correctly.
            state.corpus().load_input_into(&mut testcase)?;

            // Removing
            let mut testcase = {
                state.corpus_mut().remove(current_id)?;
                let testcase = Some(testcase);
                fuzzer
                    .scheduler_mut()
                    .on_remove(state, current_id, &testcase)?;
                testcase.unwrap()
            };

            // The metadata is read to update the parent id of the children,
            // so it is consumed at this point.
            let children = testcase.remove_metadata::<ChildrenMetadata>().unwrap();
            let parent_id = testcase.parent_id();

            // Adding as disabled
            let new_id = {
                testcase.set_disabled(true);
                state.corpus_mut().add_disabled(testcase).inspect_err(|e| {
                    log_debug!("Problem with adding input back to the corpus {}", e)
                })
            };
            let old_id = current_id;

            if let Ok(new_id) = new_id {
                update_children_metadata(state.corpus(), old_id, new_id, parent_id, children);
            }

            state.clear_corpus_id()?;
            Ok(false)
        }
        None => {
            if state.current_corpus_id().is_ok() {
                state.clear_corpus_id()?;
            }
            Ok(false)
        }
    }
}

#[derive(Debug)]
struct ProgramExecutionConfigurator {
    program: PathBuf,
    args: Vec<String>,
    timeout: Duration,
}

impl CommandConfigurator<BytesInput> for ProgramExecutionConfigurator {
    fn spawn_child(&mut self, input: &BytesInput) -> Result<Child, Error> {
        let mut command = Command::new(&self.program);
        command
            .args(self.args.iter())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        log_debug!("Executing the program to test input: {:?}", command);

        let child = command.spawn().expect("Failed to start the program");
        let mut stdin = child.stdin.as_ref().unwrap();
        stdin.write_all(input.target_bytes().as_slice())?;
        Ok(child)
    }

    fn exec_timeout(&self) -> Duration {
        self.timeout
    }

    fn exec_timeout_mut(&mut self) -> &mut Duration {
        &mut self.timeout
    }

    fn exit_kind_from_status(&self, status: &ExitStatus) -> ExitKind {
        if status.success() {
            ExitKind::Ok
        } else {
            ExitKind::Crash
        }
    }
}

mod mut_chain {
    use super::*;

    /// The reverse relationship of the parent-child for the testcases.
    /// # Remarks
    /// Used for updating the parent id of the children when the parent is disabled and its id is changed.
    #[derive(Debug, Default, Clone, Serialize, Deserialize, SerdeAny)]
    pub(super) struct ChildrenMetadata {
        /// Given that each mutation results in new children, this the last one in the corpus.
        /// # Remarks
        /// We rely on the FIFO implementation of the corpus, so this is enough to find the rest.
        last_enabled_possible_child: Option<CorpusId>,
        /// Other children that are sequentially present before the last enabled possible child anymore.
        other_children: Vec<CorpusId>,
    }

    #[derive(Default)]
    pub(super) struct LastChildStage<I>(core::marker::PhantomData<I>);

    impl<I> Named for LastChildStage<I> {
        fn name(&self) -> &Cow<'static, str> {
            static NAME: Cow<'static, str> = Cow::Borrowed("LastChildStage");
            &NAME
        }
    }

    impl<E, EM, I, S, Z> Stage<E, EM, S, Z> for LastChildStage<I>
    where
        S: HasCorpus<I> + HasCurrentTestcase<I> + HasCurrentCorpusId,
    {
        fn perform(
            &mut self,
            _fuzzer: &mut Z,
            _executor: &mut E,
            state: &mut S,
            _manager: &mut EM,
        ) -> Result<(), Error> {
            let Ok(mut current) = state.current_testcase_mut() else {
                return Ok(());
            };

            current.metadata_or_insert_with(|| ChildrenMetadata {
                last_enabled_possible_child: state
                    .corpus()
                    .last()
                    .filter(|id| id > &state.current_corpus_id().unwrap().unwrap())
                    .filter(|id| {
                        state.corpus().get(*id).is_ok_and(|t| {
                            t.borrow().parent_id() == state.current_corpus_id().unwrap()
                        })
                    }),
                other_children: vec![],
            });

            Ok(())
        }
    }

    impl<S, I> Restartable<S> for LastChildStage<I>
    where
        S: HasMetadata + HasNamedMetadata + HasCurrentCorpusId,
    {
        #[inline]
        fn should_restart(&mut self, _state: &mut S) -> Result<bool, Error> {
            Ok(true)
        }

        #[inline]
        fn clear_progress(&mut self, _state: &mut S) -> Result<(), Error> {
            Ok(())
        }
    }

    /// A feedback that appends the mutation chain to the testcase metadata.
    /// # Remarks
    /// Meant for objectives.
    #[derive(Default)]
    pub(super) struct MutationChainAdderFeedback;

    impl<EM, I, OT, S> Feedback<EM, I, OT, S> for MutationChainAdderFeedback
    where
        S: HasCorpus<I> + HasSolutions<I>,
    {
        fn append_metadata(
            &mut self,
            state: &mut S,
            _manager: &mut EM,
            _observers: &OT,
            testcase: &mut Testcase<I>,
        ) -> Result<(), Error> {
            let mut chain = vec![];
            let mut parent_id = testcase.parent_id();
            while let Some(p_id) = parent_id {
                chain.push(p_id);

                let parent = state
                    .corpus()
                    .get_from_all(p_id)
                    .or(state.solutions().get_from_all(p_id))?;

                if let Ok(meta) = parent.borrow().metadata::<DivergingSolutionMetadata>() {
                    chain = meta
                        .mutation_chain
                        .iter()
                        .cloned()
                        .chain(chain.into_iter())
                        .collect();
                    break;
                }

                parent_id = parent.borrow().parent_id();
            }

            let filenames = chain
                .iter()
                .map(|id| {
                    state
                        .corpus()
                        .get_from_all(*id)
                        .unwrap()
                        .borrow()
                        .filename()
                        .clone()
                })
                .collect::<Vec<_>>();

            testcase.add_metadata(DivergingSolutionMetadata {
                mutation_chain: chain,
                filenames,
            });

            Ok(())
        }
    }

    impl<S> StateInitializer<S> for MutationChainAdderFeedback {}

    impl Named for MutationChainAdderFeedback {
        #[inline]
        fn name(&self) -> &Cow<'static, str> {
            static NAME: Cow<'static, str> = Cow::Borrowed("MutationChainAdderFeedback");
            &NAME
        }
    }

    /// # Remarks
    /// To be available in the objective's metadata file.
    #[derive(Debug, Default, Clone, Serialize, Deserialize, SerdeAny)]
    struct DivergingSolutionMetadata {
        mutation_chain: Vec<CorpusId>,
        filenames: Vec<Option<String>>,
    }

    pub(super) fn update_children_metadata<I>(
        corpus: &impl Corpus<I>,
        old_id: CorpusId,
        new_id: CorpusId,
        parent_id: Option<CorpusId>,
        children: Box<ChildrenMetadata>,
    ) {
        // Those that are not moved yet
        if let Some(last_enabled_possible) = children.last_enabled_possible_child {
            let last_enabled = if let Some(first) = corpus.first() {
                'last_valid: {
                    let mut id = last_enabled_possible;
                    while first <= id {
                        if let Ok(testcase) = corpus.get(id) {
                            break 'last_valid Some((id, testcase));
                        }
                        id = CorpusId(id.0 - 1);
                    }
                    None
                }
                .filter(|(_, testcase)| {
                    testcase
                        .borrow()
                        .parent_id()
                        .is_some_and(|p_id| p_id == old_id)
                })
            } else {
                None
            };

            // Mutants of the parent are sequentially added to the corpus
            if let Some((last_enabled_id, _)) = last_enabled {
                let mut current = Some(last_enabled_id);
                while let Some(id) = current {
                    let mut testcase = corpus.get(id).unwrap().borrow_mut();
                    if testcase.parent_id() == Some(old_id) {
                        testcase.set_parent_id(new_id);
                    } else {
                        break;
                    }
                    current = corpus.prev(id);
                }
            }
        }

        // Those that are moved before
        for child in children.other_children.iter() {
            let mut testcase = corpus.get_from_all(*child).unwrap().borrow_mut();
            debug_assert_eq!(
                testcase.parent_id(),
                Some(old_id),
                "Child {:?} has unexpected parent {:?}",
                child,
                testcase.parent_id()
            );
            testcase.set_parent_id(new_id);
        }

        // Let the parent know about the move
        if let Some(parent) = parent_id.map(|p_id| corpus.get_from_all(p_id).unwrap()) {
            // The parent might be disabled before, and not have the metadata anymore (won't care about the children).
            if let Ok(children) = parent.borrow_mut().metadata_mut::<ChildrenMetadata>() {
                children.other_children.push(new_id);
                // The sequence is broken, so add the previous children too.
                {
                    let mut current = Some(old_id);
                    while let Some(id) = current {
                        if corpus.get(id).unwrap().borrow_mut().parent_id() == parent_id {
                            children.other_children.push(id);
                        } else {
                            break;
                        }
                        current = corpus.prev(id);
                    }
                }
            }
        }
    }
}
