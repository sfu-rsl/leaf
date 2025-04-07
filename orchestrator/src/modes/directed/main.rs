#![feature(coroutines, coroutine_trait, gen_blocks, iter_from_coroutine)]
#![feature(iterator_try_collect)]

mod reachability;
mod solve;
mod trace;
mod two_level;
mod utils;

use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    u32,
};

use clap::Parser;
use derive_more::derive::Deref;

use common::{
    directed::{DefId, ProgramMap},
    log_debug, log_info,
    pri::BasicBlockLocation,
    utils::comma_separated,
};

use orchestrator::args::CommonArgs;
use orchestrator::{utils::*, *};
use reachability::Reachability;
use serde::Serialize;
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
        .unwrap_or_else(|| program_map_path.parent().unwrap().join("reachability.json"));

    let (p_map, reachability) = load_preprocessed_info(&program_map_path, &reachability_cache_path);

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
            log_info!("Result: {:?}", result);
        } else {
            log_info!("Concrete edge: {}", edge.src.location);
        }
    }

    ExitCode::SUCCESS
}

fn load_preprocessed_info(
    p_map_path: &Path,
    reachability_cache_path: &Path,
) -> (ProgramMap, ProgramReachability) {
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

fn get_reachability(
    p_map: &ProgramMap,
    cache_path: &Path,
    cache_min_valid_time: std::time::SystemTime,
) -> ProgramReachability {
    // FIXME: Check only for entry-reachable functions

    if cache_path.exists()
        && fs::metadata(cache_path)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t >= cache_min_valid_time)
    {
        if let Ok(cached) = fs::OpenOptions::new()
            .read(true)
            .open(cache_path)
            .and_then(|f| serde_json::from_reader::<_, ProgramReachability>(f).map_err(Into::into))
            .inspect_err(|e| log_debug!("Could not load reachability info: {e}"))
        {
            return cached;
        }
    }

    log_info!("Calculating reachability");

    use reachability::calc_reachability;
    let call_edges = p_map
        .call_graph
        .iter()
        .flat_map(|(caller, callees)| callees.iter().map(|(_, callee)| (*caller, *callee)));
    let call_reachability = calc_reachability(
        call_edges,
        |def_id| ((def_id.0 as u64) << u32::BITS) + (def_id.1 as u64),
        |id| DefId((id >> u32::BITS) as u32, (id & (u32::MAX) as u64) as u32),
    );

    let cfg_reachability = p_map
        .cfgs
        .iter()
        .map(|(def_id, cfg)| {
            let edges = cfg
                .iter()
                .flat_map(|(from, targets)| targets.iter().map(|(target, _)| (*from, *target)));
            let reachability = calc_reachability(edges, |bb| *bb as u64, |bb| bb as u32);
            (*def_id, reachability)
        })
        .collect();

    let result = ProgramReachability {
        call: call_reachability,
        cfgs: cfg_reachability,
    };

    {
        let _ = cache_path
            .parent()
            .ok_or(std::io::Error::other("Invalid path"))
            .and_then(fs::create_dir_all)
            .and_then(|_| {
                fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(cache_path)
            })
            .and_then(|f| {
                let mut serializer = serde_json::Serializer::new(f);
                result.serialize(&mut serializer).map_err(Into::into)
            })
            .inspect_err(|e| log_debug!("Could not save reachability info: {e}"));
    }

    result
}

fn load_trace(full_trace_path: &Path, sym_trace_path: &Path) -> SwitchTrace {
    log_debug!(
        "Loading traces: {}, {}",
        full_trace_path.display(),
        sym_trace_path.display()
    );
    let result = trace::new_default_trace_reader(full_trace_path, sym_trace_path).read_trace();
    log_debug!("Trace: {}", comma_separated(result.iter()));
    result
}
