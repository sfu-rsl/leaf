use core::hash::Hash;
use std::{
    cell::LazyCell,
    collections::{BTreeSet, HashMap, HashSet},
    process::Stdio,
    rc::Rc,
};

use futures::StreamExt;
use tokio::{io::AsyncWriteExt, process::Command};
use tracing_indicatif::{span_ext::IndicatifSpanExt, style::ProgressStyle};

use common::{directed::ProgramMap, log_debug, log_trace, pri::DefId};

use crate::ProgramReachability;

pub(crate) struct Reachability<S, D = S> {
    src_dst: LazyCell<HashMap<S, BTreeSet<D>>, Box<dyn FnOnce() -> HashMap<S, BTreeSet<D>>>>,
    dst_src: Rc<HashMap<D, BTreeSet<S>>>,
    empty_dst: BTreeSet<D>,
    empty_src: BTreeSet<S>,
}

impl<S, D> Default for Reachability<S, D>
where
    S: 'static,
    D: 'static,
{
    fn default() -> Self {
        Self {
            src_dst: LazyCell::new(Box::new(HashMap::new)),
            dst_src: Default::default(),
            empty_dst: BTreeSet::new(),
            empty_src: BTreeSet::new(),
        }
    }
}

impl<S: Hash + Ord, D: Hash + Ord> Reachability<S, D> {
    fn src_dst_lazy_initializer(
        dst_src: Rc<HashMap<D, BTreeSet<S>>>,
    ) -> Box<dyn FnOnce() -> HashMap<S, BTreeSet<D>>>
    where
        S: Clone + 'static,
        D: Clone + 'static,
    {
        Box::new(move || {
            let mut src_dst: HashMap<S, BTreeSet<D>> = HashMap::with_capacity(dst_src.len());
            for (dst, srcs) in dst_src.iter() {
                for src in srcs {
                    src_dst.entry(src.clone()).or_default().insert(dst.clone());
                }
            }
            src_dst
        })
    }

    fn from_dst_src(dst_src: HashMap<D, BTreeSet<S>>) -> Self
    where
        S: Clone + 'static,
        D: Clone + 'static,
    {
        let dst_src = Rc::new(dst_src);
        let src_dst = LazyCell::new(Self::src_dst_lazy_initializer(dst_src.clone()));
        Reachability {
            dst_src,
            src_dst,
            ..Default::default()
        }
    }

    fn from_iter(pairs: impl Iterator<Item = (S, D)>) -> Self
    where
        S: Clone + 'static,
        D: Clone + 'static,
    {
        let mut src_dst: HashMap<S, BTreeSet<D>> = HashMap::new();
        let mut dst_src: HashMap<D, BTreeSet<S>> = HashMap::new();
        for (src, dst) in pairs {
            src_dst.entry(src.clone()).or_default().insert(dst.clone());
            dst_src.entry(dst.clone()).or_default().insert(src.clone());
        }
        Reachability {
            src_dst: LazyCell::new(Box::new(move || src_dst)),
            dst_src: Rc::new(dst_src),
            ..Default::default()
        }
    }

    pub(crate) fn reachers(&self, dst: &D) -> &BTreeSet<S> {
        self.dst_src.get(dst).unwrap_or(&self.empty_src)
    }

    pub(crate) fn reachables(&self, src: &S) -> &BTreeSet<D> {
        self.src_dst.get(src).unwrap_or(&self.empty_dst)
    }
}

mod cache {
    use std::{fs, path::Path};

    use serde::{Deserialize, Serialize};

    use crate::{
        ProgramReachability,
        utils::{cache_deserialize, cache_serialize},
    };

    use super::*;

