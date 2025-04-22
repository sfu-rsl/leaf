use core::num::NonZero;
use std::{
    borrow::Cow,
    collections::HashSet,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::Duration,
};

use clap::Parser;
use sha2::digest::{self};
use sha2::{self, Digest, Sha256};

use libafl::{
    executors::command::CommandConfigurator, prelude::*, stages::mutational::MultiMutationalStage,
};
use libafl_bolts::{AsSlice, Named, current_nanos, nonzero, rands::StdRand, tuples::tuple_list};
use libafl_leaf::DivergingMutator;

use ::common::log_debug;

const NAME_ORCHESTRATOR: &str = "leafo_onetime";

const DIR_MUTATOR_WORK: &str = "mutator";

mod utils;

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
    let mut feedback = feedback_and_fast!(DistinctInputFeedback::default());
    let mut objective = feedback_and_fast!(CrashFeedback::new(), DistinctInputFeedback::default());

    let mut state = StdState::new(
        StdRand::with_seed(current_nanos()),
        InMemoryOnDiskCorpus::new(
            args.corpus_dir
                .clone()
                .unwrap_or_else(|| args.workdir.as_ref().unwrap().join("corpus")),
        )
        .unwrap(),
        OnDiskCorpus::new(args.artifacts_dir.join("crashes")).unwrap(),
        &mut feedback,
        &mut objective,
    )
    .unwrap();

    let scheduler = QueueScheduler::new();

    let mut fuzzer = StdFuzzer::new(scheduler, feedback, objective);

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

    let mut stages = tuple_list!(IfStage::new(
        // Do not repeat inputs
        |_, _, s: &mut StdState<_, _, _, _>, _| { is_new_or_disable(s) },
        tuple_list!(MultiMutationalStage::new(mutator)),
    ));

    fuzzer
        .fuzz_loop(&mut stages, &mut executor, &mut state, &mut manager)
        .unwrap_or_else(|e| match e {
            Error::Empty(..) => {
                println!("Finished covering all instrumented parts");
            }
            _ => {
                println!("Concolic loop stopped: {:?}", e);
            }
        });
    let _ = std::fs::remove_dir_all(mutant_work_dir).inspect_err(|e| {
        eprintln!("Failed to remove the work directory: {}", e);
    });
}

fn process_args() -> Args {
    let mut args = Args::parse();
    args.program.get_or_insert(args.conc_program.clone());
    args.orchestrator
        .get_or_insert_with(|| PathBuf::from(NAME_ORCHESTRATOR));
    args.workdir
        .get_or_insert(std::env::temp_dir().join("leaf").join("pure_concolic"));
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
        SimpleMonitor::new(|s| println!("{s}"))
    }
}

fn is_new_or_disable<I: Clone, S: HasCurrentCorpusId + HasCurrentTestcase<I> + HasCorpus<I>>(
    state: &mut S,
) -> Result<bool, Error> {
    if state.current_testcase()?.scheduled_count() > 0 {
        let testcase = state.current_testcase().unwrap().clone();
        let current_id = state.current_corpus_id().unwrap().unwrap();
        let corpus = state.corpus_mut();
        corpus.remove(current_id)?;
        corpus.add_disabled(testcase)?;
        Ok(false)
    } else {
        Ok(true)
    }
}

type Hasher = Sha256;
type Fingerprint = digest::Output<Hasher>;
/// Feedback which considers distinct inputs as interesting based on their fingerprint (digest).
#[derive(Default)]
struct DistinctInputFeedback {
    calculator: Hasher,
    buf: Fingerprint,
    tested_inputs: HashSet<Fingerprint>,
}

impl Named for DistinctInputFeedback {
    fn name(&self) -> &Cow<'static, str> {
        &Cow::Borrowed("DistinctInputFeedback")
    }
}

impl<S> StateInitializer<S> for DistinctInputFeedback {}

impl<EM, I: HasTargetBytes, OT, S> Feedback<EM, I, OT, S> for DistinctInputFeedback {
    fn is_interesting(
        &mut self,
        _state: &mut S,
        _manager: &mut EM,
        input: &I,
        _observers: &OT,
        _exit_kind: &ExitKind,
    ) -> Result<bool, Error> {
        self.calculator.update(input.target_bytes().as_slice());
        self.calculator.finalize_into_reset(&mut self.buf);
        let result = self.tested_inputs.insert(self.buf);
        Ok(result)
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
        log_debug!("Executing the program to test input");

        let mut command = Command::new(&self.program);

        command
            .args(self.args.iter())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

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
