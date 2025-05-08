/// Implementation of the reachability map which avoids loading/calculating sets
/// until requested. This is based on the observation that the reachability map
/// may contain many entries that are not queried for the target.
use std::cell::LazyCell;
use std::rc::Rc;

use super::*;

type MapImpl<K, V> = HashMap<K, V>;
type SetImpl<K> = HashSet<K>;

trait QMap<K> {
    type Value;

    fn get(&self, key: &K) -> Option<&Self::Value>;
}

trait ToReverseMultiMap<K, V> {
    fn reverse(&self) -> MapImpl<V, SetImpl<K>>;
}

#[derive(dm::Deref)]
pub(crate) struct LazySetMultiMap<K, V> {
    inner: MapImpl<K, LazyCell<SetImpl<V>, ValueSetInitializer<V>>>,
}

pub(crate) enum ValueSetInitializer<V> {
    Empty,
    List(Vec<V>),
    Ready(SetImpl<V>),
}

impl<V: Eq + Hash> FnOnce<()> for ValueSetInitializer<V> {
    type Output = SetImpl<V>;

    extern "rust-call" fn call_once(self, _args: ()) -> Self::Output {
        match self {
            Self::Empty => Self::Output::default(),
            Self::List(list) => SetImpl::from_iter(list),
            Self::Ready(set) => set,
        }
    }
}

impl<K, V> Default for LazySetMultiMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Eq + Hash + Clone,
{
    fn default() -> Self {
        Self {
            inner: Default::default(),
        }
    }
}

impl<K: Hash + Eq, V: Hash + Eq> LazySetMultiMap<K, V> {
    pub(super) fn from_ready(map: MapImpl<K, SetImpl<V>>) -> Self {
        Self {
            inner: map
                .into_iter()
                .map(|(key, values)| (key, LazyCell::new(ValueSetInitializer::Ready(values))))
                .collect::<MapImpl<_, _>>(),
        }
    }

    pub(super) fn from_list(map: MapImpl<K, Vec<V>>) -> Self {
        Self {
            inner: map
                .into_iter()
                .map(|(key, values)| (key, LazyCell::new(ValueSetInitializer::List(values))))
                .collect::<MapImpl<_, _>>(),
        }
    }

    pub(super) fn from_iter(pairs: impl Iterator<Item = (K, V)>) -> Self
    where
        K: Clone + 'static,
        V: Clone + 'static,
    {
        let mut map: MapImpl<K, SetImpl<V>> = MapImpl::new();
        for (key, value) in pairs {
            map.entry(key).or_default().insert(value);
        }
        Self::from_ready(map)
    }
}

impl<K: Hash + Eq, V: Hash + Eq> QMap<K> for LazySetMultiMap<K, V> {
    type Value = SetImpl<V>;

    fn get(&self, key: &K) -> Option<&Self::Value> {
        self.inner.get(key).map(|v| v.deref())
    }
}

impl<K: Hash + Eq + Clone, V: Hash + Eq + Clone> ToReverseMultiMap<K, V> for LazySetMultiMap<K, V> {
    fn reverse(&self) -> MapImpl<V, SetImpl<K>> {
        let mut result: MapImpl<V, SetImpl<K>> = MapImpl::with_capacity(self.len());
        for (key, values) in self.iter() {
            for value in values.iter() {
                result.entry(value.clone()).or_default().insert(key.clone());
            }
        }
        result
    }
}

type DstSetImpl<D> = SetImpl<D>;
type SrcDstMapImpl<S, D> = MapImpl<S, SetImpl<D>>;

pub(crate) struct LazySrcDstReachabilityBiMap<M, S, D = S> {
    dst_src: Rc<M>,
    src_dst: LazyCell<SrcDstMapImpl<S, D>, SrcDstInitializer<M, S, D>>,
    empty_dst: SetImpl<D>,
    empty_src: SetImpl<S>,
}

impl<M: Default + ToReverseMultiMap<D, S>, S: Eq + Hash + Clone, D: Eq + Hash + Clone> Default
    for LazySrcDstReachabilityBiMap<M, S, D>
{
    fn default() -> Self {
        Self {
            dst_src: Default::default(),
            src_dst: LazyCell::new(SrcDstInitializer::Empty),
            empty_dst: Default::default(),
            empty_src: Default::default(),
        }
    }
}

pub(crate) enum SrcDstInitializer<M, S, D> {
    Empty,
    FromDstSrc(Rc<M>, core::marker::PhantomData<(S, D)>),
}

impl<S: Eq + Hash + Clone, D: Clone + Eq + Hash, M: ToReverseMultiMap<D, S>> FnOnce<()>
    for SrcDstInitializer<M, S, D>
{
    type Output = SrcDstMapImpl<S, D>;

    extern "rust-call" fn call_once(self, _args: ()) -> Self::Output {
        match self {
            Self::Empty => Self::Output::default(),
            Self::FromDstSrc(dst_src, _) => dst_src.reverse(),
        }
    }
}

impl<M: ToReverseMultiMap<D, S>, S: Eq + Hash + Clone, D: Clone + Eq + Hash>
    LazySrcDstReachabilityBiMap<M, S, D>
{
    pub(super) fn from_dst_src(dst_src: M) -> Self {
        let dst_src = Rc::new(dst_src);
        let src_dst = LazyCell::new(SrcDstInitializer::FromDstSrc(
            dst_src.clone(),
            Default::default(),
        ));
        Self {
            dst_src,
            src_dst,
            empty_dst: Default::default(),
            empty_src: Default::default(),
        }
    }

    pub(super) fn from_iter(
        pairs: impl Iterator<Item = (S, D)>,
    ) -> LazySrcDstReachabilityBiMap<DstSrcMapImpl<S, D>, S, D>
    where
        S: Eq + Clone,
        D: Eq + Clone,
    {
        let mut dst_src: MapImpl<D, SetImpl<S>> = MapImpl::new();
        for (src, dst) in pairs {
            dst_src.entry(dst.clone()).or_default().insert(src.clone());
        }
        LazySrcDstReachabilityBiMap::from_dst_src(DstSrcMapImpl::from_ready(dst_src))
    }
}

