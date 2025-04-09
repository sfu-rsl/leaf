#![feature(coroutines, coroutine_trait, gen_blocks, iter_from_coroutine)]
#![feature(iterator_try_collect)]
#![feature(iter_collect_into)]

mod reachability;
mod solve;
mod trace;
mod two_level;
mod utils;

use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::Parser;
use derive_more::derive::Deref;

use common::{
    directed::ProgramMap, log_debug, log_info, pri::BasicBlockLocation, utils::comma_separated,
};

use orchestrator::args::CommonArgs;
use orchestrator::{utils::*, *};
use reachability::Reachability;
use trace::{SwitchStep, SwitchTrace, TraceReader};
use two_level::{DirectedEdge, ProgramReachability};

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
}

fn main() -> ExitCode {
    crate::logging::init_logging();

    let args = Args::parse();

    log_info!("Loading pre-processed information about the program");

    let program_map_path = args
        .program_map
        .clone()
        .unwrap_or_else(|| args.program.parent().unwrap().join("program_map.json"));

    let reachability_cache_path = args
        .reachability
        .clone()
        .unwrap_or_else(|| program_map_path.parent().unwrap().join("reachability.bin"));

    let (p_map, reachability) = load_preprocessed_info(&program_map_path, &reachability_cache_path);

    assert!(
        p_map
            .cfgs
            .get(&args.target.body)
            .is_some_and(|cfg| cfg.contains_key(&args.target.index)),
        "Target not found in the program map: {}",
        args.target
    );

    const NAME_FULL_TRACE: &str = "full_trace";
    const NAME_SYM_TRACE: &str = "sym_trace";

    log_info!("Executing the program");

    let result = execute_once_for_trace(
        ExecutionParams::new(
            &args.program,
            args.env.iter().cloned(),
            args.stdin.as_deref(),
            output_silence_as_path(args.silent),
            output_silence_as_path(args.silent),
            args.args.iter().cloned(),
        ),
        &args.outdir,
        NAME_FULL_TRACE,
        NAME_SYM_TRACE,
    )
    .expect("Failed to execute the program");

    let trace = load_trace(
        &args.outdir.join(NAME_FULL_TRACE).with_extension("json"),
        &args.outdir.join(NAME_SYM_TRACE).with_extension("json"),
    );

    let solver = solve::Solver::new(&trace);
    let director = two_level::Director::new(&trace);

    for edge in director.find_edges_toward(&p_map, &reachability, &args.target) {
        let edge = DirectedEdge {
            metadata: p_map.cfgs[&edge.src.location.body][&edge.src.location.index].as_slice(),
            src: edge.src,
            dst: edge.dst,
        };
        let result = solver.satisfy_edge(&edge);
        if let Some(result) = result {
            log_info!("Result: {:?}", result.0);
        } else {
            log_info!("Concrete edge: {}", edge.src.location);
        }
    }

    ExitCode::SUCCESS
}

#[tracing::instrument(level = "debug")]
fn load_preprocessed_info(
    p_map_path: &Path,
    reachability_cache_path: &Path,
) -> (ProgramMap, ProgramReachability) {
    log_debug!("Loading program map");
    let p_map = ProgramMap::read(p_map_path).expect("Failed to read program map");

    let reachability = get_reachability(
        &p_map,
        reachability_cache_path,
        fs::metadata(p_map_path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::now()),
    );
    (p_map, reachability)
}

#[tracing::instrument(level = "debug", skip_all)]
fn get_reachability(
    p_map: &ProgramMap,
    cache_path: &Path,
    cache_min_valid_time: std::time::SystemTime,
) -> ProgramReachability {
    use reachability::*;

    if let Some(cached) = try_load_from_cache(cache_path, cache_min_valid_time) {
        return cached;
    }

    let result = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(calc_program_reachability(p_map));

    let _ = cache(cache_path, &result)
        .inspect_err(|e| log_debug!("Could not cache reachability info: {e}"));

    result
}

#[tracing::instrument(level = "debug")]
fn load_trace(full_trace_path: &Path, sym_trace_path: &Path) -> SwitchTrace {
    let result = trace::new_default_trace_reader(full_trace_path, sym_trace_path).read_trace();
    log_info!("Trace loaded with {} steps", result.len());
    log_debug!("Trace: {}...", comma_separated(result.iter().take(10)));
    result
}