    #[tracing::instrument(level = "debug", skip_all)]
    pub(crate) fn try_load(
        cache_path: &Path,
        cache_min_valid_time: std::time::SystemTime,
    ) -> Option<ProgramReachability> {
        if !(cache_path.exists()
            && fs::metadata(cache_path)
                .and_then(|m| m.modified())
                .is_ok_and(|t| t >= cache_min_valid_time))
        {
            return None;
        }

        log_debug!("Reading the cache");
        fs::OpenOptions::new()
            .read(true)
            .open(cache_path)
            .and_then(|mut f| {
                cache_deserialize::<ProgramReachability>(&mut f).map_err(std::io::Error::other)
            })
            .inspect_err(|e| log_debug!("Could not load the cache: {e}"))
            .ok()
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub(crate) fn cache(
        cache_path: &Path,
        data: &ProgramReachability,
    ) -> Result<(), std::io::Error> {
        cache_path
            .parent()
            .ok_or_else(|| std::io::Error::other(format!("Invalid path: {}", cache_path.display())))
            .and_then(fs::create_dir_all)
            .and_then(|_| {
                fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(cache_path)
            })
            .and_then(|mut f| cache_serialize(data, &mut f).map_err(std::io::Error::other))
    }

    impl<Src: Serialize, Dst: Serialize> Serialize for Reachability<Src, Dst> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            self.dst_src.serialize(serializer)
        }
    }

    impl<'de, Src: Deserialize<'de>, Dst: Deserialize<'de>> Deserialize<'de> for Reachability<Src, Dst>
    where
        Src: Hash + Ord + Clone + 'static,
        Dst: Hash + Ord + Clone + 'static,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            Ok(Self::from_dst_src(HashMap::<_, _>::deserialize(
                deserializer,
            )?))
        }
    }
}

pub(crate) use cache::{cache, try_load as try_load_from_cache};

#[tracing::instrument(level = "info", skip_all)]
pub(crate) async fn calc_program_reachability(p_map: &ProgramMap) -> ProgramReachability {
    let call_edges = p_map
        .call_graph
        .iter()
        .flat_map(|(caller, callees)| callees.iter().map(|(_, callee)| (*caller, *callee)));

    let call_reachability = calc_reachability(
        call_edges,
        |def_id| ((def_id.0 as u64) << u32::BITS) + (def_id.1 as u64),
        |id| DefId((id >> u32::BITS) as u32, (id & (u32::MAX) as u64) as u32),
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
            let reachability = calc_reachability(edges, |bb| *bb as u64, |bb| bb as u32).await;
            (*def_id, reachability)
        })
        .buffer_unordered(std::thread::available_parallelism().unwrap().into())
        .inspect(|_| {
            tracing::Span::current().pb_inc(1);
        })
        .collect::<HashMap<_, _>>()
        .await
    };
    drop(pb_span);

    ProgramReachability {
        call: call_reachability,
        cfgs: cfg_reachability,
    }
}

#[tracing::instrument(level = "debug", ret, skip_all)]
fn executable_funcs(
    p_map: &ProgramMap,
    call_reachability: &Reachability<DefId, DefId>,
) -> HashSet<DefId> {
    let mut result = HashSet::with_capacity(p_map.call_graph.len());
    result.extend(p_map.entry_points.iter());
    /* NOTE: Because of inaccurate static information,
     * we conservatively assume everything is reachable for now. */
    if false {
        p_map
            .entry_points
            .iter()
            .flat_map(|e| call_reachability.reachables(e))
            .copied()
            .collect_into(&mut result);
    } else {
        result.extend(p_map.call_graph.keys());
    }

    log_debug!("Found {} executable functions", result.len());

    result.shrink_to_fit();
    result
}

// #[tracing::instrument(level = "debug", skip_all)]
async fn calc_reachability<N: Hash + Ord + Clone + 'static>(
    edges: impl Iterator<Item = (N, N)> + Send,
    encode: impl Fn(&N) -> u64 + Send,
    decode: impl Fn(u64) -> N,
) -> Reachability<N> {
    let relations = run_reachability_tool(edges.map(move |(u, v)| (encode(&u), encode(&v))))
        .await
        .expect("Could not caculate the reachability relation");
    log_trace!("Found {} reachability relations", relations.len());
    Reachability::from_iter(relations.into_iter().map(|(u, v)| (decode(u), decode(v))))
}

async fn run_reachability_tool(
    pairs: impl Iterator<Item = (u64, u64)>,
) -> Result<Vec<(u64, u64)>, csv::Error> {
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