impl<M: QMap<D> + ToReverseMultiMap<D, S>, S: Eq + Hash + Clone, D: Eq + Hash + Clone>
    ReachabilityBiMap<S, D> for LazySrcDstReachabilityBiMap<M, S, D>
where
    M::Value: QSet<S>,
{
    type Set<'a, T>
        = &'a dyn QSet<T>
    where
        Self: 'a,
        T: 'a;

    fn reachers<'a>(&'a self, dst: &D) -> Self::Set<'a, S>
    where
        S: 'a,
    {
        self.dst_src
            .deref()
            .get(dst)
            .map_or(&self.empty_src, |s| s as &dyn QSet<S>)
    }

    fn reachables<'a>(&'a self, src: &S) -> Self::Set<'a, D>
    where
        D: 'a,
    {
        self.src_dst.deref().get(src).unwrap_or(&self.empty_dst)
    }
}

type DstSrcMapImpl<S, D> = LazySetMultiMap<D, S>;
pub(crate) type ReachabilityImpl<S, D = S> = LazySrcDstReachabilityBiMap<DstSrcMapImpl<S, D>, S, D>;

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct LazyConsProgramReachability {
    call: ReachabilityImpl<DefId, DefId>,
    cfgs: HashMap<DefId, ReachabilityImpl<BasicBlockIndex, BasicBlockIndex>>,
}

impl ProgramReachability for LazyConsProgramReachability {
    fn fn_call(&self) -> &impl ReachabilityBiMap<DefId, DefId> {
        &self.call
    }

    fn cfg<'a>(
        &'a self,
        id: DefId,
    ) -> Option<&'a impl ReachabilityBiMap<BasicBlockIndex, BasicBlockIndex>> {
        self.cfgs.get(&id)
    }
}

pub(crate) mod cache {
    use std::{
        fs,
        io::{Read, Write},
        path::Path,
    };

    use serde::{Deserialize, Serialize, de::DeserializeOwned, ser::SerializeMap};

    use super::*;

    impl<S, D> Serialize for LazySrcDstReachabilityBiMap<DstSrcMapImpl<S, D>, S, D>
    where
        S: Hash + Eq + Serialize,
        D: Hash + Eq + Serialize,
    {
        fn serialize<Ser>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error>
        where
            Ser: serde::Serializer,
        {
            self.dst_src.as_ref().serialize(serializer)
        }
    }

    impl<'de, S: Deserialize<'de>, D: Deserialize<'de>> Deserialize<'de>
        for LazySrcDstReachabilityBiMap<DstSrcMapImpl<S, D>, S, D>
    where
        S: Hash + Eq + Clone + 'static,
        D: Hash + Eq + Clone + 'static,
    {
        fn deserialize<Des>(deserializer: Des) -> Result<Self, Des::Error>
        where
            Des: serde::Deserializer<'de>,
        {
            Ok(Self::from_dst_src(DstSrcMapImpl::deserialize(
                deserializer,
            )?))
        }
    }

    impl<K: Hash + Eq + Serialize, V: Hash + Eq + Serialize> Serialize for LazySetMultiMap<K, V> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            let mut s_map = serializer.serialize_map(Some(self.inner.len()))?;
            for (key, value) in self.inner.iter() {
                s_map.serialize_key(key)?;
                s_map.serialize_value(value.deref())?;
            }

            s_map.end()
        }
    }

    impl<'de, Src: Deserialize<'de>, Dst: Deserialize<'de>> Deserialize<'de>
        for LazySetMultiMap<Src, Dst>
    where
        Src: Hash + Eq + Clone + 'static,
        Dst: Hash + Eq + Clone + 'static,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            Ok(Self::from_list(MapImpl::<_, _>::deserialize(deserializer)?))
        }
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub(crate) fn try_load(
        cache_path: &Path,
        cache_min_valid_time: std::time::SystemTime,
    ) -> Option<LazyConsProgramReachability> {
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
                cache_deserialize::<LazyConsProgramReachability>(&mut f)
                    .map_err(std::io::Error::other)
            })
            .inspect_err(|e| log_debug!("Could not load the cache: {e}"))
            .ok()
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub(crate) fn cache(
        cache_path: &Path,
        data: &LazyConsProgramReachability,
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

    pub(in super::super) fn load_file(
        mut file: impl Read,
    ) -> Result<LazyConsProgramReachability, std::io::Error> {
        cache_deserialize::<LazyConsProgramReachability>(&mut file).map_err(std::io::Error::other)
    }

    pub(in super::super) fn write_to_file(
        mut file: impl Write,
        data: &LazyConsProgramReachability,
    ) -> Result<(), std::io::Error> {
        cache_serialize(data, &mut file).map_err(std::io::Error::other)
    }

    fn cache_serialize<T: Serialize>(
        obj: &T,
        w: &mut impl std::io::Write,
    ) -> Result<(), impl std::error::Error + 'static> {
        bincode::serde::encode_into_std_write(obj, w, bincode::config::standard()).map(|_| {})
    }

    fn cache_deserialize<T: DeserializeOwned>(
        r: &mut impl std::io::Read,
    ) -> Result<T, impl std::error::Error + 'static> {
        bincode::serde::decode_from_std_read(r, bincode::config::standard())
    }
}
