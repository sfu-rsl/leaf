use core::num::NonZero;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::Duration,
};

use clap::Parser;

use libafl::{executors::command::CommandConfigurator, prelude::*};
use libafl_bolts::{AsSlice, current_nanos, nonzero, rands::StdRand, tuples::tuple_list};
use libafl_leaf::{DivergingInputTestcaseScore, DivergingMutator, MultiMutationalStageWithStats};

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
    let mut feedback = feedback_and_fast!(ConstFeedback::True,);
    let mut objective = feedback_and_fast!(CrashFeedback::new(), feedback.clone());

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

    let mut stages = tuple_list!(IfStage::new(
        // Do not repeat inputs
        |f, _, s: &mut StdState<_, _, _, _>, _| { is_new_or_disable(f, s) },
        tuple_list!(MultiMutationalStageWithStats::new(
            "DivergingInputGen".into(),
            "diverging_input".into(),
            mutator
        )),
    ));

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
        None => {
            if state.current_corpus_id().is_ok() {
                state.clear_corpus_id()?;
            }
            Ok(false)
        }
        Some(0) => Ok(true),
        Some(_) => {
        let testcase = state.current_testcase().unwrap().clone();
        let current_id = state.current_corpus_id().unwrap().unwrap();
            fuzzer
                .scheduler_mut()
                .on_remove(state, current_id, &Some(testcase.clone()))?;
        let corpus = state.corpus_mut();
        corpus.add_disabled(testcase)?;
            state.clear_corpus_id()?;
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
