#![feature(exit_status_error)]

extern crate orchestrator;
use derive_more::derive::{Deref, DerefMut};
use orchestrator::{
    args::{CommonArgs, OutputFormat},
    utils::{ExecutionParams, output_silence_as_path, report_diverging_input},
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

#[derive(Parser, Debug, Deref, DerefMut)]
struct Args {
    #[command(flatten)]
    #[deref]
    #[deref_mut]
    common: CommonArgs,
    /// Whether to print the diverging inputs as they are generated
    /// or collect them after the program finishes
    #[arg(short, long, action)]
    live: bool,
}

static COLLECTED_INPUTS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

fn main() -> ExitCode {
    let mut args = Args::parse();
    args.output_format.get_or_insert_default();

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
        watcher = Some(watch_diverging_inputs(
            &args.outdir,
            &args.output_format.unwrap(),
        ));
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
        notify_found_input(&mut inputs, &entry.unwrap(), &args.output_format.unwrap());
    });

    if !args.silent && !result.status.success() {
        eprintln!("Program exited with status: {}", result.status);
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn watch_diverging_inputs(dir: &Path, output_format: &OutputFormat) -> notify::RecommendedWatcher {
    use notify::{
        Event, EventKind, RecursiveMode, Result, Watcher, event::CreateKind, recommended_watcher,
    };
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel::<Result<Event>>();

    let mut watcher = recommended_watcher(tx).expect("Could not set up the file watcher");
    watcher
        .watch(dir, RecursiveMode::NonRecursive)
        .expect("Could not set up the file watcher");

    let output_format = output_format.clone();
    thread::spawn(move || {
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
                        notify_found_input(&mut inputs, &event.paths[0], &output_format);
                    }
                }
            }
        }
    });

    watcher
}

fn notify_found_input(
    collected_inputs: &mut HashSet<PathBuf>,
    input_path: &Path,
    output_format: &OutputFormat,
) {
    if collected_inputs.insert(input_path.canonicalize().unwrap()) {
        report_diverging_input(input_path, None, output_format);
    }
}
