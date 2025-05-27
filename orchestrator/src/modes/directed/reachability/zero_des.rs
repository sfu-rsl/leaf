use rkyv::{
    Archive, Archived, Deserialize, Serialize, api::high::HighDeserializer,
    collections::swiss_table::ArchivedHashSet, rancor::Error,
};

use super::*;

pub(super) type MapImpl<K, V> = HashMap<K, V>;
type SetImpl<K> = HashSet<K>;

#[derive(Archive, Serialize)]
pub(crate) struct PlainBiMap<S, D = S> {
    dst_src: MapImpl<D, SetImpl<S>>,
    src_dst: MapImpl<S, SetImpl<D>>,
}

impl<S: Eq + Hash + Clone, D: Eq + Hash + Clone> PlainBiMap<S, D> {
    pub(super) fn from_dst_src(dst_src: MapImpl<D, SetImpl<S>>) -> Self {
        let mut src_dst: MapImpl<S, SetImpl<D>> = MapImpl::with_capacity(dst_src.len());
        for (key, values) in dst_src.iter() {
            for value in values.iter() {
                src_dst
                    .entry(value.clone())
                    .or_default()
                    .insert(key.clone());
            }
        }
        Self { dst_src, src_dst }
    }
}

impl<S: Eq + Hash + Clone, D: Eq + Hash + Clone> FromIterator<(S, D)> for PlainBiMap<S, D> {
    fn from_iter<T: IntoIterator<Item = (S, D)>>(iter: T) -> Self {
        let mut dst_src: MapImpl<D, SetImpl<S>> = MapImpl::new();
        for (src, dst) in iter {
            dst_src.entry(dst.clone()).or_default().insert(src.clone());
        }
        Self::from_dst_src(dst_src)
    }
}

impl<S: Hash + Eq, D: Hash + Eq> ReachabilityBiMap<S, D> for PlainBiMap<S, D> {
    // NOTE: We cannot provide a static empty HashSet. An alternative would be using BTreeSet.

    fn reachers<'a>(&'a self, dst: &D) -> impl QSet<S> + 'a {
        self.dst_src.get(dst)
    }

    fn reachables<'a>(&'a self, src: &S) -> impl QSet<D> + 'a {
        self.src_dst.get(src)
    }
}

impl<S: Eq + Hash + Archive, D: Eq + Hash + Archive> ReachabilityBiMap<S, D>
    for ArchivedPlainBiMap<S, D>
where
    S::Archived: Eq + Hash + for<'a> From<&'a S>,
    S::Archived: Deserialize<S, HighDeserializer<Error>>,
    D::Archived: Eq + Hash + for<'a> From<&'a D>,
    D::Archived: Deserialize<D, HighDeserializer<Error>>,
{
    fn reachers<'a>(&'a self, dst: &D) -> impl QSet<S> + 'a {
        self.dst_src.get(&dst.into())
    }

    fn reachables<'a>(&'a self, src: &S) -> impl QSet<D> + 'a {
        self.src_dst.get(&src.into())
    }
}

impl<T: Archive> QSet<T> for ArchivedHashSet<Archived<T>>
where
    Archived<T>: Hash + Eq + for<'a> From<&'a T>,
    Archived<T>: Deserialize<T, HighDeserializer<Error>>,
{
    fn is_empty(&self) -> bool {
        ArchivedHashSet::is_empty(self)
    }

    fn contains(&self, value: &T) -> bool {
        ArchivedHashSet::contains(self, &value.into())
    }

    fn iter<'a, U>(&'a self, mut f: impl FnMut(&T) -> U) -> impl Iterator<Item = U>
    where
        T: 'a,
    {
        ArchivedHashSet::iter(self).map(move |e| f(&rkyv::deserialize(e).unwrap()))
    }
}

#[derive(Archive, Serialize)]
pub(crate) struct ProgramReachabilityPlainImpl {
    pub call: PlainBiMap<BodyId, BodyId>,
    pub cfgs: MapImpl<BodyId, PlainBiMap<BasicBlockIndex, BasicBlockIndex>>,
}

impl ProgramReachability for ProgramReachabilityPlainImpl {
    fn fn_call(&self) -> &impl ReachabilityBiMap<BodyId, BodyId> {
        &self.call
    }

    fn cfg<'a>(
        &'a self,
        id: BodyId,
    ) -> Option<&'a impl ReachabilityBiMap<BasicBlockIndex, BasicBlockIndex>> {
        self.cfgs.get(&id)
    }
}

pub(crate) struct OwnedArchivedProgramReachabilityPlainImpl(pub(super) Box<[u8]>);

impl Deref for OwnedArchivedProgramReachabilityPlainImpl {
    type Target = ArchivedProgramReachabilityPlainImpl;

    fn deref(&self) -> &Self::Target {
        unsafe { rkyv::access_unchecked(&self.0) }
    }
}

impl ProgramReachability for OwnedArchivedProgramReachabilityPlainImpl {
    fn fn_call(&self) -> &impl ReachabilityBiMap<BodyId, BodyId> {
        self.deref().fn_call()
    }

    fn cfg<'a>(
        &'a self,
        id: BodyId,
    ) -> Option<&'a impl ReachabilityBiMap<BasicBlockIndex, BasicBlockIndex>> {
        self.deref().cfg(id)
    }
}

impl ProgramReachability for ArchivedProgramReachabilityPlainImpl {
    fn fn_call(&self) -> &impl ReachabilityBiMap<BodyId, BodyId> {
        &self.call
    }

    fn cfg<'a>(
        &'a self,
        id: BodyId,
    ) -> Option<&'a impl ReachabilityBiMap<BasicBlockIndex, BasicBlockIndex>> {
        self.cfgs.get(&id.into())
    }
}

pub(super) mod cache {
    use std::io::{Read, Write};

    use rkyv::{rancor, ser::writer::IoWriter};

    use super::*;

    pub(in super::super) fn load_file(
        mut file: impl Read,
    ) -> Result<OwnedArchivedProgramReachabilityPlainImpl, std::io::Error> {
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map(|_| OwnedArchivedProgramReachabilityPlainImpl(buf.into_boxed_slice()))
    }

    pub(in super::super) fn write_to_file(
        file: impl Write,
        data: &ProgramReachabilityPlainImpl,
    ) -> Result<(), std::io::Error> {
        rkyv::api::high::to_bytes_in(data, IoWriter::new(file))
            .map(|_| ())
            .map_err(|e: rancor::Error| std::io::Error::other(e))
    }
}
