use core::hash::Hash;
use std::collections::HashMap;

use common::log_debug;

use crate::{abs::ConstraintKind, utils::alias::RRef};

use super::DivergenceFilter;

pub(crate) trait DepthProvider<S, C> {
    /// Returns the last depth that a step is taken with decision.
    fn last_depth(&self, step: &S, decision: ConstraintKind<&C>) -> usize;
}

impl<S, C, T: ?Sized> DepthProvider<S, C> for RRef<T>
where
    T: DepthProvider<S, C>,
{
    fn last_depth(&self, step: &S, decision: ConstraintKind<&C>) -> usize {
        self.borrow().last_depth(step, decision)
    }
}

pub(crate) struct BranchCoverageDepthDivergenceFilter<S, SC, V, VC, C, CC, DP> {
    last_accepted_depths: HashMap<(SC, VC), usize>,
    depth_provider: DP,
    distance_threshold_factor: f32,
    step_classifier: Box<dyn Fn(&S) -> SC>,
    discr_classifier: Box<dyn Fn(&V) -> VC>,
    case_classifier: Box<dyn Fn(&C) -> &CC>,
}

impl<S, SC, V, VC, C, CC, DP> BranchCoverageDepthDivergenceFilter<S, SC, V, VC, C, CC, DP> {
    pub(crate) fn new(
        snapshot: Option<HashMap<(SC, VC), usize>>,
        depth_provider: DP,
        distance_factor_threshold: f32,
        step_classifier: impl Fn(&S) -> SC + 'static,
        discr_classifier: impl Fn(&V) -> VC + 'static,
        case_classifier: impl Fn(&C) -> &CC + 'static,
    ) -> Self
    where
        SC: Eq + Hash + Clone,
        VC: Eq + Hash,
        DP: DepthProvider<SC, CC>,
    {
        assert!(distance_factor_threshold >= 1.0);
        Self {
            last_accepted_depths: snapshot.unwrap_or_default(),
            depth_provider,
            distance_threshold_factor: distance_factor_threshold,
            step_classifier: Box::new(step_classifier),
            discr_classifier: Box::new(discr_classifier),
            case_classifier: Box::new(case_classifier),
        }
    }

    pub(crate) fn get_last_depths(&self) -> &HashMap<(SC, VC), usize> {
        &self.last_accepted_depths
    }
}

impl<S, SC, V, VC, C, CC, DP> DivergenceFilter<S, V, C>
    for BranchCoverageDepthDivergenceFilter<S, SC, V, VC, C, CC, DP>
where
    SC: Eq + Hash + Clone,
    VC: Eq + Hash,
    DP: DepthProvider<SC, CC>,
{
    fn should_find(&mut self, trace: &[S], constraints: &[super::Constraint<V, C>]) -> bool {
        let (step, constraint) = (trace.last().unwrap(), constraints.last().unwrap());
        let step_class = (self.step_classifier)(step);
        let discr_class = (self.discr_classifier)(&constraint.discr);
        let kind_class = constraint.kind.as_ref().map(&mut self.case_classifier);

        let current = self.depth_provider.last_depth(&step_class, kind_class);
        let last_accepted = self
            .last_accepted_depths
            .entry((step_class, discr_class))
            .or_insert(0);

        let decision =
            if current >= (*last_accepted as f32 * self.distance_threshold_factor) as usize {
                *last_accepted = current;
                true
            } else {
                false
            };

        log_debug!(
            "Evaluated branch depth for divergence: Current = {}, Last Accepted = {}",
            current,
            last_accepted,
        );

        decision
    }
}
