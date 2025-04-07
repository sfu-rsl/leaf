use core::hash::Hash;
use std::{
    collections::{BTreeSet, HashMap},
    process::{Command, Stdio},
};

use serde::{Deserialize, Serialize};

use common::{log_debug, log_trace};

pub(crate) struct Reachability<S, D = S> {
    src_dst: HashMap<S, BTreeSet<D>>,
    dst_src: HashMap<D, BTreeSet<S>>,
    empty_dst: BTreeSet<D>,
    empty_src: BTreeSet<S>,
}

impl<S: Hash + Ord, D: Hash + Ord> Reachability<S, D> {
    fn from_iter(pairs: impl Iterator<Item = (S, D)>) -> Self
    where
        S: Clone,
        D: Clone,
    {
        let mut src_dst: HashMap<S, BTreeSet<D>> = HashMap::new();
        let mut dst_src: HashMap<D, BTreeSet<S>> = HashMap::new();
        for (src, dst) in pairs {
            src_dst.entry(src.clone()).or_default().insert(dst.clone());
            dst_src.entry(dst.clone()).or_default().insert(src.clone());
        }
        Self {
            src_dst,
            dst_src,
            empty_dst: BTreeSet::new(),
            empty_src: BTreeSet::new(),
        }
    }

    pub(crate) fn reachers(&self, dst: &D) -> &BTreeSet<S> {
        self.dst_src.get(dst).unwrap_or(&self.empty_src)
    }

    pub(crate) fn reachables(&self, src: &S) -> &BTreeSet<D> {
        self.src_dst.get(src).unwrap_or(&self.empty_dst)
    }
}

pub(crate) fn calc_reachability<N: Hash + Ord + Clone>(
    edges: impl Iterator<Item = (N, N)>,
    encode: impl Fn(&N) -> u64,
    decode: impl Fn(u64) -> N,
) -> Reachability<N> {
    log_trace!("Calculating reachability");
    let relations = run_reachability_tool(edges.map(|(u, v)| (encode(&u), encode(&v))))
        .expect("Could not caculate the reachability relation");
    log_trace!("Found {} reachability relations", relations.len());
    Reachability::from_iter(relations.into_iter().map(|(u, v)| (decode(u), decode(v))))
}

fn run_reachability_tool(
    pairs: impl Iterator<Item = (u64, u64)>,
) -> Result<Vec<(u64, u64)>, csv::Error> {
    const PATH_REACHABILITY_ANALYZER: &str = env!("LEAFO_TOOL_REACHABILITY"); // Provided by the build script

    let mut child = Command::new(PATH_REACHABILITY_ANALYZER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    {
        let stdin = child.stdin.take().unwrap();

        let mut writer = csv::WriterBuilder::new()
            .delimiter(b',')
            .double_quote(false)
            .has_headers(false)
            .from_writer(stdin);

        for pair in pairs {
            writer.serialize(pair)?
        }

        writer.flush()?;
        drop(writer);
    }

    let stdout = child.stdout.take().unwrap();
    let output_reader = std::thread::spawn(move || {
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b',')
            .double_quote(false)
            .has_headers(false)
            .from_reader(stdout);

        reader.deserialize().try_collect()
    });

    child.wait()?;

    let result = output_reader
        .join()
        .expect("Problem in reading the reachability tool's output");
    result
}

impl<Src: Serialize, Dst: Serialize> Serialize for Reachability<Src, Dst> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(
            self.src_dst
                .iter()
                .flat_map(|(src, dsts)| dsts.iter().map(move |d| (src, d))),
        )
    }
}

impl<'de, Src: Deserialize<'de>, Dst: Deserialize<'de>> Deserialize<'de> for Reachability<Src, Dst>
where
    Src: Hash + Ord + Clone,
    Dst: Hash + Ord + Clone,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from_iter(
            Vec::<(Src, Dst)>::deserialize(deserializer)?.into_iter(),
        ))
    }
}
