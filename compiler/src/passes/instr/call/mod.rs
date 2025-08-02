/// This module contains the traits and implementations for adding calls to the
/// PRI in MIR bodies.
pub(super) mod context;

use context::AssignmentInfoProvider;
use rustc_middle::{
    mir::{
        BasicBlock, BinOp, Body, CastKind, ConstOperand, Local, Operand, Place, ProjectionElem,
        Statement, UnOp,
    },
    ty::{Const, GenericArg, Ty, TyCtxt},
};
use rustc_span::{def_id::DefId, source_map::Spanned};
use rustc_target::abi::{FieldIdx, VariantIdx};

use common::{
    log_debug, log_warn,
    pri::{AssignmentId, AtomicBinaryOp, AtomicOrdering},
};
use core::iter;
use serde::Serialize;
use std::vec;

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

    fn by_len(&mut self, place: PlaceRef);

    fn by_cast(&mut self, operand: OperandRef) -> Self::Cast<'_>;

    fn by_binary_op(&mut self, operator: &BinOp, first: OperandRef, second: OperandRef);

    fn by_unary_op(&mut self, operator: &UnOp, operand: OperandRef);

    fn by_nullary_op(&mut self);

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

    // Special case for SetDiscriminant StatementType since it is similar to a regular assignment
    fn its_discriminant_to(&mut self, variant_index: &VariantIdx);
}

