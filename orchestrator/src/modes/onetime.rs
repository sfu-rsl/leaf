#![feature(exit_status_error)]

extern crate orchestrator;
use orchestrator::*;

use clap::Parser;
use glob::glob;

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::{ExitCode, Stdio},
    sync::{Mutex, OnceLock},
    thread,
};

use utils::stdio_from_path;

fn parse_env_pair(s: &str) -> Result<(String, String), String> {
    let mut split = s.splitn(2, '=');
    split
        .next()
        .zip(split.next())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .ok_or_else(|| "Expected format: KEY=VALUE".to_owned())
}

#[derive(Parser, Debug)]
struct Args {
    /// Leaf-instrumented program to run
    #[arg(short, long)]
    program: PathBuf,
    /// Path to the file to be given to the program on stdin
    #[arg(long)]
    stdin: Option<PathBuf>,
    /// Path to the directory to write diverging inputs to
    #[arg(short, long)]
    outdir: PathBuf,
    /// Whether to return failure exit code in case the program does not finish successfully
    #[arg(short, long, action)]
    silent: bool,
    /// Whether to print the diverging inputs as they are generated
    /// or collect them after the program finishes
    #[arg(short, long, action)]
    live: bool,
    /// Environment variables to pass to the program
    #[arg(long, value_parser=parse_env_pair, number_of_values=1)]
    env: Vec<(String, String)>,
    /// Argument to pass to the program
    #[arg(last = true)]
    args: Vec<String>,
}

static COLLECTED_INPUTS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

fn main() -> ExitCode {
    let args = Args::parse();

    const FILE_PREFIX: &str = "diverging_";

    let grab = || {
        glob(
            args.outdir
                .join(format!("{}*", FILE_PREFIX))
                .to_str()
                .unwrap(),
        )
        .unwrap()
    };

    let get_std_output = || {
        if args.silent {
            Stdio::null()
        } else {
            Stdio::inherit()
        }
    };

    grab().for_each(|entry| {
        fs::remove_file(entry.unwrap()).expect("Failed to remove diverging input");
    });

    COLLECTED_INPUTS.get_or_init(Default::default);
    let mut watcher = None;
    if args.live {
        watcher = Some(watch_diverging_inputs(&args.outdir));
    }

    let result = utils::execute_once_for_div_inputs(
        &args.program,
        &std::env::vars().chain(args.env).collect(),
        stdio_from_path(args.stdin.as_deref()),
        get_std_output(),
        get_std_output(),
        &args.outdir,
        FILE_PREFIX,
        args.args,
    )
    .expect("Failed to execute the program");

    drop(watcher);

    let mut inputs = COLLECTED_INPUTS.get().unwrap().lock().unwrap();
    grab().for_each(|entry| {
        notify_found_input(&mut inputs, &entry.unwrap());
    });

    if !args.silent && !result.status.success() {
        eprintln!("Program exited with status: {}", result.status);
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn watch_diverging_inputs(dir: &Path) -> notify::RecommendedWatcher {
    use notify::{
        Event, EventKind, RecursiveMode, Result, Watcher, event::CreateKind, recommended_watcher,
    };
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel::<Result<Event>>();

    let mut watcher = recommended_watcher(tx).expect("Could not set up the file watcher");
    watcher
        .watch(dir, RecursiveMode::NonRecursive)
        .expect("Could not set up the file watcher");

    thread::spawn(|| {
        for res in rx {
            match res {
                Err(e) => {
                    eprintln!("Problem in watching diverging inputs: {:?}", e);
                    return;
                }
                Ok(event) => {
                    if matches!(event.kind, EventKind::Create(CreateKind::File)) {
                        debug_assert_eq!(event.paths.len(), 1);
                        let mut inputs = COLLECTED_INPUTS.get().unwrap().lock().unwrap();
                        notify_found_input(&mut inputs, &event.paths[0]);
                    }
                }
            }
        }
    });

    watcher
}

fn notify_found_input(collected_inputs: &mut HashSet<PathBuf>, input_path: &Path) {
    if collected_inputs.insert(input_path.canonicalize().unwrap()) {
        println!("{}", input_path.display());
    }
}
