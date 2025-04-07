#![feature(exit_status_error)]

extern crate orchestrator;
use derive_more::derive::Deref;
use orchestrator::{
    args::CommonArgs,
    utils::{ExecutionParams, output_silence_as_path},
    *,
};

use clap::Parser;
use glob::glob;

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Mutex, OnceLock},
    thread,
};

#[derive(Parser, Debug, Deref)]
struct Args {
    #[command(flatten)]
    #[deref]
    common: CommonArgs,
    /// Whether to print the diverging inputs as they are generated
    /// or collect them after the program finishes
    #[arg(short, long, action)]
    live: bool,
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

    grab().for_each(|entry| {
        fs::remove_file(entry.unwrap()).expect("Failed to remove diverging input");
    });

    COLLECTED_INPUTS.get_or_init(Default::default);
    let mut watcher = None;
    if args.live {
        watcher = Some(watch_diverging_inputs(&args.outdir));
    }

    let result = utils::execute_once_for_div_inputs(
        ExecutionParams::new(
            &args.program,
            args.env.iter().cloned(),
            args.stdin.as_deref(),
            output_silence_as_path(args.silent),
            output_silence_as_path(args.silent),
            args.args.iter().cloned(),
        ),
        &args.outdir,
        FILE_PREFIX,
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