pub(crate) trait CastAssigner<'tcx> {
    fn to_int(&mut self, ty: Ty<'tcx>);

    fn to_float(&mut self, ty: Ty<'tcx>);

    fn through_unsizing(&mut self);

    fn through_fn_ptr_coercion(&mut self);

    fn through_sized_dynamization(&mut self, ty: Ty<'tcx>);

    fn expose_prov(&mut self);

    fn with_exposed_prov(&mut self, ty: Ty<'tcx>);

    fn to_another_ptr(&mut self, ty: Ty<'tcx>, kind: CastKind);

    fn transmuted(&mut self, ty: Ty<'tcx>);
}

pub(crate) trait StorageMarker {
    fn mark_dead(&mut self, place: PlaceRef);
}

#[derive(Clone, Copy)]
pub struct SwitchInfo<'tcx> {
    pub(super) node_index: BasicBlock,
    pub(super) discr_ty: Ty<'tcx>,
    pub(super) info_local: Local,
}

pub(crate) trait BranchingReferencer<'tcx> {
    fn store_branching_info(&mut self, discr: &Operand<'tcx>) -> SwitchInfo<'tcx>;
}
pub(crate) trait BranchingHandler {
    fn take_by_value(&mut self, value: u128);

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

pub(crate) struct RuntimeCallAdder<C> {
    context: C,
}

mod implementation {
    use core::{assert_matches::debug_assert_matches, intrinsics::unlikely};
    use std::collections::HashMap;

    use rustc_middle::mir::{self, BasicBlock, BasicBlockData, HasLocalDecls, UnevaluatedConst};
    use rustc_middle::ty::{self as mir_ty, TyKind};

    use delegate::delegate;

    use crate::mir_transform::*;
    use crate::passes::Storage;
    use crate::pri_utils::sym::intrinsics::{
        atomic::LeafAtomicIntrinsicSymbol, memory::LeafMemoryIntrinsicSymbol,
    };
    use crate::pri_utils::{
        FunctionInfo,
        sym::{self, LeafSymbol},
    };
    use crate::utils::mir::TyCtxtExt;

    use self::ctxtreqs::*;
    use self::utils::ty::TyExt;
    use super::{super::MirSourceExt, context::*, *};

    use InsertionLocation::*;
    use utils::*;

    pub(crate) trait MirCallAdder<'tcx> {
        fn make_bb_for_call(
            &mut self,
            func_name: LeafSymbol,
            args: Vec<Operand<'tcx>>,
        ) -> BasicBlockData<'tcx> {
            self.make_bb_for_call_with_ret(func_name, args).0
        }

        fn make_bb_for_call_with_target(
            &mut self,
            func_name: LeafSymbol,
            args: Vec<Operand<'tcx>>,
            target: Option<BasicBlock>,
        ) -> BasicBlockData<'tcx> {
            self.make_bb_for_call_with_all(func_name, iter::empty(), args, target)
                .0
        }

        fn make_bb_for_call_with_ret(
            &mut self,
            func_name: LeafSymbol,
            args: Vec<Operand<'tcx>>,
        ) -> (BasicBlockData<'tcx>, Local) {
            self.make_bb_for_call_with_all(func_name, iter::empty(), args, None)
        }

        fn make_bb_for_call_with_all(
            &mut self,
            func_name: LeafSymbol,
            generic_args: impl IntoIterator<Item = GenericArg<'tcx>>,
            args: Vec<Operand<'tcx>>,
            target: Option<BasicBlock>,
        ) -> (BasicBlockData<'tcx>, Local);

        // Just a bit more semantics.
        fn make_bb_for_helper_call_with_all(
            &mut self,
            func_info: FunctionInfo,
            generic_args: impl IntoIterator<Item = GenericArg<'tcx>>,
            args: Vec<Operand<'tcx>>,
            target: Option<BasicBlock>,
        ) -> (BasicBlockData<'tcx>, Local) {
            self.make_bb_for_call_raw(func_info, generic_args, args, target)
        }

        fn make_bb_for_call_raw(
            &mut self,
            func_info: FunctionInfo,
            generic_args: impl IntoIterator<Item = GenericArg<'tcx>>,
            args: Vec<Operand<'tcx>>,
            target: Option<BasicBlock>,
        ) -> (BasicBlockData<'tcx>, Local);
    }

    pub(crate) trait BlockInserter<'tcx> {
        fn insert_blocks(
            &mut self,
            blocks: impl IntoIterator<Item = BasicBlockData<'tcx>>,
        ) -> Vec<BasicBlock>;

        fn insert_blocks_with_stickiness(
            &mut self,
            blocks: impl IntoIterator<Item = BasicBlockData<'tcx>>,
            sticky: bool,
        ) -> Vec<BasicBlock>;
    }

    impl<'tcx, 'm, 'p, 's> RuntimeCallAdder<DefaultContext<'tcx, 'm, 'p, 's>> {
        pub fn new(
            tcx: TyCtxt<'tcx>,
            modification_unit: &'m mut BodyInstrumentationUnit<'tcx>,
            pri: &'p PriItems,
            storage: &'s mut dyn Storage,
        ) -> Self {
            Self {
                context: DefaultContext::new(tcx, modification_unit, pri, storage),
            }
        }
    }

    /*
     * The following methods are context definers that should be side-effect-free.
     * The reason that self is mutably borrowed is because of the mutability of the
     * context.
     * NOTE: Maybe this is a design mistake. Currently, the only mutable component
     * is the modification unit which may be extracted from the context and stored
     * directly in the RuntimeCallAdder. However, this change should be done when
     * the call adder is quite stable and no substantial change is expected.
     */
    impl<C> RuntimeCallAdder<C> {
        pub fn in_body<'b, 'tcx, 'bd>(
            &'b mut self,
            body: &'bd Body<'tcx>,
            block_orig_index_map: HashMap<BasicBlock, BasicBlock>,
        ) -> RuntimeCallAdder<InBodyContext<'b, 'tcx, 'bd, C>> {
            self.with_context(|base| InBodyContext {
                base,
                body,
                block_orig_index_map,
            })
        }

        pub fn at(
            &mut self,
            location: InsertionLocation,
        ) -> RuntimeCallAdder<AtLocationContext<C>> {
            self.with_context(|base| AtLocationContext { base, location })
        }

        pub fn before(&mut self) -> RuntimeCallAdder<AtLocationContext<C>>
        where
            C: BlockIndexProvider,
        {
            let index = self.context.block_index();
            self.with_context(|base| AtLocationContext {
                base,
                location: Before(index),
            })
        }

        pub fn after(&mut self) -> RuntimeCallAdder<AtLocationContext<C>>
        where
            C: BlockIndexProvider,
        {
            let index = self.context.block_index();
            self.with_context(|base| AtLocationContext {
                base,
                location: After(index),
            })
        }

        pub fn with_source_info<'b>(
            &'b mut self,
            source_info: mir::SourceInfo,
        ) -> RuntimeCallAdder<SourceInfoContext<'b, C>> {
            self.with_context(|base| SourceInfoContext { base, source_info })
        }

        pub fn assign<'b, 'tcx>(
            &'b mut self,
            id: AssignmentId,
            dest_ref: PlaceRef,
            dest_ty: Ty<'tcx>,
        ) -> RuntimeCallAdder<AssignmentContext<'b, 'tcx, C>> {
            self.with_context(|base| AssignmentContext {
                base,
                id,
                dest_ref,
                dest_ty,
            })
        }

        pub fn branch<'tcx, 'b>(
            &'b mut self,
            info: SwitchInfo<'tcx>,
        ) -> RuntimeCallAdder<BranchingContext<'b, 'tcx, C>> {
            self.with_context(|base| BranchingContext {
                base,
                switch_info: info,
            })
        }

        pub fn in_entry_fn(&mut self) -> RuntimeCallAdder<EntryFunctionMarkerContext<C>> {
            self.with_context(|base| EntryFunctionMarkerContext { base })
        }

        pub fn perform_atomic_op<'b, 'tcx>(
            &'b mut self,
            ordering: AtomicOrdering,
            ptr: Option<PointerPackage<'tcx>>,
        ) -> RuntimeCallAdder<AtomicIntrinsicContext<'b, 'tcx, C>> {
            self.with_context(|base| AtomicIntrinsicContext {
                base,
                ordering,
                ptr,
            })
        }

        pub fn perform_memory_op<'b, 'tcx>(
            &'b mut self,
            is_volatile: bool,
            ptr: Option<PointerPackage<'tcx>>,
        ) -> RuntimeCallAdder<MemoryIntrinsicContext<'b, 'tcx, C>> {
            self.with_context(|base| MemoryIntrinsicContext {
                base,
                is_volatile,
                ptr,
            })
        }

        pub fn borrow_from(
            other: &mut RuntimeCallAdder<C>,
        ) -> RuntimeCallAdder<TransparentContext<C>> {
            other.with_context(|base| TransparentContext { base })
        }

        pub fn with_context<'a: 'b, 'b, NC>(
            &'a mut self,
            f: impl FnOnce(&'b mut C) -> NC,
        ) -> RuntimeCallAdder<NC> {
            RuntimeCallAdder {
                context: f(&mut self.context),
            }
        }
    }

    impl<'tcx, C> RuntimeCallAdder<C>
    where
        C: context::BodyProvider<'tcx>,
    {
        fn current_func_id(&self) -> DefId {
            self.context.body().source.def_id()
        }
    }
    impl<'tcx, C> RuntimeCallAdder<C>
    where
        C: context::BodyProvider<'tcx> + context::TyContextProvider<'tcx> + HasLocalDecls<'tcx>,
    {
        fn current_typing_env(&self) -> mir_ty::TypingEnv<'tcx> {
            self.tcx().typing_env_in_body(self.current_func_id())
        }
    }

    impl<'tcx, C> TyContextProvider<'tcx> for RuntimeCallAdder<C>
    where
        C: TyContextProvider<'tcx>,
    {
        delegate! {
            to self.context {
                fn tcx(&self) -> TyCtxt<'tcx>;
            }
        }
    }
    impl<'tcx, C> BodyProvider<'tcx> for RuntimeCallAdder<C>
    where
        C: BodyProvider<'tcx>,
    {
        delegate! {
            to self.context {
                fn body(&self) -> &Body<'tcx>;
            }
        }
    }
    impl<'tcx, C> HasLocalDecls<'tcx> for RuntimeCallAdder<C>
    where
        C: HasLocalDecls<'tcx>,
    {
        delegate! {
            to self.context {
                fn local_decls(&self) -> &rustc_middle::mir::LocalDecls<'tcx>;
            }
        }
    }
    impl<'tcx, C> BodyLocalManager<'tcx> for RuntimeCallAdder<C>
    where
        C: BodyLocalManager<'tcx>,
    {
        delegate! {
            to self.context {
                fn add_local<T>(&mut self, decl_info: T) -> Local
                where
                    T: Into<NewLocalDecl<'tcx>>;
            }
        }
    }
    impl<'tcx, C> PriItemsProvider<'tcx> for RuntimeCallAdder<C>
    where
        C: PriItemsProvider<'tcx>,
    {
        delegate! {
            to self.context {
                fn get_pri_func_info(&self, func_name: LeafSymbol) -> &FunctionInfo;
                fn pri_types(&self) -> &crate::pri_utils::PriTypes;
                fn pri_helper_funcs(&self) -> &crate::pri_utils::PriHelperFunctions;
                fn all_pri_items(&self) -> &std::collections::HashSet<DefId>;
            }
        }
    }
    impl<'tcx, C> SourceInfoProvider for RuntimeCallAdder<C>
    where
        C: SourceInfoProvider,
    {
        delegate! {
            to self.context {
                fn source_info(&self) -> mir::SourceInfo;
            }
        }
    }
    impl<'tcx, C> AssignmentInfoProvider<'tcx> for RuntimeCallAdder<C>
    where
        C: AssignmentInfoProvider<'tcx>,
    {
        delegate! {
            to self.context {
                fn assignment_id(&self) -> AssignmentId;
                fn dest_ref(&self) -> PlaceRef;
                fn dest_ty(&self) -> Ty<'tcx>;
            }
        }
    }
    impl<'tcx, C> StorageProvider for RuntimeCallAdder<C>
    where
        C: StorageProvider,
    {
        delegate! {
            to self.context {
                fn storage(&mut self) -> &mut dyn Storage;
            }
        }
    }

    impl<'tcx, C> MirCallAdder<'tcx> for RuntimeCallAdder<C>
    where
        C: BodyLocalManager<'tcx>
            + TyContextProvider<'tcx>
            + PriItemsProvider<'tcx>
            + SourceInfoProvider,
    {
        fn make_bb_for_call_with_all(
            &mut self,
            func_name: LeafSymbol,
            generic_args: impl IntoIterator<Item = GenericArg<'tcx>>,
            args: Vec<Operand<'tcx>>,
            target: Option<BasicBlock>,
        ) -> (BasicBlockData<'tcx>, Local) {
            self.make_bb_for_call_raw(
                *self.context.get_pri_func_info(func_name),
                generic_args,
                args,
                target,
            )
        }

        fn make_bb_for_call_raw(
            &mut self,
            func_info: FunctionInfo,
            generic_args: impl IntoIterator<Item = GenericArg<'tcx>>,
            args: Vec<Operand<'tcx>>,
            target: Option<BasicBlock>,
        ) -> (BasicBlockData<'tcx>, Local) {
            assert_eq!(func_info.num_inputs(self.tcx()), args.len());
            let generic_args = generic_args.into_iter().collect::<Vec<_>>();
            let result_local = self.context.add_local((
                func_info.ret_ty(self.tcx(), &generic_args),
                self.context.source_info(),
            ));
            (
                self.make_call_bb(
                    func_info.def_id,
                    generic_args,
                    args,
                    Place::from(result_local),
                    target,
                ),
                result_local,
            )
        }
    }
    impl<'tcx, C> RuntimeCallAdder<C>
    where
        C: TyContextProvider<'tcx> + PriItemsProvider<'tcx> + SourceInfoProvider,
    {
        fn make_call_bb(
            &self,
            func_id: DefId,
            generic_args: impl IntoIterator<Item = GenericArg<'tcx>>,
            args: Vec<Operand<'tcx>>,
            destination: Place<'tcx>,
            target: Option<BasicBlock>,
        ) -> BasicBlockData<'tcx> {
            BasicBlockData::new(
                Some(terminator::call(
                    self.context.tcx(),
                    func_id,
                    generic_args,
                    args,
                    destination,
                    target,
                    self.context.source_info(),
                )),
                false,
            )
        }
    }

    impl<'tcx, C> BlockInserter<'tcx> for RuntimeCallAdder<C>
    where
        C: ForInsertion<'tcx>,
    {
        #[inline]
        fn insert_blocks(
            &mut self,
            blocks: impl IntoIterator<Item = BasicBlockData<'tcx>>,
        ) -> Vec<BasicBlock> {
            match self.context.insertion_loc() {
                Before(_) => self.insert_blocks_with_stickiness(blocks, true),
                After(index) => self.context.insert_blocks_after(index, blocks),
            }
        }

        fn insert_blocks_with_stickiness(
            &mut self,
            blocks: impl IntoIterator<Item = BasicBlockData<'tcx>>,
            sticky: bool,
        ) -> Vec<BasicBlock> {
            debug_assert_matches!(
                self.context.insertion_loc(),
                Before(..),
                "Stickiness is only defined for insertions before a block."
            );
            self.context
                .insert_blocks_before(self.context.block_index(), blocks, sticky)
        }
    }

    impl<'tcx, C> PlaceReferencer<'tcx> for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForPlaceRef<'tcx>,
    {
        fn reference_place(&mut self, place: &Place<'tcx>) -> PlaceRef {
            let BlocksAndResult(new_blocks, reference) = self.internal_reference_place(place);
            self.insert_blocks(new_blocks);
            reference.into()
        }
    }
    impl<'tcx, C> RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForPlaceRef<'tcx>,
    {
        fn internal_reference_place(&mut self, place: &Place<'tcx>) -> BlocksAndResult<'tcx> {
            let BlocksAndResult(mut blocks, place_ref) = self.reference_place_local(place.local);

            let tcx = self.tcx();

            // For setting addresses we have to remake a cumulative place up to each projection.
            let mut cum_place = Place::from(place.local);
            let mut cum_ty = cum_place.ty(&self.context, tcx);

            for (_, proj) in place.iter_projections() {
                let added_blocks = self.reference_place_projection(place_ref, proj);
                blocks.extend(added_blocks);

                cum_place = cum_place.project_deeper(&[proj], tcx);
                cum_ty = cum_ty.projection_ty(tcx, proj);

                blocks.extend(self.set_place_type(place_ref, cum_ty.ty));
                blocks.push(self.set_place_addr(place_ref, &cum_place, cum_ty.ty));
            }

            BlocksAndResult(blocks, place_ref)
        }

        fn reference_place_local(&mut self, local: Local) -> BlocksAndResult<'tcx> {
            let kind = self.body().local_kind(local);
            use mir::LocalKind::*;
            let func_name = match kind {
                Temp => sym::ref_place_local,
                Arg => sym::ref_place_argument,
                ReturnPointer => sym::ref_place_return_value,
            };
            let args = match kind {
                ReturnPointer => vec![],
                _ => vec![operand::const_from_uint(self.context.tcx(), local.as_u32())],
            };

            let (block, place_ref) = self.make_bb_for_place_ref_call(func_name, args);
            let mut blocks = vec![block];

            let ty = self.local_decls()[local].ty;
            blocks.extend(self.set_place_type(place_ref, ty));
            blocks.push(self.set_place_addr(place_ref, &local.into(), ty));

            BlocksAndResult(blocks, place_ref)
        }

        fn reference_place_projection<T>(
            &mut self,
            current_ref: Local,
            proj: ProjectionElem<Local, T>,
        ) -> Vec<BasicBlockData<'tcx>> {
            let mut new_blocks = Vec::new();

            let (func_name, additional_args) = match proj {
                ProjectionElem::Deref => (sym::ref_place_deref, vec![]),
                ProjectionElem::Field(index, _) => {
                    (sym::ref_place_field, vec![operand::const_from_uint(
                        self.context.tcx(),
                        u32::from(index),
                    )])
                }
                ProjectionElem::Index(index) => {
                    let BlocksAndResult(additional_blocks, index_ref) =
                        self.internal_reference_place(&Place::from(index));
                    new_blocks.extend(additional_blocks);
                    (sym::ref_place_index, vec![operand::copy_for_local(
                        index_ref,
                    )])
                }
                ProjectionElem::ConstantIndex {
                    offset,
                    min_length,
                    from_end,
                } => (sym::ref_place_constant_index, vec![
                    operand::const_from_uint(self.context.tcx(), offset),
                    operand::const_from_uint(self.context.tcx(), min_length),
                    operand::const_from_bool(self.context.tcx(), from_end),
                ]),
                ProjectionElem::Subslice { from, to, from_end } => (sym::ref_place_subslice, vec![
                    operand::const_from_uint(self.context.tcx(), from),
                    operand::const_from_uint(self.context.tcx(), to),
                    operand::const_from_bool(self.context.tcx(), from_end),
                ]),
                ProjectionElem::Downcast(_, index) => {
                    (sym::ref_place_downcast, vec![operand::const_from_uint(
                        self.context.tcx(),
                        u32::from(index),
                    )])
                }
                ProjectionElem::OpaqueCast(_) => (sym::ref_place_opaque_cast, vec![]),
                ProjectionElem::Subtype(_) => (sym::ref_place_subtype, vec![]),
            };

            new_blocks.push(
                self.make_bb_for_place_ref_call(
                    func_name,
                    [vec![operand::copy_for_local(current_ref)], additional_args].concat(),
                )
                .0, // The result is unit.
            );
            new_blocks
        }

        fn make_bb_for_place_ref_call(
            &mut self,
            func_name: LeafSymbol,
            args: Vec<Operand<'tcx>>,
        ) -> (BasicBlockData<'tcx>, Local) {
            self.make_bb_for_call_with_ret(func_name, args)
        }

        fn set_place_addr(
            &mut self,
            place_ref: Local,
            place: &Place<'tcx>,
            place_ty: Ty<'tcx>,
        ) -> BasicBlockData<'tcx> {
            let (stmt, ptr_local) = ptr_to_place(self.tcx(), &mut self.context, place, place_ty);
            let (mut block, _) = {
                self.make_bb_for_helper_call_with_all(
                    self.context.pri_helper_funcs().set_place_address_typed,
                    vec![place_ty.into()],
                    vec![
                        operand::copy_for_local(place_ref),
                        operand::move_for_local(ptr_local),
                    ],
                    None,
                )
            };
            block.statements.push(stmt);
            block
        }

        fn set_place_size(
            &mut self,
            place_ref: Local,
            place_ty: Ty<'tcx>,
        ) -> Vec<BasicBlockData<'tcx>> {
            if unlikely(!place_ty.is_sized(self.tcx(), self.current_typing_env())) {
                log_warn!("Encountered unsized type. Skipping size setting.");
                return vec![BasicBlockData::new(Some(terminator::goto(None)), false)];
            }

            let (get_call_block, size_local) = self.make_bb_for_helper_call_with_all(
                self.context.pri_helper_funcs().size_of,
                vec![place_ty.into()],
                Vec::default(),
                None,
            );

            let set_call_block = self.make_bb_for_call(sym::set_place_size, vec![
                operand::copy_for_local(place_ref),
                operand::move_for_local(size_local),
            ]);

            vec![get_call_block, set_call_block]
        }

        fn set_place_type(&mut self, place_ref: Local, ty: Ty<'tcx>) -> Vec<BasicBlockData<'tcx>> {
            let mut blocks = vec![];

            let tcx = self.context.tcx();
            // FIXME: To be removed when type information passing is complete.
            if let Some((func_name, additional_args)) = if ty.is_bool() {
                Some((sym::set_place_type_bool, vec![]))
            } else if ty.is_char() {
                Some((sym::set_place_type_char, vec![]))
            } else if ty.is_integral() {
                Some((sym::set_place_type_int, vec![
                    operand::const_from_uint(tcx, ty.primitive_size(tcx).bits()),
                    operand::const_from_bool(tcx, ty.is_signed()),
                ]))
            } else if ty.is_floating_point() {
                let (e_bits, s_bits) = ty::ebit_sbit_size(ty);
                Some((sym::set_place_type_float, vec![
                    operand::const_from_uint(tcx, e_bits),
                    operand::const_from_uint(tcx, s_bits),
                ]))
            } else {
                None
            } {
                blocks.push(self.make_bb_for_call(
                    func_name,
                    [vec![operand::copy_for_local(place_ref)], additional_args].concat(),
                ));
            }

            let id_local = {
                let (block, id_local) = self.make_type_id_of_bb(ty);
                blocks.push(block);
                id_local
            };

            blocks.push(self.make_bb_for_call(sym::set_place_type_id, vec![
                operand::copy_for_local(place_ref),
                operand::move_for_local(id_local),
            ]));

            blocks
        }
    }

    impl<'tcx, C> OperandReferencer<'tcx> for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForOperandRef<'tcx>,
    {
        fn reference_operand(&mut self, operand: &Operand<'tcx>) -> OperandRef {
            let BlocksAndResult(new_blocks, reference) = self.internal_reference_operand(operand);
            self.insert_blocks(new_blocks);
            reference.into()
        }
    }
    #[allow(clippy::borrowed_box)]
    impl<'tcx, C> RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: Basic<'tcx> + SourceInfoProvider,
    {
        fn internal_reference_operand(&mut self, operand: &Operand<'tcx>) -> BlocksAndResult<'tcx>
        where
            C: ForOperandRef<'tcx>,
        {
            match operand {
                Operand::Copy(place) => self.internal_reference_place_operand(place, true),
                Operand::Move(place) => self.internal_reference_place_operand(place, false),
                Operand::Constant(constant) => self.internal_reference_const_operand(constant),
            }
        }

        fn internal_reference_place_operand(
            &mut self,
            place: &Place<'tcx>,
            is_copy: bool,
        ) -> BlocksAndResult<'tcx>
        where
            C: ForPlaceRef<'tcx>,
        {
            let BlocksAndResult(mut additional_blocks, place_ref) =
                self.internal_reference_place(place);

            let tcx = self.tcx();
            let ty = place.ty(&self.context, tcx).ty;
            if ty.is_adt() || ty.is_trivially_tuple() || ty.is_array() || !ty.is_known_rigid() {
                additional_blocks.extend(self.set_place_size(place_ref, ty));
            }

            let func_name = if is_copy {
                sym::ref_operand_copy
            } else {
                sym::ref_operand_move
            };

            BlocksAndResult::from(
                self.make_bb_for_operand_ref_call(func_name, vec![operand::copy_for_local(
                    place_ref,
                )]),
            )
            .prepend(additional_blocks)
        }

        fn internal_reference_const_operand(
            &mut self,
            constant: &Box<ConstOperand<'tcx>>,
        ) -> BlocksAndResult<'tcx>
        where
            C: ForOperandRef<'tcx>,
        {
            /* NOTE: Although there might be some ways to obtain/evaluate the values, we prefer to use
             * the constant as it is, and rely on high-level conversions to support all types of
             * constants in an abstract fashion. */
            let ty = constant.ty();
            let tcx = self.tcx();
            if ty.is_primitive() {
                self.internal_reference_const_primitive(constant)
            } else if cfg!(feature = "abs_concrete") {
                self.internal_reference_const_some()
            }
            // &str
            else if ty.peel_refs().is_str() {
                self.internal_reference_const_operand_directly(sym::ref_operand_const_str, constant)
            }
            // &[u8]
            else if Self::is_u8_slice_ref(tcx, ty) {
                self.internal_reference_const_operand_directly(
                    sym::ref_operand_const_byte_str,
                    constant,
                )
            }
            // &[u8; N]
            else if ty.peel_refs().is_array()
                && ty.peel_refs().sequence_element_type(tcx) == tcx.types.u8
            {
                self.internal_reference_byte_str_const_operand(constant)
            } else if let TyKind::FnDef(..) = ty.kind() {
                self.internal_reference_func_def_const_operand(constant)
            }
            // NOTE: Check this after all other ZSTs that you want to distinguish.
            else if ty::size_of(tcx, ty, self.current_typing_env()) == rustc_abi::Size::ZERO {
                self.make_bb_for_operand_ref_call(sym::ref_operand_const_zst, Default::default())
                    .into()
            } else if let Some(c) = operand::const_try_as_unevaluated(constant) {
                self.internal_reference_unevaluated_const_operand(&c)
            } else if let Some(def_id) = Self::try_as_immut_static(tcx, constant) {
                self.internal_reference_static_ref_const_operand(def_id, ty)
            } else {
                unimplemented!(
                    "Encountered unknown constant {:?} with type {:?}",
                    constant.const_,
                    ty
                )
            }
        }

        fn internal_reference_const_primitive(
            &mut self,
            constant: &Box<ConstOperand<'tcx>>,
        ) -> BlocksAndResult<'tcx> {
            let ty = constant.ty();
            debug_assert!(ty.is_primitive(), "Expected primitive type, found {:?}", ty);

            if ty.is_bool() {
                self.internal_reference_const_operand_directly(
                    sym::ref_operand_const_bool,
                    constant,
                )
            } else if ty.is_char() {
                self.internal_reference_const_operand_directly(
                    sym::ref_operand_const_char,
                    constant,
                )
            } else if ty.is_integral() {
                self.internal_reference_int_const_operand(constant)
            } else if ty.is_floating_point() {
                self.internal_reference_float_const_operand(constant)
            } else {
                unreachable!()
            }
        }

        fn internal_reference_const_some(&mut self) -> BlocksAndResult<'tcx> {
            self.make_bb_for_some_const_ref_call().into()
        }

        fn internal_reference_const_operand_directly(
            &mut self,
            func_name: LeafSymbol,
            constant: &Box<ConstOperand<'tcx>>,
        ) -> BlocksAndResult<'tcx> {
            self.make_bb_for_operand_ref_call(func_name, vec![operand::const_from_existing(
                constant,
            )])
            .into()
        }

        fn internal_reference_int_const_operand(
            &mut self,
            constant: &Box<ConstOperand<'tcx>>,
        ) -> BlocksAndResult<'tcx> {
            let ty = constant.ty();
            debug_assert!(ty.is_integral());

            let tcx = self.tcx();
            let (bit_rep_local, additional_stmts) =
                utils::cast_int_to_bit_rep(tcx, &mut self.context, constant);
            let (mut block, result) =
                self.make_bb_for_operand_ref_call(sym::ref_operand_const_int, vec![
                    operand::move_for_local(bit_rep_local),
                    operand::const_from_uint(tcx, ty.primitive_size(tcx).bits()),
                    operand::const_from_bool(tcx, ty.is_signed()),
                ]);
            block.statements.extend(additional_stmts);
            (block, result).into()
        }

        fn internal_reference_float_const_operand(
            &mut self,
            constant: &Box<ConstOperand<'tcx>>,
        ) -> BlocksAndResult<'tcx> {
            let ty = constant.ty();
            debug_assert!(ty.is_floating_point());

            let tcx = self.tcx();
            let (bit_rep_local, conversion_block) =
                utils::cast_float_to_bit_rep(tcx, &mut self.context, constant);
            let (e_bits, s_bits) = ty::ebit_sbit_size(ty);
            let block_pair = self.make_bb_for_operand_ref_call(sym::ref_operand_const_float, vec![
                operand::move_for_local(bit_rep_local),
                operand::const_from_uint(tcx, e_bits),
                operand::const_from_uint(tcx, s_bits),
            ]);
            BlocksAndResult::from(block_pair).prepend([conversion_block])
        }

        fn internal_reference_byte_str_const_operand(
            &mut self,
            constant: &Box<ConstOperand<'tcx>>,
        ) -> BlocksAndResult<'tcx> {
            let tcx = self.tcx();
            let ty = constant.ty();
            debug_assert!(ty.is_ref() && ty.peel_refs().sequence_element_type(tcx) == tcx.types.u8,);

            let (slice_local, slice_assignment) = cast_array_ref_to_slice(
                tcx,
                &mut self.context,
                operand::const_from_existing(constant),
                None,
            );
            let mut block_pair = self
                .make_bb_for_operand_ref_call(sym::ref_operand_const_byte_str, vec![
                    operand::move_for_local(slice_local),
                ]);
            block_pair.0.statements.insert(0, slice_assignment);
            BlocksAndResult::from(block_pair)
        }

        fn internal_reference_func_def_const_operand(
            &mut self,
            _constant: &Box<ConstOperand<'tcx>>,
        ) -> BlocksAndResult<'tcx> {
            panic!("Function definition constant is not supported by this configuration.")
        }

        fn internal_reference_unevaluated_const_operand(
            &mut self,
            _constant: &UnevaluatedConst,
        ) -> BlocksAndResult<'tcx>
        where
            C: ForOperandRef<'tcx>,
        {
            panic!("Unevaluated constant is not supported by this configuration.")
        }

        fn internal_reference_static_ref_const_operand(
            &mut self,
            _def_id: DefId,
            _ty: Ty<'tcx>,
        ) -> BlocksAndResult<'tcx>
        where
            C: ForOperandRef<'tcx>,
        {
            panic!("Static reference constant is not supported by this configuration.")
        }

        fn make_bb_for_some_const_ref_call(&mut self) -> (BasicBlockData<'tcx>, Local) {
            self.make_bb_for_operand_ref_call(sym::ref_operand_const_some, Vec::default())
        }

        fn make_bb_for_operand_ref_call(
            &mut self,
            func_name: LeafSymbol,
            args: Vec<Operand<'tcx>>,
        ) -> (BasicBlockData<'tcx>, Local) {
            self.make_bb_for_call_with_ret(func_name, args)
        }

        #[inline]
        fn is_u8_slice_ref(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> bool {
            // This is just for mitigating the bug in rustfmt. Track: rustfmt#5863
            if let TyKind::Ref(_, ty, _) = ty.kind() {
                if let TyKind::Slice(ty) = ty.kind() {
                    return ty == &tcx.types.u8;
                }
            }

            false
        }

        #[inline]
        fn try_as_immut_static(
            tcx: TyCtxt<'tcx>,
            constant: &Box<ConstOperand<'tcx>>,
        ) -> Option<DefId> {
            /* Immutable statics are accessed by a constant reference which points to a statically
             * allocated program memory block. If the static item's type is T then the constant is
             * of type &T. */
            constant
                .ty()
                .ref_mutability()
                .is_some_and(|m| m.is_not())
                .then(|| constant.check_static_ptr(tcx))
                .flatten()
        }
    }

    impl<'tcx, C> Assigner<'tcx> for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForAssignment<'tcx>,
    {
        type Cast<'b>
            = RuntimeCallAdder<CastAssignmentContext<'b, C>>
        where
            CastAssignmentContext<'b, C>: ForCasting<'tcx> + 'b;

        fn by_use(&mut self, operand: OperandRef) {
            self.add_bb_for_assign_call(sym::assign_use, vec![operand::copy_for_local(
                operand.into(),
            )])
        }

        fn by_repeat(&mut self, operand: OperandRef, count: &Const<'tcx>) {
            self.add_bb_for_assign_call(sym::assign_repeat, vec![
                operand::copy_for_local(operand.into()),
                #[allow(clippy::clone_on_copy)]
                operand::const_from_existing_ty_const(self.tcx().types.usize, count.clone()),
            ])
        }

        fn by_ref(&mut self, place: PlaceRef, is_mutable: bool) {
            self.add_bb_for_assign_call(sym::assign_ref, vec![
                operand::copy_for_local(place.into()),
                operand::const_from_bool(self.context.tcx(), is_mutable),
            ])
        }

        fn by_thread_local_ref(&mut self, _def_id: &DefId) {
            if cfg!(feature = "abs_concrete") {
                self.to_some_concrete()
            } else {
                self.add_bb_for_assign_call(sym::assign_thread_local_ref, vec![])
            }
        }

        fn by_raw_ptr(&mut self, place: PlaceRef, is_mutable: bool) {
            self.add_bb_for_assign_call(sym::assign_raw_ptr_of, vec![
                operand::copy_for_local(place.into()),
                operand::const_from_bool(self.context.tcx(), is_mutable),
            ])
        }

        fn by_len(&mut self, place: PlaceRef) {
            self.add_bb_for_assign_call(sym::assign_len, vec![operand::copy_for_local(
                place.into(),
            )])
        }

        fn by_cast(&mut self, operand: OperandRef) -> Self::Cast<'_> {
            RuntimeCallAdder {
                context: CastAssignmentContext {
                    base: &mut self.context,
                    operand_ref: operand,
                },
            }
        }

        fn by_binary_op(&mut self, operator: &BinOp, first: OperandRef, second: OperandRef) {
            let tcx = self.tcx();
            let operator = convert_mir_binop_to_pri(operator);
            let operator_local = {
                let (block, local) = self.make_bb_for_helper_call_with_all(
                    self.context.pri_helper_funcs().const_binary_op_of,
                    vec![],
                    vec![operand::const_from_uint(tcx, operator.as_u8())],
                    Default::default(),
                );
                self.insert_blocks([block]);
                local
            };

            self.add_bb_for_assign_call(sym::assign_binary_op, vec![
                operand::move_for_local(operator_local),
                operand::copy_for_local(first.into()),
                operand::copy_for_local(second.into()),
            ])
        }

        fn by_unary_op(&mut self, operator: &UnOp, operand: OperandRef) {
            let tcx = self.tcx();
            let operator = convert_mir_unop_to_pri(operator);
            let operator_local = {
                let (block, local) = self.make_bb_for_helper_call_with_all(
                    self.context.pri_helper_funcs().const_unary_op_of,
                    vec![],
                    vec![operand::const_from_uint(tcx, operator.as_u8())],
                    Default::default(),
                );
                self.insert_blocks([block]);
                local
            };

            self.add_bb_for_assign_call(sym::assign_unary_op, vec![
                operand::move_for_local(operator_local),
                operand::copy_for_local(operand.into()),
            ])
        }

        fn by_nullary_op(&mut self) {
            if cfg!(feature = "abs_concrete") {
                self.to_some_concrete()
            } else {
                unimplemented!()
            }
        }

        fn by_discriminant(&mut self, place: PlaceRef) {
            self.add_bb_for_assign_call(sym::assign_discriminant, vec![operand::copy_for_local(
                place.into(),
            )])
        }

        fn by_aggregate_array(&mut self, items: &[OperandRef]) {
            self.add_bb_for_aggregate_assign_call(sym::assign_aggregate_array, items, vec![])
        }

        fn by_aggregate_tuple(&mut self, fields: &[OperandRef]) {
            self.add_bb_for_adt_assign_call(sym::assign_aggregate_tuple, fields, vec![])
        }

        fn by_aggregate_struct(&mut self, fields: &[OperandRef]) {
            self.add_bb_for_adt_assign_call(sym::assign_aggregate_struct, fields, vec![])
        }

        fn by_aggregate_enum(&mut self, fields: &[OperandRef], variant: VariantIdx) {
            self.add_bb_for_adt_assign_call(sym::assign_aggregate_enum, fields, vec![
                operand::const_from_uint(self.context.tcx(), variant.as_u32()),
            ])
        }

        fn by_aggregate_union(&mut self, active_field: FieldIdx, value: OperandRef) {
            self.add_bb_for_assign_call_with_statements(
                sym::assign_aggregate_union,
                vec![
                    operand::const_from_uint(self.context.tcx(), active_field.as_u32()),
                    operand::copy_for_local(value.into()),
                ],
                vec![],
            )
        }

        fn by_aggregate_closure(&mut self, upvars: &[OperandRef]) {
            self.add_bb_for_aggregate_assign_call(sym::assign_aggregate_closure, upvars, vec![])
        }

        fn by_aggregate_coroutine(&mut self, upvars: &[OperandRef]) {
            self.add_bb_for_aggregate_assign_call(sym::assign_aggregate_coroutine, upvars, vec![])
        }

        fn by_aggregate_coroutine_closure(&mut self, upvars: &[OperandRef]) {
            self.add_bb_for_aggregate_assign_call(
                sym::assign_aggregate_coroutine_closure,
                upvars,
                vec![],
            )
        }

        fn by_aggregate_raw_ptr(
            &mut self,
            data_ptr: OperandRef,
            metadata: OperandRef,
            is_mutable: bool,
        ) {
            self.add_bb_for_assign_call_with_statements(
                sym::assign_aggregate_raw_ptr,
                vec![
                    operand::move_for_local(data_ptr.into()),
                    operand::move_for_local(metadata.into()),
                    operand::const_from_bool(self.tcx(), is_mutable),
                ],
                vec![],
            )
        }

        fn by_shallow_init_box(&mut self, operand: OperandRef, ty: &Ty<'tcx>) {
            let id_local = {
                let (block, id_local) = self.make_type_id_of_bb(*ty);
                self.insert_blocks([block]);
                id_local
            };
            self.add_bb_for_assign_call(sym::assign_shallow_init_box, vec![
                operand::copy_for_local(operand.into()),
                operand::move_for_local(id_local),
            ]);
        }

        fn its_discriminant_to(&mut self, variant_index: &VariantIdx) {
            self.add_bb_for_assign_call(sym::set_discriminant, vec![operand::const_from_uint(
                self.context.tcx(),
                variant_index.as_u32(),
            )])
        }
    }

    impl<'tcx, C> RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForAssignment<'tcx>,
    {
        fn add_bb_for_aggregate_assign_call(
            &mut self,
            func_name: LeafSymbol,
            elements: &[OperandRef],
            additional_args: Vec<Operand<'tcx>>,
        ) {
            let (elements_local, additional_stmts) = self.make_slice_for_adt_elements(elements);

            self.add_bb_for_assign_call_with_statements(
                func_name,
                [
                    vec![operand::move_for_local(elements_local)],
                    additional_args,
                ]
                .concat(),
                additional_stmts.to_vec(),
            )
        }

        fn add_bb_for_adt_assign_call(
            &mut self,
            func_name: LeafSymbol,
            fields: &[OperandRef],
            additional_args: Vec<Operand<'tcx>>,
        ) {
            let mut args = Vec::new();
            let mut additional_stmts = Vec::new();

            let (fields_local, fields_stmts) = self.make_slice_for_adt_elements(fields);
            args.push(operand::move_for_local(fields_local));
            additional_stmts.extend(fields_stmts);

            args.extend(additional_args);

            self.add_bb_for_assign_call_with_statements(func_name, args, additional_stmts)
        }

        fn make_slice_for_adt_elements(
            &mut self,
            elements: &[OperandRef],
        ) -> (Local, [Statement<'tcx>; 3]) {
            let operand_ref_ty = self.context.pri_types().operand_ref(self.tcx());
            let (items_local, additional_stmts) = prepare_operand_for_slice(
                self.context.tcx(),
                &mut self.context,
                operand_ref_ty,
                elements
                    .iter()
                    .map(|i| operand::move_for_local((*i).into()))
                    .collect(),
            );
            (items_local, additional_stmts)
        }

        fn add_bb_for_assign_call(&mut self, func_name: LeafSymbol, args: Vec<Operand<'tcx>>) {
            self.add_bb_for_assign_call_with_statements(func_name, args, vec![])
        }

        fn add_bb_for_assign_call_with_statements(
            &mut self,
            func_name: LeafSymbol,
            args: Vec<Operand<'tcx>>,
            statements: Vec<Statement<'tcx>>,
        ) {
            let mut block = self.make_bb_for_assign_call(func_name, args);
            block.statements.extend(statements);
            self.insert_blocks([block]);
        }

        fn make_bb_for_assign_call(
            &mut self,
            func_name: LeafSymbol,
            args: Vec<Operand<'tcx>>,
        ) -> BasicBlockData<'tcx> {
            self.make_bb_for_call(
                func_name,
                [
                    vec![
                        operand::const_from_uint(self.tcx(), self.context.assignment_id()),
                        operand::copy_for_local(self.context.dest_ref().into()),
                    ],
                    args,
                ]
                .concat(),
            )
        }
    }

    impl<'tcx, C> RuntimeCallAdder<C>
    where
        Self: Assigner<'tcx>,
        C: ForInsertion<'tcx>,
    {
        fn to_some_concrete(&mut self) {
            let BlocksAndResult(blocks, operand_ref) = self.internal_reference_const_some();
            self.insert_blocks(blocks);
            self.by_use(operand_ref.into());
        }
    }

    impl<'tcx, C> CastAssigner<'tcx> for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForCasting<'tcx>,
    {
        fn to_int(&mut self, ty: Ty<'tcx>) {
            if ty.is_char() {
                self.add_bb_for_cast_assign_call(sym::assign_cast_char)
            } else {
                assert!(ty.is_integral());

                let tcx = self.context.tcx();
                let is_signed = ty.is_signed();
                let bits = ty.primitive_size(tcx).bits();

                self.add_bb_for_cast_assign_call_with_args(sym::assign_cast_integer, vec![
                    operand::const_from_uint(tcx, bits),
                    operand::const_from_bool(tcx, is_signed),
                ])
            }
        }

        fn to_float(&mut self, ty: Ty<'tcx>) {
            let (e_bits, s_bits) = ty::ebit_sbit_size(ty);
            self.add_bb_for_cast_assign_call_with_args(sym::assign_cast_float, vec![
                operand::const_from_uint(self.context.tcx(), e_bits),
                operand::const_from_uint(self.context.tcx(), s_bits),
            ])
        }

        fn through_unsizing(&mut self) {
            self.add_bb_for_cast_assign_call(sym::assign_cast_unsize)
        }

        fn through_fn_ptr_coercion(&mut self) {
            if cfg!(feature = "abs_concrete") {
                // Effective only at compile time, no operational effect.
                self.by_use(self.context.operand_ref())
            } else {
                unimplemented!("Function pointer coercion is not supported in this configuration.")
            }
        }

        fn through_sized_dynamization(&mut self, _ty: Ty<'tcx>) {
            self.add_bb_for_cast_assign_call(sym::assign_cast_sized_dyn);
        }

        fn expose_prov(&mut self) {
            self.add_bb_for_cast_assign_call(sym::assign_cast_expose_prov);
        }

        fn with_exposed_prov(&mut self, ty: Ty<'tcx>) {
            self.add_bb_for_pointer_cast_assign_call(ty, sym::assign_cast_with_exposed_prov);
        }

        fn to_another_ptr(&mut self, ty: Ty<'tcx>, kind: CastKind) {
            use CastKind::*;
            use rustc_middle::ty::adjustment::PointerCoercion::*;
            debug_assert_matches!(
                kind,
                PtrToPtr | FnPtrToPtr | PointerCoercion(MutToConstPointer | ArrayToPointer, _)
            );
            /* NOTE: Currently, we do not distinguish between different pointer casts.
             * This is because they all keep the data untouched and are just about
             * semantics. We can add support for them later if interested. */
            self.add_bb_for_pointer_cast_assign_call(ty, sym::assign_cast_to_another_ptr);
        }

        fn transmuted(&mut self, ty: Ty<'tcx>) {
            let id_local = {
                let (block, id_local) = self.make_type_id_of_bb(ty);
                self.insert_blocks([block]);
                id_local
            };
            self.add_bb_for_cast_assign_call_with_args(sym::assign_cast_transmute, vec![
                operand::move_for_local(id_local),
            ])
        }
    }

    impl<'tcx, C> RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: CastOperandProvider + ForAssignment<'tcx>,
    {
        fn add_bb_for_cast_assign_call(&mut self, func_name: LeafSymbol) {
            self.add_bb_for_cast_assign_call_with_args(func_name, vec![])
        }

        fn add_bb_for_cast_assign_call_with_args(
            &mut self,
            func_name: LeafSymbol,
            args: Vec<Operand<'tcx>>,
        ) {
            self.add_bb_for_assign_call(
                func_name,
                [
                    vec![operand::copy_for_local(self.context.operand_ref().into())],
                    args,
                ]
                .concat(),
            )
        }

        fn add_bb_for_pointer_cast_assign_call(&mut self, ty: Ty<'tcx>, func_name: LeafSymbol) {
            let id_local: Local = {
                let (block, id_local) = self.make_type_id_of_bb(ty);
                self.insert_blocks([block]);
                id_local
            };
            self.add_bb_for_cast_assign_call_with_args(func_name, vec![operand::move_for_local(
                id_local,
            )]);
        }
    }

    impl<'tcx, C> StorageMarker for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForInsertion<'tcx>,
    {
        fn mark_dead(&mut self, place: PlaceRef) {
            debug_assert_matches!(
                self.context.insertion_loc(),
                Before(..),
                "Marking storage as dead after it takes place is not expected."
            );
            let block =
                self.make_bb_for_call(sym::mark_storage_dead, vec![operand::move_for_local(
                    place.into(),
                )]);
            self.insert_blocks([block]);
        }
    }

    impl<'tcx, C> BranchingReferencer<'tcx> for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx> + OperandReferencer<'tcx>,
        C: ForBranching<'tcx>,
    {
        fn store_branching_info(&mut self, discr: &Operand<'tcx>) -> SwitchInfo<'tcx> {
            let discr_ref = self.reference_operand(discr);

            let (info_block, info_local) = self.make_bb_for_helper_call_with_all(
                self.context.pri_helper_funcs().switch_info,
                [],
                vec![
                    self.original_bb_index_as_arg(),
                    operand::move_for_local(discr_ref.into()),
                ],
                Default::default(),
            );
            self.insert_blocks([info_block]);
            SwitchInfo {
                node_index: self.context.block_index(),
                discr_ty: discr.ty(self.context.local_decls(), self.tcx()),
                info_local,
            }
        }
    }

    impl<'tcx, C> RuntimeCallAdder<C>
    where
        Self: BodyProvider<'tcx>,
        C: BaseContext<'tcx> + BlockIndexProvider + BlockOriginalIndexProvider,
    {
        fn original_bb_index_as_arg(&self) -> Operand<'tcx> {
            let tcx = self.tcx();
            let current = self.context.block_index();
            let bb_index = self
                .context
                .block_original_index(current)
                .unwrap_or_else(|| {
                    panic!(
                        "Original index is not available for {:?}: {:?}",
                        current,
                        self.body().basic_blocks.get(current),
                    )
                });
            operand::const_from_uint(tcx, bb_index.as_u32())
        }
    }

    impl<'tcx, C> BranchingHandler for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: SwitchInfoProvider<'tcx> + ForBranching<'tcx>,
    {
        fn take_by_value(&mut self, value: u128) {
            let tcx = self.context.tcx();
            let switch_info = self.context.switch_info();
            let discr_ty = switch_info.discr_ty;
            let (func_name, additional_args) = if discr_ty.is_bool() {
                const FALSE_SWITCH_VALUE: u128 = 0;
                if value == FALSE_SWITCH_VALUE {
                    (sym::take_branch_false, vec![])
                } else {
                    unreachable!(
                        "SwitchInts for booleans are expected to provide only the value 0 (false)."
                    )
                }
            } else if discr_ty.is_integral() {
                // NOTE: Discriminants are integers.
                (sym::take_branch_int, vec![
                    operand::const_from_uint(tcx, value),
                    operand::const_from_uint(tcx, discr_ty.primitive_size(tcx).bits()),
                    operand::const_from_bool(tcx, discr_ty.is_signed()),
                ])
            } else if discr_ty.is_char() {
                (sym::take_branch_char, vec![operand::const_from_char(
                    tcx,
                    char::from_u32(value.try_into().unwrap()).unwrap(),
                )])
            } else {
                unreachable!(
                    "Branching node discriminant is supposed to be either bool, int, char, or enum discriminant."
                )
            };

            let block = self.make_bb_for_call_with_target(
                func_name,
                [
                    vec![operand::move_for_local(switch_info.info_local)],
                    additional_args,
                ]
                .concat(),
                Some(self.context.block_index()),
            );
            let new_block_index = self.insert_blocks_with_stickiness([block], false)[0];
            self.context.modify_jump_target_where(
                switch_info.node_index,
                self.context.block_index(),
                new_block_index,
                JumpModificationConstraint::SwitchValue(value),
            );
        }

        fn take_otherwise<I>(&mut self, non_values: I)
        where
            I: IntoIterator<Item = u128>,
            I::IntoIter: ExactSizeIterator<Item = u128>,
        {
            let tcx = self.context.tcx();
            let switch_info = self.context.switch_info();
            let discr_ty = switch_info.discr_ty;
            let mut non_values = non_values.into_iter();
            let (additional_stmts, func_name, additional_args) = if discr_ty.is_bool() {
                debug_assert!(non_values.len() == 1);
                debug_assert!(non_values.next().unwrap() == 0);

                (vec![], sym::take_branch_ow_bool, vec![])
            } else {
                let (func_name, value_ty, non_values, additional_args) = if discr_ty.is_integral() {
                    (
                        sym::take_branch_ow_int,
                        tcx.types.u128,
                        non_values
                            .map(|nv: u128| operand::const_from_uint(tcx, nv))
                            .collect(),
                        vec![
                            operand::const_from_uint(tcx, discr_ty.primitive_size(tcx).bits()),
                            operand::const_from_bool(tcx, discr_ty.is_signed()),
                        ],
                    )
                } else if discr_ty.is_char() {
                    (
                        sym::take_branch_ow_char,
                        tcx.types.char,
                        non_values
                            .map(|nv: u128| {
                                operand::const_from_char(
                                    tcx,
                                    char::from_u32(nv.try_into().unwrap()).unwrap(),
                                )
                            })
                            .collect(),
                        vec![],
                    )
                } else {
                    unreachable!(
                        "Branching node discriminant is supposed to be either bool, int, char, or enum discriminant."
                    )
                };

                let (non_values_local, assign_stmts) =
                    prepare_operand_for_slice(tcx, &mut self.context, value_ty, non_values);
                (
                    assign_stmts.to_vec(),
                    func_name,
                    [
                        vec![operand::move_for_local(non_values_local)],
                        additional_args,
                    ]
                    .concat(),
                )
            };

            let mut block = self.make_bb_for_call_with_target(
                func_name,
                [
                    vec![operand::move_for_local(switch_info.info_local)],
                    additional_args,
                ]
                .concat(),
                Some(self.context.block_index()),
            );
            block.statements.extend(additional_stmts);
            let new_block_index = self.insert_blocks_with_stickiness([block], false)[0];
            self.context.modify_jump_target_where(
                switch_info.node_index,
                self.context.block_index(),
                new_block_index,
                JumpModificationConstraint::SwitchOtherwise,
            );
        }
    }

    impl<'tcx, C> FunctionHandler<'tcx> for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx> + DebugInfoHandler,
        C: ForFunctionCalling<'tcx>,
    {
        fn before_call_func(
            &mut self,
            func: &Operand<'tcx>,
            args: &[Spanned<Operand<'tcx>>],
            no_def: bool,
        ) {
            let tcx = self.tcx();
            let mut blocks = vec![];

            self.debug_info(&format!("{}", func.ty(self, self.tcx())));

            let def_local = {
                let func = if !no_def {
                    func.clone()
                } else {
                    operand::func(
                        self.tcx(),
                        *self.pri_helper_funcs().special_func_placeholder,
                        iter::empty(),
                    )
                };
                let BlocksAndResult(def_blocks, def_local) = func::definition_of_callee(
                    tcx,
                    self,
                    self.current_typing_env(),
                    func,
                    args.first().map(|a| &a.node),
                );
                blocks.extend(def_blocks);
                def_local
            };

            let func_ref = self.reference_operand(func);

            let arg_refs = args
                .iter()
                .map(|a| self.reference_operand_spanned(a))
                .map(|a| operand::move_for_local(a.into()))
                .collect();
            let operand_ref_ty = self.context.pri_types().operand_ref(tcx);
            let (arguments_local, additional_stmts) =
                prepare_operand_for_slice(tcx, &mut self.context, operand_ref_ty, arg_refs);

            let are_args_tupled = func::are_args_tupled(
                tcx,
                self,
                func,
                args.iter().map(|a| &a.node),
                self.current_typing_env(),
            );

            let mut block = self.make_bb_for_call(sym::before_call_func, vec![
                operand::move_for_local(def_local),
                self.original_bb_index_as_arg(),
                operand::move_for_local(func_ref.into()),
                operand::move_for_local(arguments_local),
                operand::const_from_bool(tcx, are_args_tupled),
            ]);
            block.statements.extend(additional_stmts);
            blocks.push(block);

            debug_assert_matches!(
                self.context.insertion_loc(),
                Before(..),
                "Inserting before_call after a block is not expected."
            );
            self.insert_blocks(blocks);
        }

        fn enter_func(&mut self) {
            let tcx = self.tcx();
            let mut blocks = vec![];

            self.debug_info(&format!("{}", func::body_func_ty(tcx, self.body())));

            let def_local = {
                let BlocksAndResult(def_blocks, def_local) =
                    func::definition_of_func(tcx, self, self.current_typing_env());
                blocks.extend(def_blocks);
                def_local
            };

            let (argument_places_local, additional_stmts) = {
                let arg_places_refs = self
                    .body()
                    .args_iter()
                    .map(|a| self.reference_place_local(a))
                    .map(|BlocksAndResult(ref_blocks, place_ref)| {
                        blocks.extend(ref_blocks);
                        place_ref
                    })
                    .map(|place_ref| operand::move_for_local(place_ref))
                    .collect();
                let place_ref_ty = self.context.pri_types().place_ref(tcx);
                prepare_operand_for_slice(tcx, &mut self.context, place_ref_ty, arg_places_refs)
            };

            let ret_val_place_local = {
                let BlocksAndResult(ref_blocks, place_ref) =
                    self.reference_place_local(Place::return_place().as_local().unwrap());
                blocks.extend(ref_blocks);
                place_ref
            };

            let base_args = vec![
                operand::move_for_local(def_local),
                operand::move_for_local(argument_places_local),
                operand::move_for_local(ret_val_place_local),
            ];

            let mut block = if let TyKind::Closure(_, args) = tcx
                .type_of(self.current_func_id())
                .instantiate_identity()
                .kind()
            {
                let (arg_blocks, tupled_args) = self.make_enter_func_tupled_args(args.as_closure());
                blocks.extend(arg_blocks);
                self.make_bb_for_call(
                    sym::enter_func_untupled_args,
                    [base_args, tupled_args.to_vec()].concat(),
                )
            } else if func::is_fn_trait_call_func(tcx, self.current_func_id()) {
                self.make_bb_for_call(sym::enter_func_tupled_args, base_args)
            } else {
                self.make_bb_for_call(sym::enter_func, base_args)
            };

            block.statements.extend(additional_stmts);
            blocks.push(block);

            self.insert_blocks(blocks);
        }

        fn return_from_func(&mut self) {
            let block =
                self.make_bb_for_call(sym::return_from_func, vec![self.original_bb_index_as_arg()]);
            self.insert_blocks([block]);
        }

        fn after_call_func(&mut self)
        where
            Self: AssignmentInfoProvider<'tcx>,
        {
            let block = self.make_bb_for_call(sym::after_call_func, vec![
                operand::const_from_uint(self.tcx(), self.assignment_id()),
                operand::copy_for_local(self.dest_ref().into()),
            ]);
            debug_assert_matches!(
                self.context.insertion_loc(),
                After(..),
                "Inserting after_call before a block is not expected."
            );
            self.insert_blocks([block]);
        }
    }

    impl<'tcx, C> RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: Basic<'tcx> + SourceInfoProvider,
    {
        fn make_enter_func_tupled_args(
            &mut self,
            args: mir_ty::ClosureArgs<TyCtxt<'tcx>>,
        ) -> (Vec<BasicBlockData<'tcx>>, [Operand<'tcx>; 2]) {
            let mut blocks = vec![];

            let tuple_id_local = {
                let (block, id_local) =
                    self.make_type_id_of_bb(ty::erased_tupled_closure_inputs(self.tcx(), args));
                blocks.push(block);
                id_local
            };

            (blocks, [
                operand::const_from_uint(self.tcx(), 2 as common::types::LocalIndex),
                operand::move_for_local(tuple_id_local),
            ])
        }
    }

    impl<'tcx, C> IntrinsicHandler<'tcx> for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForAssignment<'tcx>,
    {
        fn intrinsic_one_to_one_by(
            &mut self,
            intrinsic_func: DefId,
            pri_func: LeafIntrinsicSymbol,
            args: impl Iterator<Item = OperandRef>,
        ) {
            self.assert_pri_intrinsic_consistency(intrinsic_func, pri_func);

            let pri_name = *pri_func;
            let args = args.map(Into::into).map(operand::move_for_local).collect();
            let block = self.make_bb_for_assign_call(pri_name, args);
            self.insert_blocks([block]);
        }
    }

    impl<'tcx, C> RuntimeCallAdder<C>
    where
        C: Basic<'tcx>,
    {
        fn assert_pri_intrinsic_consistency(
            &mut self,
            intrinsic_func: DefId,
            pri_func: LeafIntrinsicSymbol,
        ) {
            let tcx = self.tcx();
            let arg_num = |def_id| {
                tcx.fn_sig(def_id)
                    .instantiate_identity()
                    .inputs()
                    .iter()
                    .count()
            };
            let pri_func_info = self.get_pri_func_info(*pri_func);

            let pri_func_arg_num = arg_num(pri_func_info.def_id);
            let intrinsic_arg_num = arg_num(intrinsic_func);

            assert_eq!(
                pri_func_arg_num - (1/* assignment_id */) - (1/* dest */),
                intrinsic_arg_num,
                "Inconsistent number of arguments between intrinsic and its corresponding PRI function. {:?} -x-> {:?}",
                intrinsic_func,
                pri_func_info.def_id
            );
        }
    }

    impl<'tcx, C> MemoryIntrinsicHandler<'tcx> for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForMemoryIntrinsic<'tcx>,
    {
        fn load(&mut self, is_ptr_aligned: bool) {
            self.add_bb_for_memory_op_intrinsic_call(
                sym::intrinsics::memory::intrinsic_memory_load,
                vec![
                    operand::move_for_local(self.dest_ref().into()),
                    operand::const_from_bool(self.tcx(), self.context.is_volatile()),
                    operand::const_from_bool(self.tcx(), is_ptr_aligned),
                ],
                Default::default(),
                Default::default(),
            );
        }

        fn store(&mut self, val: OperandRef, is_ptr_aligned: bool) {
            self.add_bb_for_memory_op_intrinsic_call(
                sym::intrinsics::memory::intrinsic_memory_store,
                vec![
                    operand::move_for_local(val.into()),
                    operand::const_from_bool(self.tcx(), self.context.is_volatile()),
                    operand::const_from_bool(self.tcx(), is_ptr_aligned),
                ],
                Default::default(),
                Default::default(),
            )
        }

        fn copy(
            &mut self,
            dst_ref: OperandRef,
            dst_value: &Operand<'tcx>,
            count_ref: OperandRef,
            count_value: &Operand<'tcx>,
            is_overlapping: bool,
        ) {
            let mut stmts = Vec::new();

            let conc_dst_ptr_local = {
                let (stmt, id_local) = self.make_conc_ptr_assignment(dst_value.clone());
                stmts.push(stmt);
                id_local
            };

            self.add_bb_for_memory_op_intrinsic_call(
                sym::intrinsics::memory::intrinsic_memory_copy,
                vec![
                    operand::move_for_local(dst_ref.into()),
                    operand::move_for_local(conc_dst_ptr_local),
                    operand::move_for_local(count_ref.into()),
                    count_value.to_copy(),
                    operand::const_from_bool(self.tcx(), self.context.is_volatile()),
                    operand::const_from_bool(self.tcx(), is_overlapping),
                ],
                stmts,
                Default::default(),
            )
        }
    }

    impl<'tcx, C> RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForMemoryIntrinsic<'tcx>,
    {
        fn add_bb_for_memory_op_intrinsic_call(
            &mut self,
            func: LeafMemoryIntrinsicSymbol,
            additional_args: Vec<Operand<'tcx>>,
            additional_stmts: Vec<Statement<'tcx>>,
            additional_blocks: Vec<BasicBlockData<'tcx>>,
        ) {
            let mut stmts = additional_stmts;
            let mut blocks = additional_blocks;

            let [ptr_ref, ptr_value, ptr_type_id] = {
                let (ptr_block, ptr_stmt, args) = self.make_ptr_pack_args();
                stmts.push(ptr_stmt);
                blocks.push(ptr_block);
                args
            };

            let mut block = self.make_bb_for_call(
                **func,
                [
                    vec![
                        operand::const_from_uint(self.tcx(), self.assignment_id()),
                        ptr_ref,
                        ptr_value,
                        ptr_type_id,
                    ],
                    additional_args,
                ]
                .concat(),
            );
            block.statements.extend(stmts);
            blocks.push(block);

            self.insert_blocks(blocks);
        }
    }

    impl<'tcx, C> AtomicIntrinsicHandler<'tcx> for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForAtomicIntrinsic<'tcx>,
    {
        fn load(&mut self)
        where
            Self: Assigner<'tcx>,
        {
            self.add_bb_for_atomic_intrinsic_call_with_ptr(
                sym::intrinsics::atomic::intrinsic_atomic_load,
                vec![operand::move_for_local(self.dest_ref().into())],
                Default::default(),
            );
        }

        fn store(&mut self, val: OperandRef)
        where
            Self: Assigner<'tcx>,
        {
            self.add_bb_for_atomic_intrinsic_call_with_ptr(
                sym::intrinsics::atomic::intrinsic_atomic_store,
                vec![operand::move_for_local(val.into())],
                Default::default(),
            )
        }

        fn exchange(&mut self, val: OperandRef)
        where
            Self: Assigner<'tcx>,
        {
            self.add_bb_for_atomic_intrinsic_call_with_ptr(
                sym::intrinsics::atomic::intrinsic_atomic_xchg,
                vec![
                    operand::move_for_local(val.into()),
                    operand::move_for_local(self.dest_ref().into()),
                ],
                Default::default(),
            );
        }

        fn compare_exchange(
            &mut self,
            failure_ordering: AtomicOrdering,
            weak: bool,
            old: OperandRef,
            src: OperandRef,
        ) where
            Self: Assigner<'tcx>,
        {
            let mut additional_blocks = vec![];

            let failure_ordering_local = {
                let bb = self.make_bb_for_atomic_ordering(failure_ordering);
                additional_blocks.extend(bb.0);
                bb.1
            };

            self.add_bb_for_atomic_intrinsic_call_with_ptr(
                sym::intrinsics::atomic::intrinsic_atomic_cxchg,
                vec![
                    operand::move_for_local(failure_ordering_local),
                    operand::const_from_bool(self.tcx(), weak),
                    operand::move_for_local(old.into()),
                    operand::move_for_local(src.into()),
                    operand::move_for_local(self.dest_ref().into()),
                ],
                additional_blocks,
            )
        }

        fn binary_op(&mut self, operator: AtomicBinaryOp, src: OperandRef)
        where
            Self: Assigner<'tcx>,
        {
            let tcx = self.tcx();
            let mut additional_blocks = vec![];

            let operator_local = {
                let (block, local) = self.make_bb_for_helper_call_with_all(
                    self.pri_helper_funcs().const_atomic_binary_op_of,
                    vec![],
                    vec![operand::const_from_uint(tcx, operator.as_u8())],
                    Default::default(),
                );
                additional_blocks.push(block);
                local
            };

            let prev_dest = self.dest_ref();

            self.add_bb_for_atomic_intrinsic_call_with_ptr(
                sym::intrinsics::atomic::intrinsic_atomic_binary_op,
                vec![
                    operand::move_for_local(operator_local),
                    operand::move_for_local(src.into()),
                    operand::move_for_local(prev_dest.into()),
                ],
                additional_blocks,
            )
        }

        fn fence(&mut self, single_thread: bool) {
            self.add_bb_for_atomic_intrinsic_call(
                sym::intrinsics::atomic::intrinsic_atomic_fence,
                vec![operand::const_from_bool(self.tcx(), single_thread)],
                Default::default(),
                Default::default(),
            );
        }
    }

    impl<'tcx, C> RuntimeCallAdder<C>
    where
        C: ForAtomicIntrinsic<'tcx>,
    {
        fn add_bb_for_atomic_intrinsic_call_with_ptr(
            &mut self,
            func: LeafAtomicIntrinsicSymbol,
            additional_args: Vec<Operand<'tcx>>,
            additional_blocks: Vec<BasicBlockData<'tcx>>,
        ) where
            Self: Assigner<'tcx>,
        {
            let mut stmts = Vec::new();
            let mut blocks = additional_blocks;

            let [ptr_ref, ptr_value, ptr_type_id] = {
                let (ptr_block, ptr_stmt, args) = self.make_ptr_pack_args();
                stmts.push(ptr_stmt);
                blocks.push(ptr_block);
                args
            };

            self.add_bb_for_atomic_intrinsic_call(
                func,
                vec![
                    vec![
                        operand::const_from_uint(self.tcx(), self.assignment_id()),
                        ptr_ref,
                        ptr_value,
                        ptr_type_id,
                    ],
                    additional_args,
                ]
                .concat(),
                stmts,
                blocks,
            );
        }

        fn add_bb_for_atomic_intrinsic_call(
            &mut self,
            func: LeafAtomicIntrinsicSymbol,
            additional_args: Vec<Operand<'tcx>>,
            additional_stmts: Vec<Statement<'tcx>>,
            additional_blocks: Vec<BasicBlockData<'tcx>>,
        ) {
            let mut blocks = additional_blocks;

            let ordering_local = {
                let bb = self.make_bb_for_atomic_ordering(self.context.ordering());
                blocks.extend(bb.0);
                bb.1
            };

            let mut block = self.make_bb_for_call(
                **func,
                [
                    vec![operand::move_for_local(ordering_local)],
                    additional_args,
                ]
                .concat(),
            );
            block.statements.extend(additional_stmts);
            blocks.push(block);

            self.insert_blocks(blocks);
        }
    }

    impl<'tcx, C> RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx>,
        C: Basic<'tcx>,
    {
        fn make_ptr_pack_args(
            &mut self,
        ) -> (BasicBlockData<'tcx>, Statement<'tcx>, [Operand<'tcx>; 3])
        where
            C: PointerInfoProvider<'tcx>,
        {
            let (conc_ptr_assignment, conc_ptr_local) =
                self.make_conc_ptr_assignment(self.context.ptr_value().clone());

            let (ptr_type_id_block, ptr_type_id_local) =
                self.make_type_id_of_bb(self.context.ptr_ty());

            (ptr_type_id_block, conc_ptr_assignment, [
                operand::move_for_local(self.context.ptr_operand_ref().into()),
                operand::move_for_local(conc_ptr_local),
                operand::move_for_local(ptr_type_id_local),
            ])
        }

        fn make_conc_ptr_assignment(
            &mut self,
            ptr_value: Operand<'tcx>,
        ) -> (Statement<'tcx>, Local) {
            let tcx = self.tcx();
            let raw_addr_ty = Ty::new_imm_ptr(tcx, tcx.types.unit);
            let local = self.add_local(raw_addr_ty);
            let assignment = assignment::create(
                Place::from(local),
                mir::Rvalue::Cast(CastKind::PtrToPtr, ptr_value, raw_addr_ty),
            );

            (assignment, local)
        }
    }

    impl<'tcx, C> RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx>,
        C: Basic<'tcx>,
    {
        fn make_bb_for_atomic_ordering(
            &mut self,
            ordering: AtomicOrdering,
        ) -> BlocksAndResult<'tcx> {
            let tcx = self.tcx();

            self.make_bb_for_helper_call_with_all(
                self.pri_helper_funcs().const_atomic_ord_of,
                vec![],
                vec![operand::const_from_uint(tcx, ordering.as_u8())],
                Default::default(),
            )
            .into()
        }
    }

    impl<'tcx, C> AssertionHandler<'tcx> for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForAssertion<'tcx>,
    {
        fn check_assert(
            &mut self,
            cond: OperandRef,
            expected: bool,
            msg: &rustc_middle::mir::AssertMessage<'tcx>,
        ) {
            let Some((func_name, additional_operands)) = self.reference_assert_kind(msg) else {
                return;
            };

            let (info_block, info_local) = {
                self.make_bb_for_helper_call_with_all(
                    self.context.pri_helper_funcs().assertion_info,
                    [],
                    vec![
                        self.original_bb_index_as_arg(),
                        operand::move_for_local(cond.into()),
                        operand::const_from_bool(self.context.tcx(), expected),
                    ],
                    Default::default(),
                )
            };

            let block = self.make_bb_for_call(
                func_name,
                [
                    vec![operand::move_for_local(info_local.into())],
                    additional_operands,
                ]
                .concat(),
            );

            self.insert_blocks([info_block, block]);
        }
    }
    impl<'tcx, C> RuntimeCallAdder<C>
    where
        C: ForAssertion<'tcx>,
    {
        fn reference_assert_kind(
            &mut self,
            msg: &rustc_middle::mir::AssertMessage<'tcx>,
        ) -> Option<(LeafSymbol, Vec<Operand<'tcx>>)> {
            use rustc_middle::mir::AssertKind;
            match msg {
                AssertKind::BoundsCheck { len, index } => {
                    let len_ref = self.reference_operand(len);
                    let index_ref = self.reference_operand(index);
                    Some((sym::assert_bounds_check, vec![
                        operand::copy_for_local(len_ref.into()),
                        operand::copy_for_local(index_ref.into()),
                    ]))
                }
                AssertKind::Overflow(operator, op1, op2) => {
                    let tcx = self.tcx();

                    let operator = convert_mir_binop_to_pri(operator);
                    let operator_local = {
                        let (block, local) = self.make_bb_for_helper_call_with_all(
                            self.context.pri_helper_funcs().const_binary_op_of,
                            vec![],
                            vec![operand::const_from_uint(tcx, operator.as_u8())],
                            Default::default(),
                        );
                        self.insert_blocks([block]);
                        local
                    };

                    let op1_ref = self.reference_operand(op1);
                    let op2_ref = self.reference_operand(op2);
                    Some((sym::assert_overflow, vec![
                        // TODO: double check that moves and copies here are correct
                        operand::move_for_local(operator_local),
                        operand::copy_for_local(op1_ref.into()),
                        operand::copy_for_local(op2_ref.into()),
                    ]))
                }
                AssertKind::OverflowNeg(op) => {
                    let op_ref = self.reference_operand(op);
                    Some((sym::assert_overflow_neg, vec![operand::copy_for_local(
                        op_ref.into(),
                    )]))
                }
                AssertKind::DivisionByZero(op) => {
                    let op_ref = self.reference_operand(op);
                    Some((sym::assert_div_by_zero, vec![operand::copy_for_local(
                        op_ref.into(),
                    )]))
                }
                AssertKind::RemainderByZero(op) => {
                    let op_ref = self.reference_operand(op);
                    Some((sym::assert_rem_by_zero, vec![operand::copy_for_local(
                        op_ref.into(),
                    )]))
                }
                AssertKind::ResumedAfterReturn(..) | AssertKind::ResumedAfterPanic(..) => {
                    /* NOTE: These two assertions look to be used to make sure
                     * that the state machine is correctly generated.
                     * When they are reached, an illegal has happened in the state machine.
                     * They are in fact assert(false) statements.
                     */
                    None
                }
                AssertKind::MisalignedPointerDereference { required, found } => {
                    let required_ref = self.reference_operand(required);
                    let found_ref = self.reference_operand(found);
                    Some((sym::assert_misaligned_ptr_deref, vec![
                        operand::copy_for_local(required_ref.into()),
                        operand::copy_for_local(found_ref.into()),
                    ]))
                }
            }
        }
    }

    impl<'tcx, C> EntryFunctionHandler for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForEntryFunction<'tcx>,
    {
        fn init_runtime_lib(&mut self) {
            let block = self.make_bb_for_call(sym::init_runtime_lib, vec![]);
            self.insert_blocks([block]);
        }

        fn shutdown_runtime_lib(&mut self) {
            let block = self.make_bb_for_call(sym::shutdown_runtime_lib, vec![]);
            self.insert_blocks([block]);
        }
    }

    impl<'tcx, C> RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx>,
        C: TyContextProvider<'tcx> + PriItemsProvider<'tcx>,
    {
        fn make_type_id_of_bb(&mut self, ty: Ty<'tcx>) -> (BasicBlockData<'tcx>, Local) {
            /* NOTE: As `TypeId::of` requires static lifetime, do we need to clear lifetimes?
             * Yes, as higher-ranked regions still appear here. Importantly, they are distinguished
             * in the calculation of type id. However, as the types are used for runtime information
             * (like layout) and also in type exportation, we perform erasing, we erase the regions
             * to make sure the same type is used although over-approximated. */
            let ty = self.context.tcx().erase_regions(ty);

            self.make_bb_for_helper_call_with_all(
                self.context.pri_helper_funcs().type_id_of,
                vec![ty.into()],
                Vec::default(),
                None,
            )
        }
    }

    impl<'tcx, C> DebugInfoHandler for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx>,
        C: ForInsertion<'tcx>,
    {
        fn debug_info<T: Serialize>(&mut self, info: &T) {
            // FIXME: Make it configurable.
            if self.tcx().sess.opts.optimize != rustc_session::config::OptLevel::No {
                return;
            }

            let serialized = ron::to_string(info).unwrap();
            let block = self.make_bb_for_call(sym::debug_info, vec![operand::const_from_byte_str(
                self.context.tcx(),
                serialized.as_bytes(),
            )]);
            self.insert_blocks([block]);
        }
    }

    struct BlocksAndResult<'tcx>(Vec<BasicBlockData<'tcx>>, Local);

    impl<'tcx> BlocksAndResult<'tcx> {
        fn prepend(self, blocks: impl IntoIterator<Item = BasicBlockData<'tcx>>) -> Self {
            Self(blocks.into_iter().chain(self.0).collect(), self.1)
        }
    }

    impl<'tcx> From<(BasicBlockData<'tcx>, Local)> for BlocksAndResult<'tcx> {
        fn from(value: (BasicBlockData<'tcx>, Local)) -> Self {
            BlocksAndResult(vec![value.0], value.1)
        }
    }

    pub(super) mod utils {
        use rustc_middle::{
            mir::{
                self, BasicBlock, BasicBlockData, HasLocalDecls, Local, Operand, Place, Rvalue,
                SourceInfo, Statement,
            },
            query::Key,
            ty::{Ty, TyCtxt, TyKind, TypingEnv, adjustment::PointerCoercion},
        };
        use rustc_span::DUMMY_SP;

        use crate::mir_transform::{BodyLocalManager, NEXT_BLOCK};

        pub(super) use self::assignment::rvalue;

        use super::*;

        pub(super) mod operand {
            use std::mem::size_of;

            use rustc_const_eval::interpret::Scalar;
            use rustc_middle::{
                mir::Const,
                ty::{self, ScalarInt},
            };
            use rustc_type_ir::UintTy;

            use super::*;

            fn uint_ty_from_bytes(size: usize) -> UintTy {
                [
                    UintTy::U8,
                    UintTy::U16,
                    UintTy::U32,
                    UintTy::U64,
                    UintTy::U128,
                ]
                .into_iter()
                .find(|t| (t.bit_width().unwrap() / 8) as usize == size)
                .unwrap()
            }

            pub fn const_from_existing_ty_const<'tcx>(
                ty: Ty<'tcx>,
                constant: ty::Const<'tcx>,
            ) -> Operand<'tcx> {
                const_from_existing(&Box::new(ConstOperand {
                    span: DUMMY_SP,
                    user_ty: None,
                    const_: Const::Ty(ty, constant),
                }))
            }

            #[allow(clippy::borrowed_box)]
            pub fn const_from_existing<'tcx>(constant: &Box<ConstOperand<'tcx>>) -> Operand<'tcx> {
                Operand::Constant(constant.clone())
            }

            pub fn const_from_uint<T>(tcx: TyCtxt, value: T) -> Operand
            where
                T: Into<u128>,
            {
                const_from_scalar_int(
                    tcx,
                    ScalarInt::try_from_uint(value, rustc_abi::Size::from_bytes(size_of::<T>()))
                        .unwrap(),
                    Ty::new_uint(tcx, uint_ty_from_bytes(size_of::<T>())),
                )
            }

            pub fn const_from_bool(tcx: TyCtxt, value: bool) -> Operand {
                const_from_scalar_int(tcx, ScalarInt::from(value), tcx.types.bool)
            }

            pub fn const_from_char(tcx: TyCtxt, value: char) -> Operand {
                const_from_scalar_int(tcx, ScalarInt::from(value), tcx.types.char)
            }

            pub fn const_from_scalar_int<'tcx>(
                tcx: TyCtxt<'tcx>,
                value: ScalarInt,
                ty: Ty<'tcx>,
            ) -> Operand<'tcx> {
                Operand::const_from_scalar(tcx, ty, Scalar::Int(value), DUMMY_SP)
            }

            pub fn const_from_byte_str<'tcx>(tcx: TyCtxt<'tcx>, value: &[u8]) -> Operand<'tcx> {
                let ty = Ty::new_imm_ref(
                    tcx,
                    tcx.lifetimes.re_static,
                    Ty::new_slice(tcx, tcx.types.u8),
                );
                Operand::Constant(Box::new(ConstOperand {
                    span: DUMMY_SP,
                    user_ty: None,
                    const_: Const::from_value(
                        tcx.valtree_to_const_val((ty, mir_ty::ValTree::from_raw_bytes(tcx, value))),
                        ty,
                    ),
                }))
            }

            pub fn const_try_as_unevaluated<'tcx>(
                constant: &ConstOperand<'tcx>,
            ) -> Option<mir::UnevaluatedConst<'tcx>> {
                match constant.const_ {
                    Const::Unevaluated(c, _) => Some(c),
                    Const::Ty(_ty, c) => match c.kind() {
                        ty::ConstKind::Unevaluated(ty::UnevaluatedConst { def, args }) => {
                            Some(mir::UnevaluatedConst {
                                def,
                                args,
                                promoted: None,
                            })
                        }
                        _ => None,
                    },
                    _ => None,
                }
            }

            pub fn copy_for_local<'tcx>(value: Local) -> Operand<'tcx> {
                for_local(value, true)
            }

            pub fn move_for_local<'tcx>(value: Local) -> Operand<'tcx> {
                for_local(value, false)
            }

            pub fn for_local<'tcx>(value: Local, copy: bool) -> Operand<'tcx> {
                let place = Place::from(value);
                if copy {
                    Operand::Copy(place)
                } else {
                    Operand::Move(place)
                }
            }

            pub fn func<'tcx>(
                tcx: TyCtxt<'tcx>,
                def_id: DefId,
                substs: impl IntoIterator<Item = GenericArg<'tcx>>,
            ) -> Operand<'tcx> {
                Operand::function_handle(tcx, def_id, substs, DUMMY_SP)
            }
        }

        pub(super) mod assignment {
            use rustc_middle::mir::StatementKind;

            use super::*;

            pub mod rvalue {
                use rustc_index::IndexVec;
                use rustc_middle::mir::{AggregateKind, BorrowKind, CastKind, CoercionSource};

                use super::*;

                pub fn ref_of<'tcx>(target: Place<'tcx>, tcx: TyCtxt<'tcx>) -> Rvalue<'tcx> {
                    Rvalue::Ref(tcx.lifetimes.re_erased, BorrowKind::Shared, target)
                }

                pub fn cast_to_unsize<'tcx>(
                    operand: Operand<'tcx>,
                    to_ty: Ty<'tcx>,
                ) -> Rvalue<'tcx> {
                    cast_to_coerced_as(PointerCoercion::Unsize, operand, to_ty)
                }

                pub fn cast_to_coerced_as<'tcx>(
                    coercion: PointerCoercion,
                    operand: Operand<'tcx>,
                    to_ty: Ty<'tcx>,
                ) -> Rvalue<'tcx> {
                    Rvalue::Cast(
                        CastKind::PointerCoercion(coercion, CoercionSource::AsCast),
                        operand,
                        to_ty,
                    )
                }

                pub fn cast_to_fn_ptr<'tcx>(
                    tcx: TyCtxt<'tcx>,
                    operand: Operand<'tcx>,
                    operand_ty: Ty<'tcx>,
                ) -> Rvalue<'tcx> {
                    cast_to_coerced_as(
                        PointerCoercion::ReifyFnPointer,
                        operand,
                        Ty::new_fn_ptr(tcx, ty::fn_ptr_sig(tcx, operand_ty)),
                    )
                }

                pub fn array<'tcx>(ty: Ty<'tcx>, items: Vec<Operand<'tcx>>) -> Rvalue<'tcx> {
                    Rvalue::Aggregate(
                        Box::new(AggregateKind::Array(ty)),
                        IndexVec::from_raw(items),
                    )
                }
            }

            pub fn create<'tcx>(destination: Place<'tcx>, value: Rvalue<'tcx>) -> Statement<'tcx> {
                Statement {
                    /* NOTE: The source info can be propagated here too.
                     * However, as these statements end with a function call which has source info,
                     * we avoid the expense passing it around. */
                    source_info: SourceInfo::outermost(DUMMY_SP),
                    kind: StatementKind::Assign(Box::new((destination, value))),
                }
            }
        }

        pub(super) mod ty {
            use rustc_abi::Size;

            use super::*;

            pub trait TyExt<'tcx> {
                fn is_trivially_tuple(self) -> bool;
                fn is_tuple(self, tcx: TyCtxt<'tcx>, typing_env: TypingEnv<'tcx>) -> bool;
            }

            impl<'tcx> TyExt<'tcx> for Ty<'tcx> {
                #[inline]
                fn is_trivially_tuple(self) -> bool {
                    matches!(self.kind(), TyKind::Tuple(..))
                }

                fn is_tuple(self, tcx: TyCtxt<'tcx>, typing_env: TypingEnv<'tcx>) -> bool {
                    self.is_trivially_tuple()
                        || is_trait(
                            tcx,
                            self,
                            typing_env,
                            tcx.lang_items().tuple_trait().unwrap(),
                        )
                }
            }

            pub fn mk_imm_ref<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Ty<'tcx> {
                Ty::new_imm_ref(tcx, tcx.lifetimes.re_erased, ty)
            }

            pub fn size_of<'tcx>(
                tcx: TyCtxt<'tcx>,
                ty: Ty<'tcx>,
                typing_env: TypingEnv<'tcx>,
            ) -> Size {
                tcx.layout_of(typing_env.as_query_input(ty)).unwrap().size
            }

            pub fn ebit_sbit_size<'tcx>(ty: Ty<'tcx>) -> (u64, u64) {
                use rustc_apfloat::{Float, ieee::*};
                let mir_ty::Float(float_ty) = ty.kind() else {
                    panic!("Expected floating point type but received: {}", ty)
                };
                let bit_size = float_ty.bit_width();
                let sbit_size = match bit_size as usize {
                    Half::BITS => Half::PRECISION,
                    Single::BITS => Single::PRECISION,
                    Double::BITS => Double::PRECISION,
                    Quad::BITS => Quad::PRECISION,
                    _ => unreachable!("Unexpected floating point bit size: {}", bit_size),
                } as u64;
                (bit_size - sbit_size, sbit_size)
            }

            /// Returns the corresponding FnDef type of a closure when called,
            /// i.e. `<closure as Fn*<I>>::call*()`
            ///
            /// [`ty`]: closure type
            pub fn fn_def_of_closure_call<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Ty<'tcx> {
                let TyKind::Closure(_, args) = ty.kind() else {
                    panic!("Expected closure type but received: {}", ty)
                };

                log_debug!("Getting FnDef type of closure: {:?}", ty);
                let args = args.as_closure();

                // Finding the call* method in the Fn* trait.
                let fn_trait_fn_id = def_id_of_single_func_of_trait(
                    tcx,
                    tcx.fn_trait_kind_to_def_id(args.kind()).unwrap(),
                );

                let inputs = erased_tupled_closure_inputs(tcx, args);
                Ty::new_fn_def(tcx, fn_trait_fn_id, [ty, inputs])
            }

            pub fn erased_tupled_closure_inputs<'tcx>(
                tcx: TyCtxt<'tcx>,
                args: mir_ty::ClosureArgs<TyCtxt<'tcx>>,
            ) -> Ty<'tcx> {
                // Inputs types are collated into a tuple and are the only generic argument of the Fn trait.
                let inputs = args.sig().inputs().map_bound(|inputs| inputs[0]);
                tcx.instantiate_bound_regions_with_erased(inputs)
            }

            /// Returns the corresponding FnDef type of a coroutine when resumed,
            /// i.e. `<coroutine as Coroutine<R>>::resume()`
            ///
            /// [`ty`]: coroutine type
            pub fn fn_def_of_coroutine_resume<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Ty<'tcx> {
                let TyKind::Coroutine(_, args) = ty.kind() else {
                    panic!("Expected coroutine type but received: {}", ty)
                };
                log_debug!("Getting FnDef type of coroutine: {:?}", ty);
                let args = args.as_coroutine();

                // Finding `resume` method in `Coroutine` trait.
                let coroutine_trait_fn_id = def_id_of_single_func_of_trait(
                    tcx,
                    tcx.lang_items().coroutine_trait().unwrap(),
                );

                // NOTE: Currently, either zero or one parameters are supported for coroutines.
                let inputs = args.sig().resume_ty;
                Ty::new_fn_def(tcx, coroutine_trait_fn_id, [ty, inputs])
            }

            /// Returns the corresponding FnDef type of an async coroutine when awaited,
            /// i.e. `<coroutine as Future>::poll()`
            ///
            /// [`ty`]: coroutine type
            pub fn fn_def_of_coroutine_await<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Ty<'tcx> {
                let TyKind::Coroutine(def_id, args) = ty.kind() else {
                    panic!("Expected coroutine type but received: {}", ty)
                };
                assert!(
                    tcx.coroutine_is_async(*def_id),
                    "Expected async coroutine kind but received: {:?}",
                    tcx.coroutine_kind(*def_id),
                );

                log_debug!("Getting FnDef type of async coroutine: {:?}", ty);
                let args = args.as_coroutine();

                // Finding `resume` method in `Coroutine` trait.
                let future_trait_fn_id = tcx.lang_items().future_poll_fn().unwrap();

                // NOTE: Currently, either zero or one parameters are supported for coroutines.
                let inputs = args.sig().resume_ty;
                Ty::new_fn_def(tcx, future_trait_fn_id, [ty, inputs])
            }

            /// Returns the corresponding FnDef type of calling a function through Fn* traits.
            /// i.e. `<fn_ty as Fn*<I>>::call*()`
            pub fn fn_def_of_fn_ptr_shim<'tcx>(
                tcx: TyCtxt<'tcx>,
                fn_trait_fn_id: DefId,
                fn_ty: Ty<'tcx>,
            ) -> Ty<'tcx> {
                debug_assert_matches!(fn_ty.kind(), TyKind::FnPtr(..) | TyKind::FnDef(..));
                let inputs = fn_ty
                    .fn_sig(tcx)
                    .inputs()
                    .map_bound(|ts| Ty::new_tup(tcx, ts));
                let inputs = tcx.instantiate_bound_regions_with_erased(inputs);
                Ty::new_fn_def(tcx, fn_trait_fn_id, [fn_ty, inputs])
            }

            pub fn fn_def_of_closure_once_shim<'tcx>(
                tcx: TyCtxt<'tcx>,
                fn_trait_fn_id: DefId,
                input_arg_tys: &[Ty<'tcx>],
            ) -> Ty<'tcx> {
                let [closure_ty, args_ty] = input_arg_tys else {
                    panic!(
                        "Expected two arguments for (Self, (Args)) but received: {:?}",
                        input_arg_tys
                    )
                };

                Ty::new_fn_def(tcx, fn_trait_fn_id, [*closure_ty, *args_ty])
            }

            pub fn fn_def_of_clone_shim<'tcx>(
                tcx: TyCtxt<'tcx>,
                clone_fn_id: DefId,
                ty: Ty<'tcx>,
            ) -> Ty<'tcx> {
                let mir_ty::TyKind::Ref(_, ty, _) = ty.kind() else {
                    /* NOTE: It is not really obvious where or why,
                     * but apparently a reference to the type is passed for the shim. */
                    panic!("Expected reference type but received: {:?}", ty)
                };
                Ty::new_fn_def(tcx, clone_fn_id, [*ty])
            }

            fn def_id_of_single_func_of_trait(tcx: TyCtxt, trait_id: DefId) -> DefId {
                let mut funcs = tcx
                    .associated_items(trait_id)
                    .in_definition_order()
                    .filter(|x| matches!(x.kind, rustc_middle::ty::AssocKind::Fn));
                let func = funcs.next().expect("No function found in the trait.");
                assert!(
                    funcs.next().is_none(),
                    "More than one function found in the trait."
                );
                func.def_id
            }

            pub fn fn_ptr_sig<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> mir_ty::PolyFnSig<'tcx> {
                match ty.kind() {
                    TyKind::FnDef(..) => ty.fn_sig(tcx),
                    TyKind::FnPtr(tys, header) => tys.with(*header),
                    _ => unreachable!(
                        "Unexpected type to get function pointer directly from: {}",
                        ty
                    ),
                }
            }

            pub(super) fn is_trait<'tcx>(
                tcx: TyCtxt<'tcx>,
                ty: Ty<'tcx>,
                typing_env: TypingEnv<'tcx>,
                trait_def_id: DefId,
            ) -> bool {
                use rustc_infer::infer::TyCtxtInferExt;
                use rustc_trait_selection::infer::InferCtxtExt;

                tcx.infer_ctxt()
                    .build(typing_env.typing_mode)
                    .type_implements_trait(trait_def_id, [ty], typing_env.param_env)
                    .must_apply_modulo_regions()
            }
        }

        pub(super) mod terminator {
            use rustc_middle::mir::{Terminator, TerminatorKind, UnwindAction};
            use rustc_span::source_map::Spanned;

            use super::*;

            pub fn call<'tcx>(
                tcx: TyCtxt<'tcx>,
                func_def_id: DefId,
                generic_args: impl IntoIterator<Item = GenericArg<'tcx>>,
                args: Vec<Operand<'tcx>>,
                destination: Place<'tcx>,
                target: Option<BasicBlock>,
                source_info: SourceInfo,
            ) -> Terminator<'tcx> {
                Terminator {
                    source_info,
                    kind: TerminatorKind::Call {
                        /* NOTE: Check if it is supposed to be the same operand for each function definition,
                         * i.e. caching/lazy singleton. */
                        func: operand::func(tcx, func_def_id, generic_args),
                        args: args
                            .into_iter()
                            .map(|a| Spanned {
                                node: a,
                                span: DUMMY_SP,
                            })
                            .collect(),
                        destination,
                        target: Some(target.unwrap_or(NEXT_BLOCK)),
                        unwind: UnwindAction::Continue,
                        call_source: mir::CallSource::Normal,
                        fn_span: DUMMY_SP,
                    },
                }
            }

            pub fn goto<'tcx>(target: Option<BasicBlock>) -> Terminator<'tcx> {
                Terminator {
                    source_info: SourceInfo::outermost(DUMMY_SP),
                    kind: TerminatorKind::Goto {
                        target: target.unwrap_or(NEXT_BLOCK),
                    },
                }
            }
        }

        pub(super) mod func {
            use common::log_info;
            use mir_ty::{AssocItem, ExistentialTraitRef, TraitRef, TypingEnv};
            use rustc_middle::ty::InstanceKind;
            use rustc_trait_selection::traits::is_vtable_safe_method;

            use crate::{
                passes::instr::decision::get_baked_dyn_def_rules, utils::mir::InstanceKindExt,
            };

            use super::*;

            pub fn body_func_ty<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>) -> Ty<'tcx> {
                let source = body.source;
                log_debug!("Creating type of current function: {}", source.to_log_str());

                use mir_ty::InstanceKind::*;
                let fn_def_ty = match source.instance {
                    Item(def_id) => {
                        let ty = tcx.type_of(def_id).instantiate_identity();
                        match ty.kind() {
                            TyKind::FnDef(..) => ty,
                            TyKind::Closure(..) => ty::fn_def_of_closure_call(tcx, ty),
                            TyKind::Coroutine(def_id, ..) => {
                                use rustc_hir::{CoroutineDesugaring::*, CoroutineKind::*};
                                match tcx.coroutine_kind(*def_id).unwrap() {
                                    Coroutine(..) => ty::fn_def_of_coroutine_resume(tcx, ty),
                                    Desugared(Async, ..) => ty::fn_def_of_coroutine_await(tcx, ty),
                                    Desugared(des, ..) => unimplemented!(
                                        "This type of coroutine is unstable and currently out of scope: {:?}, source: {:?}",
                                        des,
                                        ty,
                                    ),
                                }
                            }
                            _ => unreachable!("Unexpected type for body instance: {:?}", ty),
                        }
                    }
                    ReifyShim(def_id, _) => tcx.type_of(def_id).instantiate_identity(),
                    FnPtrShim(fn_trait_fn_id, fn_ptr_ty) => {
                        ty::fn_def_of_fn_ptr_shim(tcx, fn_trait_fn_id, fn_ptr_ty)
                    }
                    ClosureOnceShim { call_once, .. } => {
                        let arg_tys = body
                            .args_iter()
                            .map(|local| body.local_decls()[local].ty)
                            .collect::<Vec<_>>();
                        ty::fn_def_of_closure_once_shim(tcx, call_once, &arg_tys)
                    }
                    CloneShim(clone_fn_id, self_ty) => {
                        ty::fn_def_of_clone_shim(tcx, clone_fn_id, self_ty)
                    }
                    instance @ _ => unreachable!("Unsupported instance: {:?}", instance),
                };

                debug_assert_matches!(fn_def_ty.kind(), TyKind::FnDef(..));
                fn_def_ty
            }

            pub fn definition_of_func<'tcx>(
                tcx: TyCtxt<'tcx>,
                call_adder: &mut (
                         impl BodyLocalManager<'tcx>
                         + BodyProvider<'tcx>
                         + MirCallAdder<'tcx>
                         + PriItemsProvider<'tcx>
                         + StorageProvider
                     ),
                typing_env: mir_ty::TypingEnv<'tcx>,
            ) -> BlocksAndResult<'tcx> {
                let instance_kind = call_adder.body().source.instance;
                let fn_def_ty = body_func_ty(tcx, call_adder.body());
                let TyKind::FnDef(def_id, generic_args) = *fn_def_ty.kind() else {
                    unreachable!(
                        "Expected function definition type but received: {}",
                        fn_def_ty
                    )
                };

                let fn_value = operand::func(tcx, def_id, generic_args);

                if let Some((trait_ref, trait_item)) = as_dyn_compatible_method(tcx, def_id)
                    && trait_ref.self_ty().is_sized(tcx, typing_env)
                {
                    let ruled_out = get_baked_dyn_def_rules(call_adder.storage())
                        .accept(&(tcx, def_id))
                        .map_or(false, |include| !include);
                    if ruled_out {
                        log_info!(
                            "Dyn-compatible method will be defined as static: {:?}",
                            def_id
                        );
                        def_of_static_func(tcx, call_adder, instance_kind, fn_value)
                    } else {
                        def_of_dyn_compatible_method(
                            tcx,
                            call_adder,
                            typing_env,
                            instance_kind,
                            fn_value,
                            trait_ref,
                            trait_item.def_id,
                        )
                    }
                } else {
                    def_of_static_func(tcx, call_adder, instance_kind, fn_value)
                }
            }

            fn def_of_static_func<'tcx>(
                tcx: TyCtxt<'tcx>,
                call_adder: &mut (
                         impl BodyLocalManager<'tcx> + MirCallAdder<'tcx> + PriItemsProvider<'tcx>
                     ),
                instance_kind: InstanceKind<'tcx>,
                fn_value: Operand<'tcx>,
            ) -> BlocksAndResult<'tcx> {
                let (fn_ptr_ty, fn_ptr_local, ptr_assignment) =
                    to_fn_ptr(tcx, call_adder, fn_value.clone());

                let (mut block, id_local) = call_adder.make_bb_for_helper_call_with_all(
                    call_adder.pri_helper_funcs().func_def_static,
                    vec![fn_ptr_ty.into()],
                    [
                        vec![operand::move_for_local(fn_ptr_local)],
                        instance_kind_id_operand_triple(tcx, instance_kind).into(),
                    ]
                    .concat(),
                    None,
                );
                block.statements.push(ptr_assignment);
                BlocksAndResult::from((block, id_local))
            }

            fn def_of_dyn_compatible_method<'tcx>(
                tcx: TyCtxt<'tcx>,
                call_adder: &mut (
                         impl BodyLocalManager<'tcx>
                         + BodyProvider<'tcx>
                         + MirCallAdder<'tcx>
                         + PriItemsProvider<'tcx>
                     ),
                typing_env: TypingEnv<'tcx>,
                instance_kind: InstanceKind<'tcx>,
                fn_value: Operand<'tcx>,
                trait_ref: TraitRef<'tcx>,
                method_id: DefId,
            ) -> BlocksAndResult<'tcx> {
                let (fn_ptr_ty, fn_ptr_local, ptr_assignment) =
                    to_fn_ptr(tcx, call_adder, fn_value.clone());

                let self_ty = trait_ref.self_ty();
                let (raw_ptr_of_receiver_block, raw_ptr_of_receiver_local) = raw_ptr_of_receiver(
                    tcx,
                    call_adder,
                    typing_env,
                    call_adder
                        .body()
                        .args_iter()
                        .next()
                        .expect("An object safe trait method is expected to have a receiver")
                        .into(),
                    self_ty,
                );

                let dyn_ty = Ty::new_dynamic(
                    tcx,
                    tcx.mk_poly_existential_predicates(&[mir_ty::Binder::dummy(
                        rustc_type_ir::ExistentialPredicate::Trait(
                            ExistentialTraitRef::erase_self_ty(tcx, trait_ref),
                        ),
                    )]),
                    tcx.lifetimes.re_erased,
                    mir_ty::DynKind::Dyn,
                );

                let identifier = identifier_of_method(tcx, method_id);

                let (mut block, id_local) = call_adder.make_bb_for_helper_call_with_all(
                    call_adder.pri_helper_funcs().func_def_dyn_method,
                    vec![fn_ptr_ty.into(), self_ty.into(), dyn_ty.into()],
                    [
                        vec![
                            operand::move_for_local(fn_ptr_local),
                            operand::move_for_local(raw_ptr_of_receiver_local),
                            operand::const_from_uint(tcx, identifier),
                        ],
                        instance_kind_id_operand_triple(tcx, instance_kind).into(),
                    ]
                    .concat(),
                    None,
                );
                block.statements.push(ptr_assignment);

                BlocksAndResult(
                    merge_if_not_call((raw_ptr_of_receiver_block, block)),
                    id_local,
                )
            }

            fn as_dyn_compatible_method<'tcx>(
                tcx: TyCtxt<'tcx>,
                def_id: DefId,
            ) -> Option<(TraitRef<'tcx>, AssocItem)> {
                let trait_ref = tcx
                    .impl_of_method(def_id)
                    .and_then(|impl_id| tcx.impl_trait_ref(impl_id))
                    .map(|trait_ref| trait_ref.instantiate_identity())
                    .filter(|trait_ref| tcx.is_dyn_compatible(trait_ref.def_id))
                    .filter(|trait_ref| {
                        trait_ref.def_id != tcx.lang_items().deref_trait().unwrap()
                    })?;

                let item = to_trait_associated_item(tcx, def_id);
                is_vtable_safe_method(tcx, trait_ref.def_id, item).then_some((trait_ref, item))
            }

            pub fn definition_of_callee<'tcx>(
                tcx: TyCtxt<'tcx>,
                call_adder: &mut (
                         impl BodyLocalManager<'tcx> + MirCallAdder<'tcx> + PriItemsProvider<'tcx>
                     ),
                typing_env: TypingEnv<'tcx>,
                fn_value: Operand<'tcx>,
                first_arg: Option<&Operand<'tcx>>,
            ) -> BlocksAndResult<'tcx> {
                let fn_ty = fn_value.ty(call_adder, tcx);

                match fn_ty.kind() {
                    TyKind::FnDef(def_id, generic_args) => {
                        let def_id = *def_id;
                        /* NOTE: As the bodies are still polymorphic, it is still possible that the
                         * instance is not resolvable. Thus we cannot use `expect_resolve` to decide
                         * at this point. */
                        // let is_dynamic = UNKNOWN;

                        if let Some(item) = as_possibly_dynamic_method_call(tcx, def_id) {
                            let receiver = first_arg.unwrap_or_else(|| {
                                panic!("Expected receiver for a dyn-compatible method: {:?}", fn_ty)
                            });
                            let self_ty = generic_args.type_at(0);
                            def_of_possibly_virtual_callee(
                                tcx,
                                call_adder,
                                typing_env,
                                fn_value,
                                item.def_id,
                                self_ty,
                                receiver,
                            )
                        } else {
                            def_of_static_callee(tcx, call_adder, fn_value)
                        }
                    }
                    TyKind::FnPtr(..) => def_of_static_callee(tcx, call_adder, fn_value),
                    _ => {
                        unreachable!("Unexpected type of callee: {:?}: {:?}", fn_value, fn_ty);
                    }
                }
            }

            fn def_of_static_callee<'tcx>(
                tcx: TyCtxt<'tcx>,
                call_adder: &mut (
                         impl BodyLocalManager<'tcx> + MirCallAdder<'tcx> + PriItemsProvider<'tcx>
                     ),
                fn_value: Operand<'tcx>,
            ) -> BlocksAndResult<'tcx> {
                let (fn_ptr_ty, fn_ptr_local, ptr_assignment) =
                    to_fn_ptr(tcx, call_adder, fn_value);

                let (mut block, id_local) = call_adder.make_bb_for_helper_call_with_all(
                    call_adder.pri_helper_funcs().callee_def_static,
                    vec![fn_ptr_ty.into()],
                    vec![operand::move_for_local(fn_ptr_local)],
                    None,
                );
                block.statements.push(ptr_assignment);
                BlocksAndResult::from((block, id_local))
            }

            fn def_of_possibly_virtual_callee<'tcx>(
                tcx: TyCtxt<'tcx>,
                call_adder: &mut (
                         impl BodyLocalManager<'tcx> + MirCallAdder<'tcx> + PriItemsProvider<'tcx>
                     ),
                typing_env: TypingEnv<'tcx>,
                fn_value: Operand<'tcx>,
                method_id: DefId,
                self_ty: Ty<'tcx>,
                receiver: &Operand<'tcx>,
            ) -> BlocksAndResult<'tcx> {
                let (fn_ptr_ty, fn_ptr_local, ptr_assignment) =
                    to_fn_ptr(tcx, call_adder, fn_value);

                let (receiver_raw_ptr_block, receiver_raw_ptr_local) = match receiver {
                    Operand::Copy(place) | Operand::Move(place) => {
                        raw_ptr_of_receiver(tcx, call_adder, typing_env, *place, self_ty)
                    }
                    Operand::Constant(..) => {
                        let receiver_local = call_adder.add_local(receiver.ty(call_adder, tcx));
                        let receiver_assignment = assignment::create(
                            Place::from(receiver_local),
                            Rvalue::Use(receiver.clone()),
                        );
                        let (mut block, local) = raw_ptr_of_receiver(
                            tcx,
                            call_adder,
                            typing_env,
                            receiver_local.into(),
                            self_ty,
                        );
                        block.statements.push(receiver_assignment);
                        (block, local)
                    }
                };

                let identifier = identifier_of_method(tcx, method_id);

                let (mut block, id_local) = call_adder.make_bb_for_helper_call_with_all(
                    call_adder.pri_helper_funcs().callee_def_maybe_virtual,
                    vec![fn_ptr_ty.into(), self_ty.into()],
                    vec![
                        operand::move_for_local(fn_ptr_local),
                        operand::move_for_local(receiver_raw_ptr_local),
                        operand::const_from_uint(tcx, identifier),
                    ],
                    Default::default(),
                );
                block.statements.push(ptr_assignment);
                BlocksAndResult(merge_if_not_call((receiver_raw_ptr_block, block)), id_local)
            }

            fn as_possibly_dynamic_method_call<'tcx>(
                tcx: TyCtxt<'tcx>,
                def_id: DefId,
            ) -> Option<AssocItem> {
                let trait_id = tcx.trait_of_item(def_id)?;
                let item = to_trait_associated_item(tcx, def_id);
                is_vtable_safe_method(tcx, trait_id, item).then_some(item)
            }

            // Receiver -> *const Self
            fn raw_ptr_of_receiver<'tcx>(
                tcx: TyCtxt<'tcx>,
                call_adder: &mut (
                         impl BodyLocalManager<'tcx>
                         + HasLocalDecls<'tcx>
                         + MirCallAdder<'tcx>
                         + PriItemsProvider<'tcx>
                     ),
                typing_env: TypingEnv<'tcx>,
                receiver_place: Place<'tcx>,
                self_ty: Ty<'tcx>,
            ) -> (BasicBlockData<'tcx>, Local) {
                let self_ty = tcx.normalize_erasing_regions(typing_env, self_ty);

                let receiver_ty = receiver_place.ty(call_adder, tcx).ty;
                let receiver_ty = tcx.normalize_erasing_regions(typing_env, receiver_ty);

                let is_self_ref = if let TyKind::Ref(_, pointee, _) = receiver_ty.kind()
                    && *pointee == self_ty
                {
                    true
                } else {
                    false
                };

                let (raw_ptr_block, raw_ptr_local) = if receiver_ty == self_ty || is_self_ref {
                    let local = call_adder.add_local(Ty::new_imm_ptr(tcx, self_ty));
                    let assignment = assignment::create(
                        Place::from(local),
                        Rvalue::RawPtr(
                            mir::RawPtrKind::Const,
                            if is_self_ref {
                                receiver_place.project_deeper(&[ProjectionElem::Deref], tcx)
                            } else {
                                receiver_place
                            },
                        ),
                    );
                    let mut block = BasicBlockData::new(Some(terminator::goto(None)), false);
                    block.statements.push(assignment);
                    (block, local)
                } else {
                    let receiver_ref_local = call_adder.add_local(Ty::new_imm_ref(
                        tcx,
                        tcx.lifetimes.re_erased,
                        receiver_ty,
                    ));
                    let receiver_ref_assignment = assignment::create(
                        receiver_ref_local.into(),
                        rvalue::ref_of(receiver_place, tcx),
                    );
                    let is_pin = tcx
                        .lang_items()
                        .pin_type()
                        .zip(receiver_ty.def_id_for_ty_in_cycle())
                        .is_some_and(|(a, b)| a == b);
                    let converter_func = if is_pin {
                        call_adder.pri_helper_funcs().receiver_pin_to_raw_ptr
                    } else {
                        call_adder.pri_helper_funcs().receiver_to_raw_ptr
                    };
                    let pointee_ty = self_ty;

                    let (mut block, local) = call_adder.make_bb_for_helper_call_with_all(
                        converter_func,
                        vec![pointee_ty.into(), receiver_ty.into()],
                        vec![operand::move_for_local(receiver_ref_local)],
                        Default::default(),
                    );
                    block.statements.push(receiver_ref_assignment);
                    (block, local)
                };

                (raw_ptr_block, raw_ptr_local)
            }

            fn merge_if_not_call<'tcx>(
                mut block_pair: (BasicBlockData<'tcx>, BasicBlockData<'tcx>),
            ) -> Vec<BasicBlockData<'tcx>> {
                if block_pair
                    .0
                    .terminator
                    .as_ref()
                    .unwrap()
                    .kind
                    .as_goto()
                    .is_some_and(|x| x == NEXT_BLOCK)
                {
                    block_pair.1.statements =
                        vec![block_pair.0.statements, block_pair.1.statements].concat();
                    vec![block_pair.1]
                } else {
                    vec![block_pair.0, block_pair.1]
                }
            }

            fn to_fn_ptr<'tcx>(
                tcx: TyCtxt<'tcx>,
                local_manager: &mut impl BodyLocalManager<'tcx>,
                fn_value: Operand<'tcx>,
            ) -> (Ty<'tcx>, Local, Statement<'tcx>) {
                let value_ty = fn_value.ty(local_manager, tcx);
                let fn_ptr_ty = Ty::new_fn_ptr(tcx, ty::fn_ptr_sig(tcx, value_ty));
                let fn_rvalue = if fn_ptr_ty != value_ty {
                    rvalue::cast_to_fn_ptr(tcx, fn_value, value_ty)
                } else {
                    Rvalue::Use(fn_value)
                };
                let fn_ptr_local = local_manager.add_local(fn_ptr_ty);
                let fn_ptr_assign = assignment::create(Place::from(fn_ptr_local), fn_rvalue);
                (fn_ptr_ty, fn_ptr_local, fn_ptr_assign)
            }

            /// Returns the corresponding trait item of a trait method or an implementation of it.
            fn to_trait_associated_item<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> AssocItem {
                let item = tcx.associated_item(def_id);
                if let Some(id) = item.trait_item_def_id
                    && id != def_id
                {
                    to_trait_associated_item(tcx, id)
                } else {
                    item
                }
            }

            fn identifier_of_method<'tcx>(tcx: TyCtxt<'tcx>, method: DefId) -> u64 {
                let method_item = to_trait_associated_item(tcx, method);
                let trait_id = method_item
                    .trait_container(tcx)
                    .expect("Expected a trait method");
                tcx.associated_items(trait_id)
                    .in_definition_order()
                    .position(|item| item == &method_item)
                    .unwrap() as u64
            }

            pub fn are_args_tupled<'tcx: 'a, 'a>(
                tcx: TyCtxt<'tcx>,
                local_manager: &impl HasLocalDecls<'tcx>,
                callee: &Operand<'tcx>,
                args: impl Iterator<Item = &'a Operand<'tcx>>,
                typing_env: mir_ty::TypingEnv<'tcx>,
            ) -> bool {
                // Tupling is only observed in fn trait (closure) calls.
                if !is_fn_trait_method_call(tcx, callee.ty(local_manager, tcx)) {
                    return false;
                }

                // Ensure assumptions that runtime may rely upon.
                let args = args.collect::<Vec<_>>();
                assert_eq!(args.len(), 2);
                assert!(
                    args.last()
                        .unwrap()
                        .ty(local_manager, tcx)
                        .is_tuple(tcx, typing_env),
                    "Fn trait method call without tupled arguments observed. {:?}, {:?}",
                    callee,
                    args.iter()
                        .map(|a| a.ty(local_manager, tcx))
                        .collect::<Vec<_>>()
                );
                true
            }

            pub fn instance_kind_id_operand_triple<'tcx>(
                tcx: TyCtxt<'tcx>,
                instance_kind: InstanceKind<'tcx>,
            ) -> [Operand<'tcx>; 3] {
                let def_id = def_id_operand_pair(tcx, instance_kind.def_id());
                [
                    operand::const_from_uint(tcx, instance_kind.discriminant()),
                    def_id.0,
                    def_id.1,
                ]
            }

            fn def_id_operand_pair<'tcx>(
                tcx: TyCtxt<'tcx>,
                def_id: DefId,
            ) -> (Operand<'tcx>, Operand<'tcx>) {
                (
                    operand::const_from_uint(tcx, def_id.krate.as_u32()),
                    operand::const_from_uint(tcx, def_id.index.as_u32()),
                )
            }

            pub fn is_fn_trait_call_func<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> bool {
                matches!(tcx.def_kind(def_id), rustc_hir::def::DefKind::AssocFn)
                    && to_trait_associated_item(tcx, def_id)
                        .trait_container(tcx)
                        .is_some_and(|trait_id| tcx.is_fn_trait(trait_id))
            }
        }

        pub(super) fn prepare_operand_for_slice<'tcx>(
            tcx: TyCtxt<'tcx>,
            local_manager: &mut (impl BodyLocalManager<'tcx> + HasLocalDecls<'tcx>),
            item_ty: Ty<'tcx>,
            items: Vec<Operand<'tcx>>,
        ) -> (Local, [Statement<'tcx>; 3]) {
            let array_ty = Ty::new_array(tcx, item_ty, items.len() as u64);
            let array_local = local_manager.add_local(array_ty);
            let array_assign =
                assignment::create(Place::from(array_local), rvalue::array(item_ty, items));

            let ref_ty = ty::mk_imm_ref(tcx, array_ty);
            let ref_local = local_manager.add_local(ref_ty);
            let ref_assign = assignment::create(
                Place::from(ref_local),
                rvalue::ref_of(Place::from(array_local), tcx),
            );

            let (cast_local, cast_assign) = cast_array_ref_to_slice(
                tcx,
                local_manager,
                operand::move_for_local(ref_local),
                Some(ref_ty),
            );

            (cast_local, [array_assign, ref_assign, cast_assign])
        }

        pub(super) fn cast_array_ref_to_slice<'tcx>(
            tcx: TyCtxt<'tcx>,
            local_manager: &mut (impl BodyLocalManager<'tcx> + HasLocalDecls<'tcx>),
            ref_operand: Operand<'tcx>,
            operand_ty: Option<Ty<'tcx>>,
        ) -> (Local, Statement<'tcx>) {
            let TyKind::Ref(region, array_ty, mutability) = operand_ty
                .unwrap_or_else(|| ref_operand.ty(local_manager, tcx))
                .kind()
            else {
                panic!("Operand was not from reference type.")
            };
            debug_assert!(array_ty.is_array());
            let item_ty = array_ty.sequence_element_type(tcx);
            let slice_ty = tcx.mk_ty_from_kind(TyKind::Ref(
                *region,
                Ty::new_slice(tcx, item_ty),
                *mutability,
            ));

            let local: Local = local_manager.add_local(slice_ty);
            let assignment = assignment::create(
                Place::from(local),
                rvalue::cast_to_unsize(ref_operand, slice_ty),
            );
            (local, assignment)
        }

        #[allow(clippy::borrowed_box)]
        pub(super) fn cast_int_to_bit_rep<'tcx>(
            tcx: TyCtxt<'tcx>,
            context: &mut impl BodyLocalManager<'tcx>,
            constant: &Box<ConstOperand<'tcx>>,
        ) -> (Local, [Statement<'tcx>; 1]) {
            debug_assert!(constant.ty().is_integral());
            let bit_rep_local = context.add_local(tcx.types.u128);
            let bit_rep_assign = assignment::create(
                Place::from(bit_rep_local),
                Rvalue::Cast(
                    mir::CastKind::IntToInt,
                    operand::const_from_existing(constant),
                    tcx.types.u128,
                ),
            );
            (bit_rep_local, [bit_rep_assign])
        }

        #[allow(clippy::borrowed_box)]
        pub(super) fn cast_float_to_bit_rep<'tcx>(
            tcx: TyCtxt<'tcx>,
            context: &mut (impl BodyLocalManager<'tcx> + PriItemsProvider<'tcx> + SourceInfoProvider),
            constant: &Box<ConstOperand<'tcx>>,
        ) -> (Local, BasicBlockData<'tcx>) {
            use rustc_middle::ty::FloatTy;
            debug_assert!(constant.ty().is_floating_point());
            let bit_rep_local = context.add_local(tcx.types.u128);
            let conversion_func = if matches!(constant.ty().kind(), TyKind::Float(FloatTy::F32)) {
                context.pri_helper_funcs().f32_to_bits
            } else {
                context.pri_helper_funcs().f64_to_bits
            };
            let block = BasicBlockData::new(
                Some(terminator::call(
                    tcx,
                    conversion_func.def_id,
                    iter::empty(),
                    vec![operand::const_from_existing(constant)],
                    bit_rep_local.into(),
                    None,
                    context.source_info(),
                )),
                false,
            );
            (bit_rep_local, block)
        }

        pub(super) fn ptr_to_place<'tcx>(
            tcx: TyCtxt<'tcx>,
            local_manager: &mut impl BodyLocalManager<'tcx>,
            place: &Place<'tcx>,
            place_ty: Ty<'tcx>,
        ) -> (Statement<'tcx>, Local) {
            let ptr_local = local_manager.add_local(Ty::new_imm_ptr(tcx, place_ty));
            let ptr_assignment = assignment::create(
                Place::from(ptr_local),
                mir::Rvalue::RawPtr(mir::RawPtrKind::Const, place.clone()),
            );

            (ptr_assignment, ptr_local)
        }

        pub(super) fn convert_mir_binop_to_pri(op: &mir::BinOp) -> common::pri::BinaryOp {
            // FIXME: #197: Add support for unchecked operations.
            match op {
                mir::BinOp::Add => common::pri::BinaryOp::ADD,
                mir::BinOp::AddUnchecked => common::pri::BinaryOp::ADD_UNCHECKED,
                mir::BinOp::AddWithOverflow => common::pri::BinaryOp::ADD_WITH_OVERFLOW,
                mir::BinOp::Sub => common::pri::BinaryOp::SUB,
                mir::BinOp::SubUnchecked => common::pri::BinaryOp::SUB_UNCHECKED,
                mir::BinOp::SubWithOverflow => common::pri::BinaryOp::SUB_WITH_OVERFLOW,
                mir::BinOp::Mul => common::pri::BinaryOp::MUL,
                mir::BinOp::MulUnchecked => common::pri::BinaryOp::MUL_UNCHECKED,
                mir::BinOp::MulWithOverflow => common::pri::BinaryOp::MUL_WITH_OVERFLOW,
                mir::BinOp::Div => common::pri::BinaryOp::DIV,
                mir::BinOp::Rem => common::pri::BinaryOp::REM,
                mir::BinOp::BitXor => common::pri::BinaryOp::BIT_XOR,
                mir::BinOp::BitAnd => common::pri::BinaryOp::BIT_AND,
                mir::BinOp::BitOr => common::pri::BinaryOp::BIT_OR,
                mir::BinOp::Shl => common::pri::BinaryOp::SHL,
                mir::BinOp::ShlUnchecked => common::pri::BinaryOp::SHL_UNCHECKED,
                mir::BinOp::Shr => common::pri::BinaryOp::SHR,
                mir::BinOp::ShrUnchecked => common::pri::BinaryOp::SHR_UNCHECKED,
                mir::BinOp::Eq => common::pri::BinaryOp::EQ,
                mir::BinOp::Lt => common::pri::BinaryOp::LT,
                mir::BinOp::Le => common::pri::BinaryOp::LE,
                mir::BinOp::Ne => common::pri::BinaryOp::NE,
                mir::BinOp::Ge => common::pri::BinaryOp::GE,
                mir::BinOp::Gt => common::pri::BinaryOp::GT,
                mir::BinOp::Cmp => common::pri::BinaryOp::CMP,
                mir::BinOp::Offset => common::pri::BinaryOp::OFFSET,
            }
        }

        pub(super) fn convert_mir_unop_to_pri(op: &mir::UnOp) -> common::pri::UnaryOp {
            match op {
                mir::UnOp::Not => common::pri::UnaryOp::NOT,
                mir::UnOp::Neg => common::pri::UnaryOp::NEG,
                mir::UnOp::PtrMetadata => common::pri::UnaryOp::PTR_METADATA,
            }
        }

        pub(super) fn is_fn_trait_method_call(tcx: TyCtxt, func_ty: Ty) -> bool {
            let TyKind::FnDef(def_id, ..) = func_ty.kind() else {
                return false;
            };
            tcx.trait_of_item(*def_id)
                .is_some_and(|id| tcx.is_fn_trait(id))
        }

        impl FunctionInfo {
            pub(super) fn num_inputs<'tcx>(&self, tcx: TyCtxt<'tcx>) -> usize {
                tcx.fn_sig(self.def_id)
                    .skip_binder()
                    .inputs()
                    .skip_binder()
                    .len()
            }

            pub(super) fn ret_ty<'tcx>(
                &self,
                tcx: TyCtxt<'tcx>,
                generic_args: &[GenericArg<'tcx>],
            ) -> Ty<'tcx> {
                // FIXME: Check if additional caching can be beneficial
                tcx.fn_sig(self.def_id)
                    .instantiate(tcx, generic_args)
                    .output()
                    .no_bound_vars()
                    .expect(
                        "PRI functions are not expected to have bound vars (generics) for the return type.",
                    )
            }
        }
    }

    /*
     * Context requirements work as aliases for context traits to guarantee that a
     * certain feature will be available in `RuntimeCallAdder` when its context
     * implement that set of traits.
     */
    pub(crate) mod context_requirements {
        use super::{context::*, *};

        pub(crate) trait Basic<'tcx>: BaseContext<'tcx> + BodyProvider<'tcx> {}
        impl<'tcx, C> Basic<'tcx> for C where C: BaseContext<'tcx> + BodyProvider<'tcx> {}

        pub(crate) trait ForInsertion<'tcx>:
            Basic<'tcx> + InsertionLocationProvider + SourceInfoProvider
        {
        }
        impl<'tcx, C> ForInsertion<'tcx> for C where
            C: Basic<'tcx> + InsertionLocationProvider + SourceInfoProvider
        {
        }

        pub(crate) trait ForPlaceRef<'tcx>: ForInsertion<'tcx> {}
        impl<'tcx, C> ForPlaceRef<'tcx> for C where C: ForInsertion<'tcx> {}

        pub(crate) trait ForOperandRef<'tcx>: ForPlaceRef<'tcx> {}
        impl<'tcx, C> ForOperandRef<'tcx> for C where C: ForPlaceRef<'tcx> {}

        pub(crate) trait ForAssignment<'tcx>:
            ForInsertion<'tcx> + AssignmentInfoProvider<'tcx>
        {
        }
        impl<'tcx, C> ForAssignment<'tcx> for C where C: ForInsertion<'tcx> + AssignmentInfoProvider<'tcx> {}

        pub(crate) trait ForCasting<'tcx>:
            CastOperandProvider + ForAssignment<'tcx>
        {
        }
        impl<'tcx, C> ForCasting<'tcx> for C where C: CastOperandProvider + ForAssignment<'tcx> {}

        pub(crate) trait ForBranching<'tcx>:
            ForInsertion<'tcx> + BlockOriginalIndexProvider + JumpTargetModifier
        {
        }
        impl<'tcx, C> ForBranching<'tcx> for C where
            C: ForInsertion<'tcx> + BlockOriginalIndexProvider + JumpTargetModifier
        {
        }

        pub(crate) trait ForAssertion<'tcx>:
            ForOperandRef<'tcx> + BlockOriginalIndexProvider
        {
        }
        impl<'tcx, C> ForAssertion<'tcx> for C where C: ForOperandRef<'tcx> + BlockOriginalIndexProvider {}

        pub(crate) trait ForFunctionCalling<'tcx>:
            ForInsertion<'tcx> + JumpTargetModifier + BlockOriginalIndexProvider
        {
        }
        impl<'tcx, C> ForFunctionCalling<'tcx> for C where
            C: ForInsertion<'tcx> + JumpTargetModifier + BlockOriginalIndexProvider
        {
        }

        pub(crate) trait ForReturning<'tcx>: ForInsertion<'tcx> {}
        impl<'tcx, C> ForReturning<'tcx> for C where C: ForInsertion<'tcx> {}

        pub(crate) trait ForEntryFunction<'tcx>:
            ForInsertion<'tcx> + InEntryFunction
        {
        }
        impl<'tcx, C> ForEntryFunction<'tcx> for C where C: ForInsertion<'tcx> + InEntryFunction {}

        pub(crate) trait ForAtomicIntrinsic<'tcx>:
            ForInsertion<'tcx> + AtomicIntrinsicParamsProvider<'tcx> + PointerInfoProvider<'tcx>
        {
        }
        impl<'tcx, C> ForAtomicIntrinsic<'tcx> for C where
            C: ForInsertion<'tcx> + AtomicIntrinsicParamsProvider<'tcx> + PointerInfoProvider<'tcx>
        {
        }

        pub(crate) trait ForMemoryIntrinsic<'tcx>:
            ForAssignment<'tcx>
            + MemoryIntrinsicParamsProvider<'tcx>
            + PointerInfoProvider<'tcx>
        {
        }
        impl<'tcx, C> ForMemoryIntrinsic<'tcx> for C where
            C: ForAssignment<'tcx>
                + MemoryIntrinsicParamsProvider<'tcx>
                + PointerInfoProvider<'tcx>
        {
        }
    }
}

pub(super) use implementation::context_requirements as ctxtreqs;

use crate::pri_utils::sym::intrinsics::LeafIntrinsicSymbol;
