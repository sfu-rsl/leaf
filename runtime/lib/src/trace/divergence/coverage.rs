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
    last_divergence_depths: HashMap<(SC, VC), usize>,
    depth_provider: DP,
    distance_threshold_factor: f32,
    step_classifier: Box<dyn Fn(&S) -> SC>,
    discr_classifier: Box<dyn Fn(&V) -> VC>,
    case_classifier: Box<dyn Fn(&C) -> &CC>,
}

impl<S, SC, V, VC, C, CC, P> BranchCoverageDepthDivergenceFilter<S, SC, V, VC, C, CC, P> {
    pub(crate) fn new(
        depth_provider: P,
        distance_factor_threshold: f32,
        step_classifier: impl Fn(&S) -> SC + 'static,
        discr_classifier: impl Fn(&V) -> VC + 'static,
        case_classifier: impl Fn(&C) -> &CC + 'static,
    ) -> Self
    where
        SC: Eq + Hash + Clone,
        VC: Eq + Hash,
        P: DepthProvider<SC, CC>,
    {
        assert!(distance_factor_threshold >= 1.0);
        Self {
            last_divergence_depths: Default::default(),
            depth_provider,
            distance_threshold_factor: distance_factor_threshold,
            step_classifier: Box::new(step_classifier),
            discr_classifier: Box::new(discr_classifier),
            case_classifier: Box::new(case_classifier),
        }
    }

    pub(crate) fn get_last_depths(&self) -> &HashMap<(SC, VC), usize> {
        &self.last_divergence_depths
    }
}

impl<S, SC, V, VC, C, CC, P> DivergenceFilter<S, V, C>
    for BranchCoverageDepthDivergenceFilter<S, SC, V, VC, C, CC, P>
where
    SC: Eq + Hash + Clone,
    VC: Eq + Hash,
    P: DepthProvider<SC, CC>,
{
    fn should_find(&mut self, trace: &[S], constraints: &[super::Constraint<V, C>]) -> bool {
        let (step, constraint) = (trace.last().unwrap(), constraints.last().unwrap());
        let step_class = (self.step_classifier)(step);
        let discr_class = (self.discr_classifier)(&constraint.discr);
        let classified_decision = constraint.kind.as_ref().map(&mut self.case_classifier);

        let current_depth = self
            .depth_provider
            .last_depth(&step_class, classified_decision);
        let last_depth = self
            .last_divergence_depths
            .entry((step_class, discr_class))
            .or_insert(0);

        log_debug!(
            "Deciding convergence based on depth distance: Current = {}, Last = {}",
            current_depth,
            last_depth,
        );
        if current_depth >= (*last_depth as f32 * self.distance_threshold_factor) as usize {
            *last_depth = current_depth;
            true
        } else {
            false
        }
    }
}
