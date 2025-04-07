#![deprecated(note = "This mode is going to be replaced by an LibAFL-based one")]

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::{self, Stdio},
};

use orchestrator::utils::{ExecutionParams, stdio_from_path};
use sha2::{Digest, Sha256};

use common::{log_debug, log_info};

use crate::utils::execute_once_for_div_inputs;

use statistics::Statistics;

extern crate orchestrator;
use orchestrator::*;

fn test_in_pure_concolic_mode(program: &str, search_keyword: &str) {
    let options = Options {
        program: PathBuf::from(program),
        child_env: {
            let mut env = std::collections::HashMap::new();
            env.insert("LEAF_LOG".to_string(), "warn".to_string());
            env
        },
        current_inputs_dir: PathBuf::from("loop/current"),
        past_inputs_dir: PathBuf::from("loop/past"),
        next_inputs_dir: PathBuf::from("loop/next"),
    };
    run_loop(options, |output| {
        String::from_utf8_lossy(&output.stdout).contains(search_keyword)
    });
}

pub(crate) struct Options {
    pub program: PathBuf,
    pub child_env: HashMap<String, String>,
    pub current_inputs_dir: PathBuf,
    pub past_inputs_dir: PathBuf,
    pub next_inputs_dir: PathBuf,
}

pub(crate) fn run_loop(options: Options, target: impl Fn(&process::Output) -> bool) {
    let mut past_inputs = collect_past_inputs(&options.past_inputs_dir);
    let mut current_inputs = HashMap::new();
    let mut current_inputs_queue = VecDeque::new();
    fs::create_dir_all(&options.current_inputs_dir).unwrap_or_else(|err| {
        panic!(
            "Could not create current inputs directory {}: {}",
            options.current_inputs_dir.display(),
            err
        )
    });
    let mut stats = Statistics::new();
    loop {
        grab_new_next_inputs(
            &options.next_inputs_dir,
            &options.current_inputs_dir,
            &past_inputs,
            &mut current_inputs,
            &mut current_inputs_queue,
            &mut stats,
        );

        let Some(input) = current_inputs_queue
            .pop_front()
            .map(|k| current_inputs.remove_entry(&k).unwrap())
        else {
            break;
        };

        let output = execute_once(
            &options.program,
            &options.child_env,
            &options.next_inputs_dir,
            &input.1,
            &mut stats,
        );
        fs::rename(
            &input.1,
            options.past_inputs_dir.join(input.1.file_name().unwrap()),
        )
        .expect("Could not move file");
        past_inputs.insert(input.0);

        stats.notify_iteration();

        if target(&output) {
            log_info!("Found the target output. Input: {}", input.1.display(),);
            log_debug!(
                "Output:\n{}\nError:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
            break;
        }
    }
}

fn execute_once(
    program: &Path,
    env: &HashMap<String, String>,
    next_inputs_dir: &Path,
    input: &Path,
    stats: &mut Statistics,
) -> process::Output {
    let result = execute_once_for_div_inputs(
        ExecutionParams::new(
            &program,
            env.iter().map(|(k, v)| (k.clone(), v.clone())),
            Some(input),
            Option::<&str>::None,
            Option::<&str>::None,
            core::iter::empty(),
        ),
        next_inputs_dir,
        "next_",
    )
    .expect("Could not execute the program");
    stats.notify_execution(result.elapsed_time);
    result.output
}

fn collect_past_inputs(dir: &Path) -> HashSet<String> {
    fs::create_dir_all(dir).expect("Could not create directory");
    list_files(dir)
        .into_iter()
        .map(|(fingerprint, _)| fingerprint)
        .collect()
}

fn grab_new_next_inputs(
    next_dir: &Path,
    current_dir: &Path,
    past_inputs: &HashSet<String>,
    current_inputs: &mut HashMap<String, PathBuf>,
    current_inputs_queue: &mut VecDeque<String>,
    stats: &mut Statistics,
) {
    // NOTE: We clean up the next directory for the next execution.
    let count_before = current_inputs.len();
    let mut next_files = list_files(next_dir);
    let next_files = if next_files.is_sorted_by_key(|(_, p)| p) {
        next_files
    } else {
        // Respect the possible ordering in file names.
        next_files.sort_by(|(_, p1), (_, p2)| p1.cmp(p2));
        next_files
    };
    let next_count = next_files.len();
    for (fingerprint, path) in next_files {
        if past_inputs.contains(&fingerprint) || current_inputs.contains_key(&fingerprint) {
            fs::remove_file(&path).expect("Could not remove file");
        } else {
            let new_path = current_dir.join(get_standardized_file_name(
                past_inputs.len() + current_inputs.len(),
                &fingerprint,
            ));
            fs::rename(&path, &new_path).unwrap_or_else(|err| {
                panic!(
                    "Could not move next input {} to {}: {}",
                    path.display(),
                    new_path.display(),
                    err
                )
            });
            current_inputs.insert(fingerprint.clone(), new_path);
            current_inputs_queue.push_back(fingerprint);
        }
    }

    stats.notify_next_inputs_grabbed(next_count, current_inputs.len() - count_before);
}

fn list_files(dir: &Path) -> Vec<(String, PathBuf)> {
    fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("Could not read directory {}: {}", dir.display(), err))
        .flat_map(|e| e)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .map(|p| (get_fingerprint(&p), p))
        .collect()
}

