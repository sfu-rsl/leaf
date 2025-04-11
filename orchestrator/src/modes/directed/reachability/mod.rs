use core::{hash::Hash, ops::Deref};
use std::collections::{BTreeSet, HashMap, HashSet};

use derive_more as dm;

use common::{
    log_debug, log_trace,
    pri::{BasicBlockIndex, DefId},
};

pub(crate) trait QSet<T> {
    fn contains(&self, value: &T) -> bool;

    fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = &'a T> + 'a>
    where
        T: 'a;
}

pub(crate) trait ReachabilityBiMap<S, D = S> {
    type Set<'a, T>: QSet<T>
    where
        Self: 'a,
        T: 'a;

    fn reachers<'a>(&'a self, dst: &D) -> Self::Set<'a, S>
    where
        S: 'a;

    fn reachables<'a>(&'a self, src: &S) -> Self::Set<'a, D>
    where
        D: 'a;
}

pub(crate) trait ProgramReachability {
    fn fn_call(&self) -> &impl ReachabilityBiMap<DefId, DefId>;

    fn cfg<'a>(
        &'a self,
        id: DefId,
    ) -> Option<&'a impl ReachabilityBiMap<BasicBlockIndex, BasicBlockIndex>>;
}

mod implementation {
    use super::*;

    impl<T: Eq + Hash> QSet<T> for HashSet<T> {
        fn contains(&self, value: &T) -> bool {
            HashSet::contains(self, value)
        }

        fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = &'a T> + 'a>
        where
            T: 'a,
        {
            Box::new(HashSet::iter(self))
        }
    }

    impl<T: Ord> QSet<T> for BTreeSet<T> {
        fn contains(&self, value: &T) -> bool {
            BTreeSet::contains(&self, value)
        }

        fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = &'a T> + 'a>
        where
            T: 'a,
        {
            Box::new(BTreeSet::iter(self))
        }
    }

    impl<T> QSet<T> for () {
        fn contains(&self, _value: &T) -> bool {
            false
        }

        fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = &'a T> + 'a>
        where
            T: 'a,
        {
            Box::new(core::iter::empty())
        }
    }

    impl<'x, S: QSet<T>, T> QSet<T> for &'x S {
        fn contains(&self, value: &T) -> bool {
            QSet::contains(*self, value)
        }
        fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = &'a T> + 'a>
        where
            T: 'a,
        {
            QSet::iter(*self)
        }
    }

    impl<T> QSet<T> for &'_ dyn QSet<T> {
        fn contains(&self, value: &T) -> bool {
            QSet::contains(*self, value)
        }

        fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = &'a T> + 'a>
        where
            Self: Sized,
            T: 'a,
        {
            QSet::iter(*self)
        }
    }

    pub(crate) struct EmptyReachability;

    impl<S, D> ReachabilityBiMap<S, D> for EmptyReachability {
        type Set<'a, T>
            = ()
        where
            Self: 'a,
            T: 'a;

        fn reachables<'a>(&'a self, _src: &S) -> Self::Set<'a, D>
        where
            D: 'a,
        {
            Default::default()
        }

        fn reachers<'a>(&'a self, _dst: &D) -> Self::Set<'a, S>
        where
            S: 'a,
        {
            Default::default()
        }
    }
}
pub(crate) use implementation::*;

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
