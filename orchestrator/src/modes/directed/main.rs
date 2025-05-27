#![feature(coroutines, coroutine_trait, gen_blocks, iter_from_coroutine)]
#![feature(iterator_try_collect)]
#![feature(iter_collect_into)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(exit_status_error)]
#![feature(substr_range)]
#![feature(hash_extract_if)]
#![feature(iterator_try_reduce)]
#![feature(iter_map_windows)]

mod outgen;
mod reachability;
mod solve;
mod trace;
mod two_level;
mod utils;

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, ValueEnum};
use derive_more::derive::Deref;

use common::{
    directed::ProgramMap, log_debug, log_info, pri::BasicBlockLocation, utils::comma_separated,
};

use orchestrator::args::CommonArgs;
use orchestrator::{utils::*, *};
use outgen::NextInputGenerator;
use reachability::{ProgramReachability, QSet, ReachabilityBiMap};
use trace::{SwitchStep, SwitchTrace, TraceReader};
use two_level::DirectedEdge;

#[derive(ValueEnum, Debug, Clone, Copy)]
enum Scoring {
    PrefixLen,
}

#[derive(Parser, Debug, Deref)]
struct Args {
    #[command(flatten)]
    #[deref]
    common: CommonArgs,
    #[arg(long)]
    program_map: Option<PathBuf>,
    #[arg(long)]
    reachability: Option<PathBuf>,
    #[arg(long, short)]
    target: BasicBlockLocation,
    #[arg(long)]
    scoring: Option<Scoring>,
}

fn main() -> ExitCode {
    crate::logging::init_logging();

    let args = Args::parse();
    log_debug!("Parsed Args: {:?}", args);

    let (p_map, reachability) = load_preprocessed_info(&args);

    assert_target_is_reachable(&reachability, &args.target);

    log_info!("Executing the program");
    let (input, stdin_path) = prepare_input(&args.stdin);
    let trace = match execute_and_load_trace(&args, stdin_path) {
        Ok(value) => value,
        Err(value) => return value,
    };

    let solver = solve::Solver::new(&trace, &input, &p_map, &reachability);
    let director = two_level::Director::new(&trace);
    let mut next_input_dumper = NextInputGenerator::new(
        &args.outdir,
        &args.output_format.unwrap_or_default(),
        args.target,
        &input,
    );

    for edge in director.find_edges_toward(&p_map, &reachability, &args.target) {
        let prefix_len = trace
            .element_offset(edge.src)
            .expect("Inconsistent referencing");
        process_edge(
            &p_map,
            &solver,
            &mut next_input_dumper,
            edge,
            prefix_len,
            args.scoring.as_ref(),
        );
    }

    log_info!("Generated {} new inputs", next_input_dumper.total_count());

    ExitCode::SUCCESS
}

#[tracing::instrument(level = "debug")]
fn load_preprocessed_info(args: &Args) -> (ProgramMap, impl ProgramReachability) {
    log_info!("Loading pre-processed information about the program");

    let p_map_path = args
        .program_map
        .clone()
        .or_else(|| try_find_program_map(&args.program))
        .expect("Could not find the program map file");

    let reachability_cache_path = args
        .reachability
        .clone()
        .unwrap_or_else(|| p_map_path.parent().unwrap().join("reachability.bin"));

    log_debug!("Loading program map");
    let p_map = ProgramMap::read(&p_map_path).expect("Failed to read program map");

    let reachability = get_reachability(
        &p_map,
        &reachability_cache_path,
        fs::metadata(p_map_path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::now()),
    );
    (p_map, reachability)
}

fn try_find_program_map(program_path: &Path) -> Option<PathBuf> {
    use common::utils::try_join_path;
    const NAME: &str = "program_map.json";

    let program_dir = program_path.parent().unwrap();
    try_join_path(program_dir, NAME).or_else(|| try_join_path(program_dir.join("deps"), NAME))
}

#[tracing::instrument(level = "debug", skip_all)]
fn get_reachability(
    p_map: &ProgramMap,
    cache_path: &Path,
    cache_min_valid_time: std::time::SystemTime,
) -> impl ProgramReachability + use<> {
    use reachability::*;

    if let Some(cached) = try_load_from_cache(cache_path, cache_min_valid_time) {
        return cached;
    }

    log_info!("Calculating reachabilities");
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .unwrap()
        .block_on(calc_program_reachability(p_map));

    let _ = cache(cache_path, &result)
        .inspect_err(|e| log_debug!("Could not cache reachability info: {e}"));

    get_reachability(p_map, cache_path, cache_min_valid_time)
}