fn get_fingerprint(file: &Path) -> String {
    let mut sha = Sha256::new();
    let mut file = OpenOptions::new()
        .read(true)
        .open(file)
        .expect("Could not open file");
    std::io::copy(&mut file, &mut sha).expect("Could not read file");
    sha.finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

fn get_standardized_file_name(index: usize, fingerprint: &str) -> String {
    format!("input_{:03}_{}.bin", index, &fingerprint[..8])
}

mod statistics {
    use std::time::{Duration, Instant};

    use common::{log_debug, log_info};

    pub(super) struct Statistics {
        start_time: Instant,
        total_exe_time: Duration,
        total_exe_count: usize,
        total_generated_inputs: usize,
        repeated_inputs: usize,
    }

    impl Statistics {
        pub fn new() -> Self {
            Self {
                start_time: Instant::now(),
                total_exe_time: Duration::default(),
                total_exe_count: 0,
                total_generated_inputs: 0,
                repeated_inputs: 0,
            }
        }

        pub fn notify_execution(&mut self, exe_time: Duration) {
            self.total_exe_time += exe_time;
            self.total_exe_count += 1;
        }

        pub fn notify_next_inputs_grabbed(&mut self, generated: usize, added: usize) {
            self.total_generated_inputs += generated;
            self.repeated_inputs += generated - added;
            if added == 0 {
                log_debug!("No different next input was found");
            } else {
                log_debug!("Found {} new inputs out of {}", added, generated);
            }
        }

        pub fn notify_iteration(&self) {
            if self.total_exe_count % 10 != 1 {
                return;
            }

            let average_exe_time = self.total_exe_time / self.total_exe_count as u32;

            log_info!(
                "{} - Elapsed time (exe/total): {}/{}, Average execution time: {}, Inputs (distinct/total): {}/{}",
                self.total_exe_count,
                self.start_time.elapsed().as_secs(),
                self.total_exe_time.as_secs(),
                average_exe_time.as_secs_f32(),
                self.total_generated_inputs - self.repeated_inputs,
                self.total_generated_inputs,
            )
        }
    }
}

fn main() {
    crate::logging::init_logging();

    log_info!("Starting the orchestrator");
    test_in_pure_concolic_mode(
        "/workspaces/Rust/leaf/leaf/samples/crates/repos/nom/target/debug/examples/css_stdin",
        "Welcome!",
    );
}
