use std::{collections::HashMap, fmt::Display, hash::Hash, marker::PhantomData};

use common::log_debug;
use derive_more as dm;
use serde::Serialize;

use crate::abs::ConstraintKind;

use super::{Constraint, StepInspector};

#[derive(dm::Deref, dm::DerefMut, Serialize)]
pub(crate) struct Counts<C>(Vec<(ConstraintKind<C>, usize)>);

impl<C> Default for Counts<C> {
    fn default() -> Self {
        Self(Default::default())
    }
}

pub(crate) struct BranchCoverageStepInspector<S: Eq + Hash, V, C> {
    map: HashMap<S, Counts<C>>,
    _phantom: PhantomData<(V,)>,
}

impl<S: Eq + Hash, V, C> BranchCoverageStepInspector<S, V, C> {
    pub(crate) fn new() -> Self {
        Self {
            map: Default::default(),
            _phantom: Default::default(),
        }
    }

    pub(crate) fn get_coverage(&self) -> &HashMap<S, Counts<C>> {
        &self.map
    }
}

impl<S: Eq + Hash + Clone + Display, V: Display, C: Eq + Clone + Display> StepInspector<S, V, C>
    for BranchCoverageStepInspector<S, V, C>
{
    fn inspect(&mut self, step: &S, constraint: &Constraint<V, C>) {
        let counts = self
            .map
            .entry(step.clone())
            .or_insert_with(Default::default);
        let index = counts
            .iter()
            .position(|c| c.0 == constraint.kind)
            .unwrap_or_else(|| {
                counts.push((constraint.kind.clone(), 0));
                counts.len() - 1
            });
        counts[index].1 += 1;

        log_debug!(
            "Branch coverage: {} at step {} has been covered {} times",
            constraint.kind,
            step,
            counts[index].1
        );
    }
}
