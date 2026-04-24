/// This module contains the traits and implementations for adding calls to the
/// PRI in MIR bodies.
pub(super) mod context;

use rustc_abi::{FieldIdx, VariantIdx};
use rustc_middle::{
    mir::{BasicBlock, BinOp, Body, CastKind, ConstOperand, Local, Operand, Place, UnOp},
    ty::{Const, GenericArg, Ty, TyCtxt},
};
use rustc_span::{def_id::DefId, source_map::Spanned};

use core::iter;
use std::vec;

use serde::Serialize;

use common::pri::{AssignmentId, AtomicBinaryOp, AtomicOrdering};

use super::{
    decision::rules::{
        AssignmentRules, CallFlowRules, ConstantTypeRules, DropRules, OperandKindRules,
        PlaceInfoRules, PlaceStructureRules, StorageLifetimeMarkerRules, SwitchRules,
    },
    pri_utils::{self, sym::intrinsics::LeafIntrinsicSymbol},
};

use context::AssignmentInfoProvider;

/*
 * Contexts and RuntimeCallAdder.
 * Based on the location and the statement we are going to add runtime calls for,
 * there are some data that are required to be passed to the runtime or used in
 * MIR generation. We place these data in a `Context` and `RuntimeCallAdder`
 * capabilities is determined by this context. For example, if the information
 * for a destination place (left hand side of an assignment) is available in the
 * current context, then `RuntimeCallAdder` will be able to generate basic blocks
 * corresponding to calling the assignment functions in the runtime library.
 */

/*
 * The following traits are meant for definition of features that we expect
 * from `RuntimeCallAdder` for various call adding situations.
 */

/*
 * These wrappers just ensure the semantics for the runtime call adder and
 * prevent interchangeably using them.
 * Note that these types are different from what pri has declared. They are
 * direct aliases for interface clarification but these are separate structures
 * that provide stricter interface rules.
 */
macro_rules! make_local_wrapper {
    ($name:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub struct $name(Local);
        impl $name {
            // Local zero is the return value local. So it can never be acquired by a ref.
            pub const INVALID: $name = $name(Local::ZERO);
        }
        impl From<Local> for $name {
            fn from(value: Local) -> Self {
                Self(value)
            }
        }
        impl From<$name> for Local {
            fn from(value: $name) -> Self {
                assert_ne!(value, $name::INVALID);
                value.0
            }
        }
    };
}
make_local_wrapper!(PlaceRef);
make_local_wrapper!(OperandRef);

pub(crate) trait PlaceReferencer<'tcx> {
    fn reference_place(&mut self, place: &Place<'tcx>) -> PlaceRef;
}

pub(crate) trait OperandReferencer<'tcx> {
    fn reference_operand(&mut self, operand: &Operand<'tcx>) -> OperandRef;
}

pub(crate) trait Assigner<'tcx>: AssignmentInfoProvider<'tcx> {
    type Cast<'a>: CastAssigner<'tcx>
    where
        Self: 'a;

    fn by_use(&mut self, operand: OperandRef);

