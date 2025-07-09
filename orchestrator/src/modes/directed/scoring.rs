use common::pri::BasicBlockIndex;

use crate::{
    DirectedEdge, SwitchStep,
    two_level::{IntermediateTargetInfo, IntermediateTargetKind, SearchResultInfo},
};

type Prefix<'a> = &'a [SwitchStep];

type Frontier<'a> = DirectedEdge<'a, SearchResultInfo>;

pub(super) struct Scorer<'t> {
    trace: &'t [SwitchStep],
}

impl<'t> Scorer<'t> {
    pub fn new(trace: &'t [SwitchStep]) -> Self {
        Self { trace }
    }
}

impl<'t> Scorer<'t> {
    #[tracing::instrument(level = "debug", skip_all, ret)]
    pub fn score(&self, frontier: &Frontier<'t>, constraint: (), relation: ()) -> f64 {
        let frontier_score = self.score_trace(frontier);
        let intention_score = self.score_intention(self.trace, frontier.metadata.as_ref());

        let score = frontier_score * intention_score;
        score
    }

    fn score_trace(&self, frontier: &Frontier<'t>) -> f64 {
        1.0
    }

    #[tracing::instrument(level = "debug", skip_all, fields(?inter_target), ret)]
    fn score_intention(&self, context: Prefix, inter_target: &IntermediateTargetInfo) -> f64 {
        // FIXME: These are relatively random values. Do not take them seriously!
        match inter_target.kind {
            IntermediateTargetKind::TargetBody => 50.0,
            IntermediateTargetKind::CallSite => 7.0,
            IntermediateTargetKind::ReturnPoint => 1.0,
        }
    }
}