fn assert_target_is_reachable(
    reachability: &impl ProgramReachability,
    target: &BasicBlockLocation,
) {
    assert!(
        reachability
            .cfg(target.body)
            .is_some_and(|cfg| target.index == 0 || !cfg.reachers(&target.index).is_empty()),
        "Target not found/unreachable in the program map: {}",
        target
    );
}

fn prepare_input(args_stdin: &Option<impl AsRef<Path>>) -> (Vec<u8>, Option<PathBuf>) {
    let (input, stdin_path) = args_stdin
        .as_ref()
        .map(|p| read_input(p).expect("Failed to read/write stdin"))
        .map(|(input, p)| (input, Some(p)))
        .unwrap_or_default();
    (input, stdin_path)
}

fn read_input(stdin_path: impl AsRef<Path>) -> Result<(Vec<u8>, PathBuf), std::io::Error> {
    if is_inherit(&stdin_path) {
        let mut contents = Vec::new();
        std::io::stdin().read_to_end(&mut contents)?;
        let path = std::env::temp_dir()
            .join("leaf")
            .join("directed")
            .join(&format!(
                "current_input_{}",
                common::utils::current_instant_millis()
            ));
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, &contents)?;
        Ok((contents, path))
    } else {
        let path = stdin_path.as_ref().to_path_buf();
        let contents = std::fs::read(&path)?;
        Ok((contents, path))
    }
}

fn execute_and_load_trace(
    args: &Args,
    stdin_path: Option<PathBuf>,
) -> Result<Vec<SwitchStep>, ExitCode> {
    const NAME_FULL_TRACE: &str = "full_trace";
    const NAME_SYM_TRACE: &str = "sym_trace";
    const NAME_PRECONDITIONS: &str = "preconditions";

    let exe_result = execute_once_for_trace(
        ExecutionParams::new(
            &args.program,
            args.env.iter().cloned(),
            stdin_path,
            output_silence_as_path(args.silent),
            output_silence_as_path(args.silent),
            args.args.iter().cloned(),
        ),
        &args.outdir,
        NAME_FULL_TRACE,
        NAME_SYM_TRACE,
        NAME_PRECONDITIONS,
    )
    .expect("Failed to execute the program");

    exe_result.status.exit_ok().map_err(|s| {
        if !args.silent {
            eprintln!("Program exited with status: {}", s);
            ExitCode::FAILURE
        } else {
            log_debug!("Program did not exit successfully: {}, Exiting silently", s);
            ExitCode::SUCCESS
        }
    })?;

    let artifact_path = |name| args.outdir.join(name).with_extension("jsonl");

    Ok(load_trace(
        &artifact_path(NAME_FULL_TRACE),
        &artifact_path(NAME_SYM_TRACE),
        &artifact_path(NAME_PRECONDITIONS),
    ))
}

#[tracing::instrument(level = "debug")]
fn load_trace(
    full_trace_path: &Path,
    sym_trace_path: &Path,
    preconditions_path: &Path,
) -> SwitchTrace {
    let result =
        trace::new_default_trace_reader(full_trace_path, sym_trace_path, preconditions_path)
            .read_trace();
    log_info!("Trace loaded with {} steps", result.len());
    log_debug!("Trace: {}...", comma_separated(result.iter().take(10)));
    result
}

fn process_edge(
    p_map: &ProgramMap,
    solver: &solve::Solver<'_, '_>,
    next_input_dumper: &mut NextInputGenerator,
    edge: DirectedEdge<'_>,
    prefix_len: usize,
    scoring: Option<&Scoring>,
) {
    let edge = DirectedEdge {
        metadata: p_map.cfgs[&edge.src.location.body][&edge.src.location.index].as_slice(),
        src: edge.src,
        dst: edge.dst,
    };
    if let Some(results) = solver.satisfy_edge(&edge) {
        for result in results {
            log_debug!("Result: {:?}", result.0);
            if matches!(result.0, z3::SatResult::Sat) {
                next_input_dumper.dump_as_next_input(
                    &result.1,
                    scoring.map(|s| match s {
                        Scoring::PrefixLen => prefix_len as f64,
                    }),
                )
            }
        }
    }
}
