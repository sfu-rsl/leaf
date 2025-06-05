use futures::StreamExt;
use tokio::{io::AsyncWriteExt, process::Command};
use tracing_indicatif::{span_ext::IndicatifSpanExt, style::ProgressStyle};

use common::{
    directed::ProgramMap,
    types::{DefId, InstanceKindDiscr, InstanceKindId},
};

use super::*;

pub(crate) async fn calc_program_reachability(p_map: &ProgramMap) -> ProgramReachabilityImpl {
    let call_edges = p_map
        .call_graph
        .iter()
        .flat_map(|(caller, callees)| callees.iter().map(|(_, callee, _)| (*caller, *callee)));

    fn encode_body_id(discr: InstanceKindDiscr, crate_num: u32, item_id: u32) -> u128 {
        ((discr as u128) << (u32::BITS + u32::BITS))
            | ((crate_num as u128) << u32::BITS)
            | (item_id as u128)
    }

    fn decode_body_id(encoded: u128) -> (InstanceKindDiscr, (u32, u32)) {
        (
            ((encoded >> (u32::BITS + u32::BITS)) & (u8::MAX) as u128) as InstanceKindDiscr,
            (
                ((encoded >> u32::BITS) & (u32::MAX) as u128) as u32,
                (encoded & (u32::MAX) as u128) as u32,
            ),
        )
    }

    let call_reachability = calc_reachability(
        call_edges,
        |body_id| encode_body_id(body_id.0, body_id.1.0, body_id.1.1),
        |id| {
            let (discr, (crate_num, item_id)) = decode_body_id(id);
            InstanceKindId(discr, DefId(crate_num, item_id))
        },
    )
    .await;

    let executables = executable_funcs(p_map, &call_reachability);

    let pb_span = tracing::debug_span!("CFGs Reachability");
    pb_span.pb_set_style(&ProgressStyle::default_bar());
    pb_span.pb_set_length(executables.len() as u64);
    let pb_span = pb_span.enter();

    let cfg_reachability = {
        futures::stream::iter(
            p_map
                .cfgs
                .iter()
                .filter(|(def_id, _)| executables.contains(def_id)),
        )
        .map(async move |(def_id, cfg)| {
            let edges = cfg
                .iter()
                .flat_map(|(from, targets)| targets.iter().map(|(target, _)| (*from, *target)));
            let reachability = calc_reachability(edges, |bb| *bb as u128, |bb| bb as u32).await;
            (*def_id, reachability)
        })
        .buffer_unordered(std::thread::available_parallelism().unwrap().into())
        .inspect(|_| {
            tracing::Span::current().pb_inc(1);
        })
        .collect::<MapImpl<_, _>>()
        .await
    };
    drop(pb_span);

    ProgramReachabilityImpl {
        call: call_reachability,
        cfgs: cfg_reachability,
    }
}

#[tracing::instrument(level = "debug", skip_all)]
fn executable_funcs(
    p_map: &ProgramMap,
    call_reachability: &ReachabilityImpl<BodyId, BodyId>,
) -> HashSet<BodyId> {
    let mut result = HashSet::with_capacity(p_map.call_graph.len());
    result.extend(p_map.entry_points.iter());
    /* NOTE: Because of inaccurate static information,
     * we conservatively assume everything is reachable for now. */
    if false {
        p_map
            .entry_points
            .iter()
            .flat_map(|e| {
                call_reachability
                    .reachables(e)
                    .iter(|body| body.clone())
                    .collect::<Vec<_>>()
            })
            .collect_into(&mut result);
    } else {
        result.extend(p_map.call_graph.keys());
    }

    log_debug!("Found {} executable functions", result.len());

    result.shrink_to_fit();
    result
}

async fn calc_reachability<N: Hash + Eq + Clone + 'static>(
    edges: impl Iterator<Item = (N, N)> + Send,
    encode: impl Fn(&N) -> u128 + Send,
    decode: impl Fn(u128) -> N,
) -> ReachabilityImpl<N> {
    let relations = run_reachability_tool(edges.map(move |(u, v)| (encode(&u), encode(&v))))
        .await
        .expect("Could not caculate the reachability relation");
    log_trace!("Found {} reachability relations", relations.len());
    ReachabilityImpl::from_iter(relations.into_iter().map(|(u, v)| (decode(u), decode(v))))
}

async fn run_reachability_tool(
    pairs: impl Iterator<Item = (u128, u128)>,
) -> Result<Vec<(u128, u128)>, csv::Error> {
    use std::process::Stdio;

    const PATH_REACHABILITY_ANALYZER: &str = env!("LEAFO_TOOL_REACHABILITY"); // Provided by the build script

    let mut child = Command::new(PATH_REACHABILITY_ANALYZER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    let input = {
        let mut writer = csv::WriterBuilder::new()
            .delimiter(b',')
            .double_quote(false)
            .has_headers(false)
            .from_writer(Vec::new());

        for pair in pairs {
            writer.serialize(pair)?
        }
        writer.into_inner().map_err(|e| e.into_error())?
    };
    child.stdin.take().unwrap().write_all(&input).await?;

    let output = child.wait_with_output().await?;

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b',')
        .double_quote(false)
        .has_headers(false)
        .from_reader(output.stdout.as_slice());

    reader.deserialize().try_collect()
}