    fn by_repeat(&mut self, operand: OperandRef, count: &Const<'tcx>);

    fn by_ref(&mut self, place: PlaceRef, is_mutable: bool);

    fn by_thread_local_ref(&mut self, def_id: &DefId);

    fn by_raw_ptr(&mut self, place: PlaceRef, is_mutable: bool);

    fn by_cast(&mut self, operand: OperandRef) -> Self::Cast<'_>;

    fn by_binary_op(&mut self, operator: &BinOp, first: OperandRef, second: OperandRef);

    fn by_unary_op(&mut self, operator: &UnOp, operand: OperandRef);

    fn by_discriminant(&mut self, place: PlaceRef);

    fn by_aggregate_array(&mut self, items: &[OperandRef]);

    fn by_aggregate_tuple(&mut self, fields: &[OperandRef]);

    fn by_aggregate_struct(&mut self, fields: &[OperandRef]);

    fn by_aggregate_enum(&mut self, fields: &[OperandRef], variant: VariantIdx);

    fn by_aggregate_union(&mut self, active_field: FieldIdx, value: OperandRef);

    fn by_aggregate_closure(&mut self, upvars: &[OperandRef]);

    fn by_aggregate_coroutine(&mut self, upvars: &[OperandRef]);

    fn by_aggregate_coroutine_closure(&mut self, upvars: &[OperandRef]);

    fn by_aggregate_raw_ptr(
        &mut self,
        data_ptr: OperandRef,
        metadata: OperandRef,
        is_mutable: bool,
    );

    fn by_shallow_init_box(&mut self, operand: OperandRef, ty: &Ty<'tcx>);

    fn by_wrap_unsafe_binder(&mut self, operand: OperandRef, ty: &Ty<'tcx>);

    // Special case for SetDiscriminant StatementType since it is similar to a regular assignment
    fn its_discriminant_to(&mut self, variant_index: &VariantIdx);

    fn by_some(&mut self);
}

pub(crate) trait CastAssigner<'tcx> {
    fn to_int(&mut self, ty: Ty<'tcx>);

    fn to_float(&mut self, ty: Ty<'tcx>);

    fn through_unsizing(&mut self);

    fn through_fn_ptr_coercion(&mut self);

    fn expose_prov(&mut self);

    fn with_exposed_prov(&mut self, ty: Ty<'tcx>);

    fn to_another_ptr(&mut self, ty: Ty<'tcx>, kind: CastKind);

    fn transmuted(&mut self, ty: Ty<'tcx>);

    fn subtyped(&mut self, ty: Ty<'tcx>);
}

pub(crate) trait StorageMarker: Sized {
    fn mark_live(&mut self, place: impl FnOnce(&mut Self) -> PlaceRef);
    fn mark_dead(&mut self, place: impl FnOnce(&mut Self) -> PlaceRef);
}

#[derive(Clone)]
pub struct SwitchInfo<'tcx> {
    pub(super) node_index: BasicBlock,
    pub(super) original_node_index: Operand<'tcx>,
    pub(super) discr_ty: Ty<'tcx>,
    pub(super) discr: Option<OperandRef>,
}

pub(crate) trait BranchingReferencer<'tcx> {
    fn store_branching_info(&mut self, discr: &Operand<'tcx>) -> SwitchInfo<'tcx>;
}
pub(crate) trait BranchingHandler {
    fn take_case(&mut self, index: usize, value: u128);

    fn take_otherwise<I>(&mut self, non_values: I)
    where
        I: IntoIterator<Item = u128> + ExactSizeIterator,
        I::IntoIter: ExactSizeIterator<Item = u128>;
}

pub(crate) trait FunctionHandler<'tcx> {
    fn before_call_func(
        &mut self,
        func: &Operand<'tcx>,
        arguments: &[Spanned<Operand<'tcx>>],
        no_def: bool,
    );

    fn enter_func(&mut self);

    fn return_from_func(&mut self);

    fn after_call_func(&mut self)
    where
        Self: AssignmentInfoProvider<'tcx>;
}

pub(crate) trait DropHandler<'tcx> {
    fn before_call_drop(&mut self, place: &Place<'tcx>);

    fn after_call_drop(&mut self);
}

pub(crate) trait IntrinsicHandler<'tcx> {
    fn intrinsic_one_to_one_by(
        &mut self,
        intrinsic_func: DefId,
        pri_func: LeafIntrinsicSymbol,
        args: impl Iterator<Item = OperandRef>,
    );
}

pub(crate) trait MemoryIntrinsicHandler<'tcx> {
    fn load(&mut self, is_ptr_aligned: bool);

    fn store(&mut self, val: OperandRef, is_ptr_aligned: bool);

    fn copy(
        &mut self,
        dst_ref: OperandRef,
        dst_value: &Operand<'tcx>,
        count_ref: OperandRef,
        count_value: &Operand<'tcx>,
        is_overlapping: bool,
    );

    fn set(&mut self, val: OperandRef, count_ref: OperandRef, count_value: &Operand<'tcx>);

    fn swap(&mut self, second_ref: OperandRef, second_value: &Operand<'tcx>);
}

pub(crate) trait AtomicIntrinsicHandler<'tcx> {
    fn load(&mut self)
    where
        Self: Assigner<'tcx>;

    fn store(&mut self, val: OperandRef)
    where
        // This is a redundant requirement as it is a unit function with a ptr passed to it.
        // However, it is used for the assignment id.
        Self: Assigner<'tcx>;

    fn exchange(&mut self, val: OperandRef)
    where
        Self: Assigner<'tcx>;

    fn compare_exchange(
        &mut self,
        failure_ordering: AtomicOrdering,
        weak: bool,
        old: OperandRef,
        src: OperandRef,
    ) where
        Self: Assigner<'tcx>;

    fn binary_op(&mut self, operator: AtomicBinaryOp, src: OperandRef)
    where
        Self: Assigner<'tcx>;

    fn fence(&mut self, single_threaded: bool);
}

pub(crate) trait EntryFunctionHandler {
    fn init_runtime_lib(&mut self);

    fn shutdown_runtime_lib(&mut self);
}

pub(crate) trait AssertionHandler<'tcx> {
    fn check_assert(
        &mut self,
        cond: OperandRef,
        expected: bool,
        msg: &rustc_middle::mir::AssertMessage<'tcx>,
    );
}

pub(crate) trait DebugInfoHandler {
    fn debug_info<T: Serialize>(&mut self, info: &T);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InsertionLocation {
    Before(BasicBlock),
    After(BasicBlock),
}

impl InsertionLocation {
    #[inline]
    fn index(&self) -> BasicBlock {
        match self {
            Self::Before(bb) | Self::After(bb) => *bb,
        }
    }
}

pub(crate) struct Config {
    pub place_info_filter: PlaceInfoRules<PlaceStructureRules<bool>, bool>,
    pub operand_info_filter: OperandKindRules<bool, Option<ConstantTypeRules<bool>>>,
    pub assignment_filter: AssignmentRules<Option<bool>>,
    pub storage_lifetime_filter: StorageLifetimeMarkerRules<bool>,
    pub call_flow_filter: CallFlowRules<bool>,
    pub drop_filter: DropRules<bool>,
    pub switch_filter: SwitchRules<bool>,
}

mod implementation;

pub(super) use implementation::RuntimeCallAdder;
pub(super) use implementation::ctxt_reqs as ctxtreqs;
