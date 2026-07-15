//! Context requirements work as aliases for context traits to guarantee that a
//! certain feature will be available in `RuntimeCallAdder` when its context
//! implement that set of traits.

use super::*;

macro_rules! ctxt_req_trait {
    ($name:ident<'tcx> : $($bounds:tt)+) => {
        pub(in super::super::super) trait $name<'tcx>: $($bounds)+ {}
        impl<'tcx, C> $name<'tcx> for C where C: $($bounds)+ {}
    };
}

ctxt_req_trait!(Basic<'tcx>: BaseContext<'tcx> + BodyProvider<'tcx>);

ctxt_req_trait!(ForInsertion<'tcx>: Basic<'tcx> + InsertionLocationProvider + SourceInfoProvider);

ctxt_req_trait!(ForPlaceRef<'tcx>: ForInsertion<'tcx>);

ctxt_req_trait!(ForOperandRef<'tcx>: ForPlaceRef<'tcx>);

ctxt_req_trait!(ForAssignment<'tcx>: ForInsertion<'tcx> + AssignmentInfoProvider);

ctxt_req_trait!(ForCasting<'tcx>: CastOperandProvider + ForAssignment<'tcx>);

ctxt_req_trait!(
    ForBranching<'tcx>: ForInsertion<'tcx> + BlockOriginalIndexProvider + JumpTargetModifier
);

ctxt_req_trait!(ForAssertion<'tcx>: ForOperandRef<'tcx> + BlockOriginalIndexProvider);

ctxt_req_trait!(
    ForFunctionCalling<'tcx>: ForInsertion<'tcx> + JumpTargetModifier + BlockOriginalIndexProvider
);

ctxt_req_trait!(ForDropping<'tcx>: ForInsertion<'tcx> + BlockOriginalIndexProvider);

ctxt_req_trait!(ForReturning<'tcx>: ForInsertion<'tcx>);

ctxt_req_trait!(ForEntryFunction<'tcx>: ForInsertion<'tcx> + InEntryFunction);

ctxt_req_trait!(
    ForAtomicIntrinsic<'tcx>:
        ForInsertion<'tcx> + AtomicIntrinsicParamsProvider<'tcx> + PointerInfoProvider<'tcx>
);

ctxt_req_trait!(
    ForMemoryIntrinsic<'tcx>:
        ForAssignment<'tcx> + MemoryIntrinsicParamsProvider<'tcx> + PointerInfoProvider<'tcx>
);

ctxt_req_trait!(ForStorageMarking<'tcx>: ForInsertion<'tcx>);
