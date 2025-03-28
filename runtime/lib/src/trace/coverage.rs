use std::{borrow::Borrow, collections::HashMap, fmt::Display, hash::Hash, marker::PhantomData};

use common::log_debug;
use derive_more as dm;
use serde::Serialize;

use crate::abs::ConstraintKind;

use super::{Constraint, StepInspector};

#[derive(Default, Serialize)]
pub(crate) struct CoverageData {
    count: usize,
    last_depth: usize,
}

#[derive(dm::Deref, dm::DerefMut, Serialize)]
pub(crate) struct Decisions<C>(Vec<(ConstraintKind<C>, CoverageData)>);

impl<C> Default for Decisions<C> {
    fn default() -> Self {
        Self(Default::default())
    }
}

pub(crate) struct BranchCoverageStepInspector<S: Eq + Hash, C> {
    map: HashMap<S, Decisions<C>>,
    current_depth: usize,
    _phantom: PhantomData<()>,
}

impl<S: Eq + Hash, C> BranchCoverageStepInspector<S, C> {
    pub(crate) fn new() -> Self {
        Self {
            map: Default::default(),
            current_depth: 0,
            _phantom: Default::default(),
        }
    }

    pub(crate) fn get_coverage(&self) -> &HashMap<S, Decisions<C>> {
        &self.map
    }
}

impl<S: Eq + Hash + Clone + Display, V, C: Eq + Clone + Display, SR, CR> StepInspector<SR, V, CR>
    for BranchCoverageStepInspector<S, C>
where
    SR: Borrow<S>,
    CR: Borrow<C>,
{
    fn inspect(&mut self, step: &SR, constraint: Constraint<&V, &CR>) {
        self.current_depth += 1;

        let kind = constraint.kind.map(|c| c.borrow());

        let decisions = self
            .map
            .entry(step.borrow().clone())
            .or_insert_with(Default::default);
        let index = decisions
            .iter()
            .position(|c| c.0.as_ref() == kind)
            .unwrap_or_else(|| {
                decisions.push((kind.as_ref().map(|c| (*c).clone()), Default::default()));
                decisions.len() - 1
            });
        let decision = &mut decisions[index].1;
        decision.count += 1;
        decision.last_depth = self.current_depth;

        log_debug!(
            "Branch coverage: {} at step {} has been covered {} times (depth: {})",
            kind,
            step.borrow(),
            decision.count,
            self.current_depth,
        );
    }
}

impl<S: Eq + Hash, C: Eq, SR, CR> super::divergence::DepthProvider<SR, CR>
    for BranchCoverageStepInspector<S, C>
where
    SR: Borrow<S>,
    CR: Borrow<C>,
{
    fn last_depth(&self, step: &SR, decision: ConstraintKind<&CR>) -> usize {
        let decision = decision.map(|c| c.borrow());
        self.map
            .get(step.borrow())
            .and_then(|decisions| {
                decisions
                    .iter()
                    .find(|(kind, _)| kind.as_ref() == decision)
                    .map(|(_, data)| data.last_depth)
            })
            .unwrap_or(0)
    }
}
