use core::{hash::Hash, ops::Deref};
use std::collections::{BTreeSet, HashMap, HashSet};

use derive_more as dm;

use common::{
    log_debug, log_trace,
    types::{BasicBlockIndex, InstanceKindId as BodyId},
};

pub(crate) trait QSet<T> {
    fn is_empty(&self) -> bool;

    fn contains(&self, value: &T) -> bool;

    fn iter<'a, U>(&'a self, f: impl FnMut(&T) -> U) -> impl Iterator<Item = U>
    where
        T: 'a;
}

pub(crate) trait ReachabilityBiMap<S, D = S> {
    fn reachers<'a>(&'a self, dst: &D) -> impl QSet<S> + 'a;

    fn reachables<'a>(&'a self, src: &S) -> impl QSet<D> + 'a;
}

pub(crate) trait ProgramReachability {
    fn fn_call(&self) -> &impl ReachabilityBiMap<BodyId, BodyId>;

    fn cfg<'a>(
        &'a self,
        id: BodyId,
    ) -> Option<&'a impl ReachabilityBiMap<BasicBlockIndex, BasicBlockIndex>>;
}

mod implementation {
    use super::*;

    impl<T: Eq + Hash> QSet<T> for HashSet<T> {
        fn is_empty(&self) -> bool {
            HashSet::is_empty(self)
        }

        fn contains(&self, value: &T) -> bool {
            HashSet::contains(self, value)
        }

        fn iter<'a, U>(&'a self, f: impl FnMut(&T) -> U) -> impl Iterator<Item = U>
        where
            T: 'a,
        {
            HashSet::iter(self).map(f)
        }
    }

    impl<T: Ord> QSet<T> for BTreeSet<T> {
        fn is_empty(&self) -> bool {
            BTreeSet::is_empty(self)
        }

        fn contains(&self, value: &T) -> bool {
            BTreeSet::contains(&self, value)
        }

        fn iter<'a, U>(&'a self, f: impl FnMut(&T) -> U) -> impl Iterator<Item = U>
        where
            T: 'a,
        {
            BTreeSet::iter(self).map(f)
        }
    }

    impl<T> QSet<T> for () {
        fn is_empty(&self) -> bool {
            true
        }

        fn contains(&self, _value: &T) -> bool {
            false
        }

        fn iter<'a, U>(&'a self, f: impl FnMut(&T) -> U) -> impl Iterator<Item = U>
        where
            T: 'a,
        {
            core::iter::empty().map(f)
        }
    }

    impl<S: QSet<T>, T> QSet<T> for Option<S> {
        fn is_empty(&self) -> bool {
            self.as_ref().map(|s| s.is_empty()).unwrap_or(true)
        }

        fn contains(&self, value: &T) -> bool {
            self.as_ref().map(|s| s.contains(value)).unwrap_or(false)
        }

        fn iter<'a, U>(&'a self, f: impl FnMut(&T) -> U) -> impl Iterator<Item = U>
        where
            T: 'a,
        {
            self.as_ref().map(|s| s.iter(f)).into_iter().flatten()
        }
    }

    impl<'x, S: QSet<T>, T> QSet<T> for &'x S {
        fn is_empty(&self) -> bool {
            QSet::is_empty(*self)
        }

        fn contains(&self, value: &T) -> bool {
            QSet::contains(*self, value)
        }

        fn iter<'a, U>(&'a self, f: impl FnMut(&T) -> U) -> impl Iterator<Item = U>
        where
            T: 'a,
        {
            QSet::iter(*self, f)
        }
    }
}
pub(super) mod cache {
    use std::{fs, path::Path};

    use super::*;

    #[tracing::instrument(level = "debug", skip_all)]
    pub(crate) fn try_load(
        cache_path: &Path,
        cache_min_valid_time: std::time::SystemTime,
    ) -> Option<impl ProgramReachability + use<>> {
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
            .and_then(|f| load_file(f))
            .inspect_err(|e| log_debug!("Could not load the cache: {e}"))
            .ok()
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub(crate) fn cache(
        cache_path: &Path,
        data: &ProgramReachabilityImpl,
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
            .and_then(|f| write_to_file(f, data))
    }
}

mod lazy_optimized;
mod zero_des;

type ProgramReachabilityImpl = zero_des::ProgramReachabilityPlainImpl;
type ReachabilityImpl<S, D = S> = zero_des::PlainBiMap<S, D>;
pub(crate) use cache::{cache, try_load as try_load_from_cache};
use zero_des::MapImpl;
use zero_des::cache::{load_file, write_to_file};

mod calc;
pub(crate) use calc::*;
