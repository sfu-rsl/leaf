/// This module contains the traits and implementations for adding calls to the
/// PRI in MIR bodies.
pub(super) mod context;

use rustc_middle::{
    mir::{
        BasicBlock, BinOp, Body, CastKind, ConstOperand, Local, Operand, Place, ProjectionElem,
        Statement, UnOp,
    },
    ty::{Const, GenericArg, Ty, TyCtxt},
};
use rustc_span::def_id::DefId;
use rustc_target::abi::{FieldIdx, VariantIdx};

use common::{log_debug, log_warn};
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
        #[derive(Clone, Copy)]
        pub struct $name(Local);
        impl From<Local> for $name {
            fn from(value: Local) -> Self {
                Self(value)
            }
        }
        impl From<$name> for Local {
            fn from(value: $name) -> Self {
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

pub(crate) trait Assigner<'tcx> {
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

#[derive(Clone, Copy)]
pub struct SwitchInfo<'tcx> {
    pub(super) node_index: BasicBlock,
    pub(super) discr_ty: Ty<'tcx>,
    pub(super) runtime_info_store_var: Local,
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
    fn reference_func(&mut self, func: &Operand<'tcx>) -> OperandRef;

    fn reference_special_func(&mut self, func: &Operand<'tcx>) -> OperandRef;

    fn before_call_func(
        &mut self,
        func: OperandRef,
        arguments: impl Iterator<Item = OperandRef>,
        are_args_tupled: bool,
    );

    fn enter_func(&mut self);

    fn return_from_func(&mut self);

    fn after_call_func(&mut self, destination: &Place<'tcx>);
}

pub(crate) trait IntrinsicHandler<'tcx> {
    fn perform_intrinsic_by(
        &mut self,
        intrinsic_func: DefId,
        pri_func: LeafIntrinsicSymbol,
        args: impl Iterator<Item = OperandRef>,
    );
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
    use core::intrinsics::unlikely;
    use std::assert_matches::{assert_matches, debug_assert_matches};

    use rustc_middle::mir::{self, BasicBlock, BasicBlockData, HasLocalDecls, UnevaluatedConst};
    use rustc_middle::ty::{self as mir_ty, TyKind, TypeVisitableExt};

    use delegate::delegate;

    use crate::mir_transform::*;
    use crate::passes::Storage;
    use crate::pri_utils::{
        sym::{self, LeafSymbol},
        FunctionInfo,
    };

    use self::ctxtreqs::*;
    use self::utils::ty::TyExt;
    use super::context::*;
    use super::*;

    use utils::*;
    use InsertionLocation::*;

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
        ) -> RuntimeCallAdder<InBodyContext<'b, 'tcx, 'bd, C>> {
            self.with_context(|base| InBodyContext { base, body })
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
            dest_ref: PlaceRef,
            dest_ty: Ty<'tcx>,
        ) -> RuntimeCallAdder<AssignmentContext<'_, 'tcx, C>> {
            self.with_context(|base| AssignmentContext {
                base,
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
        C: context::BodyProvider<'tcx> + context::TyContextProvider<'tcx>,
    {
        fn current_func(&self) -> Operand<'tcx> {
            let kind = self.context.body().source.instance;
            let def_id = kind.def_id();
            log_debug!("Creating operand of current function: {:?}", def_id);

            assert_matches!(
                kind,
                rustc_middle::ty::InstanceKind::Item(..),
                "Only user-defined items are expected."
            );

            let tcx = self.tcx();

            let ty = tcx.type_of(def_id).instantiate_identity();
            let fn_def_ty = match ty.kind() {
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
            };
            debug_assert_matches!(fn_def_ty.kind(), TyKind::FnDef(..));

            Operand::Constant(Box::new(mir::ConstOperand {
                span: self.context.body().span,
                user_ty: None,
                const_: mir::Const::zero_sized(fn_def_ty),
            }))
        }

        fn current_param_env(&self) -> mir_ty::ParamEnv<'tcx> {
            self.context.tcx().param_env(self.current_func_id())
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
            let result_local = self
                .context
                .add_local((func_info.ret_ty(self.tcx()), self.context.source_info()));
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
            BasicBlockData::new(Some(terminator::call(
                self.context.tcx(),
                func_id,
                generic_args,
                args,
                destination,
                target,
                self.context.source_info(),
            )))
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

                if cfg!(place_addr) {
                    cum_place = cum_place.project_deeper(&[proj], tcx);
                    cum_ty = cum_ty.projection_ty(tcx, proj);

                    blocks.extend(self.set_place_type(place_ref, cum_ty.ty));
                    blocks.push(self.set_place_addr(place_ref, &cum_place, cum_ty.ty));
                }
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

            if cfg!(place_addr) {
                let ty = self.local_decls()[local].ty;
                blocks.extend(self.set_place_type(place_ref, ty));
                blocks.push(self.set_place_addr(place_ref, &local.into(), ty));
            }

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
                ProjectionElem::Field(index, _) => (
                    sym::ref_place_field,
                    vec![operand::const_from_uint(
                        self.context.tcx(),
                        u32::from(index),
                    )],
                ),
                ProjectionElem::Index(index) => {
                    let BlocksAndResult(additional_blocks, index_ref) =
                        self.internal_reference_place(&Place::from(index));
                    new_blocks.extend(additional_blocks);
                    (
                        sym::ref_place_index,
                        vec![operand::copy_for_local(index_ref)],
                    )
                }
                ProjectionElem::ConstantIndex {
                    offset,
                    min_length,
                    from_end,
                } => (
                    sym::ref_place_constant_index,
                    vec![
                        operand::const_from_uint(self.context.tcx(), offset),
                        operand::const_from_uint(self.context.tcx(), min_length),
                        operand::const_from_bool(self.context.tcx(), from_end),
                    ],
                ),
                ProjectionElem::Subslice { from, to, from_end } => (
                    sym::ref_place_subslice,
                    vec![
                        operand::const_from_uint(self.context.tcx(), from),
                        operand::const_from_uint(self.context.tcx(), to),
                        operand::const_from_bool(self.context.tcx(), from_end),
                    ],
                ),
                ProjectionElem::Downcast(_, index) => (
                    sym::ref_place_downcast,
                    vec![operand::const_from_uint(
                        self.context.tcx(),
                        u32::from(index),
                    )],
                ),
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
            if unlikely(!place_ty.is_sized(self.tcx(), self.current_param_env())) {
                log_warn!("Encountered unsized type. Skipping size setting.");
                return vec![BasicBlockData::new(Some(terminator::goto(None)))];
            }

            let (get_call_block, size_local) = self.make_bb_for_helper_call_with_all(
                self.context.pri_helper_funcs().size_of,
                vec![place_ty.into()],
                Vec::default(),
                None,
            );

            let set_call_block = self.make_bb_for_call(
                sym::set_place_size,
                vec![
                    operand::copy_for_local(place_ref),
                    operand::move_for_local(size_local),
                ],
            );

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
                Some((
                    sym::set_place_type_int,
                    vec![
                        operand::const_from_uint(tcx, ty.primitive_size(tcx).bits()),
                        operand::const_from_bool(tcx, ty.is_signed()),
                    ],
                ))
            } else if ty.is_floating_point() {
                let (e_bits, s_bits) = ty::ebit_sbit_size(ty);
                Some((
                    sym::set_place_type_float,
                    vec![
                        operand::const_from_uint(tcx, e_bits),
                        operand::const_from_uint(tcx, s_bits),
                    ],
                ))
            } else {
                None
            } {
                blocks.push(self.make_bb_for_call(
                    func_name,
                    [vec![operand::copy_for_local(place_ref)], additional_args].concat(),
                ));
            }

            #[cfg(place_addr)]
            let id_local = {
                let (block, id_local) = self.make_type_id_of_bb(ty);
                blocks.push(block);
                id_local
            };

            #[cfg(place_addr)]
            blocks.push(self.make_bb_for_call(
                sym::set_place_type_id,
                vec![
                    operand::copy_for_local(place_ref),
                    operand::move_for_local(id_local),
                ],
            ));

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

            if cfg!(place_addr) {
                let tcx = self.tcx();
                let ty = place.ty(&self.context, tcx).ty;
                if ty.is_adt() || ty.is_trivially_tuple() || ty.is_array() || !ty.is_known_rigid() {
                    additional_blocks.extend(self.set_place_size(place_ref, ty));
                }
            }

            let func_name = if is_copy {
                sym::ref_operand_copy
            } else {
                sym::ref_operand_move
            };

            BlocksAndResult::from(
                self.make_bb_for_operand_ref_call(
                    func_name,
                    vec![operand::copy_for_local(place_ref)],
                ),
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
            } else if cfg!(abs_concrete) {
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
            else if ty::size_of(tcx, ty, self.current_param_env()) == rustc_abi::Size::ZERO {
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
            self.make_bb_for_operand_ref_call(
                func_name,
                vec![operand::const_from_existing(constant)],
            )
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
            let (mut block, result) = self.make_bb_for_operand_ref_call(
                sym::ref_operand_const_int,
                vec![
                    operand::move_for_local(bit_rep_local),
                    operand::const_from_uint(tcx, ty.primitive_size(tcx).bits()),
                    operand::const_from_bool(tcx, ty.is_signed()),
                ],
            );
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
            let block_pair = self.make_bb_for_operand_ref_call(
                sym::ref_operand_const_float,
                vec![
                    operand::move_for_local(bit_rep_local),
                    operand::const_from_uint(tcx, e_bits),
                    operand::const_from_uint(tcx, s_bits),
                ],
            );
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
            let mut block_pair = self.make_bb_for_operand_ref_call(
                sym::ref_operand_const_byte_str,
                vec![operand::move_for_local(slice_local)],
            );
            block_pair.0.statements.insert(0, slice_assignment);
            BlocksAndResult::from(block_pair)
        }

        fn internal_reference_func_def_const_operand(
            &mut self,
            constant: &Box<ConstOperand<'tcx>>,
        ) -> BlocksAndResult<'tcx> {
            self.internal_reference_func(&Operand::Constant(constant.clone()))
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
        type Cast<'b> = RuntimeCallAdder<CastAssignmentContext<'b, C>>
            where CastAssignmentContext<'b, C>: ForCasting<'tcx> + 'b;

        fn by_use(&mut self, operand: OperandRef) {
            self.add_bb_for_assign_call(
                sym::assign_use,
                vec![operand::copy_for_local(operand.into())],
            )
        }

        fn by_repeat(&mut self, operand: OperandRef, count: &Const<'tcx>) {
            self.add_bb_for_assign_call(
                sym::assign_repeat,
                vec![
                    operand::copy_for_local(operand.into()),
                    #[allow(clippy::clone_on_copy)]
                    operand::const_from_existing_ty_const(self.tcx().types.usize, count.clone()),
                ],
            )
        }

        fn by_ref(&mut self, place: PlaceRef, is_mutable: bool) {
            self.add_bb_for_assign_call(
                sym::assign_ref,
                vec![
                    operand::copy_for_local(place.into()),
                    operand::const_from_bool(self.context.tcx(), is_mutable),
                ],
            )
        }

        fn by_thread_local_ref(&mut self, _def_id: &DefId) {
            if cfg!(abs_concrete) {
                self.to_some_concrete()
            } else {
                self.add_bb_for_assign_call(sym::assign_thread_local_ref, vec![])
            }
        }

        fn by_raw_ptr(&mut self, place: PlaceRef, is_mutable: bool) {
            self.add_bb_for_assign_call(
                sym::assign_raw_ptr_of,
                vec![
                    operand::copy_for_local(place.into()),
                    operand::const_from_bool(self.context.tcx(), is_mutable),
                ],
            )
        }

        fn by_len(&mut self, place: PlaceRef) {
            self.add_bb_for_assign_call(
                sym::assign_len,
                vec![operand::copy_for_local(place.into())],
            )
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

            self.add_bb_for_assign_call(
                sym::assign_binary_op,
                vec![
                    operand::move_for_local(operator_local),
                    operand::copy_for_local(first.into()),
                    operand::copy_for_local(second.into()),
                ],
            )
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

            self.add_bb_for_assign_call(
                sym::assign_unary_op,
                vec![
                    operand::move_for_local(operator_local),
                    operand::copy_for_local(operand.into()),
                ],
            )
        }

        fn by_discriminant(&mut self, place: PlaceRef) {
            self.add_bb_for_assign_call(
                sym::assign_discriminant,
                vec![operand::copy_for_local(place.into())],
            )
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
            self.add_bb_for_adt_assign_call(
                sym::assign_aggregate_enum,
                fields,
                vec![operand::const_from_uint(
                    self.context.tcx(),
                    variant.as_u32(),
                )],
            )
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
            self.add_bb_for_assign_call(
                sym::assign_shallow_init_box,
                vec![
                    operand::copy_for_local(operand.into()),
                    operand::move_for_local(id_local),
                ],
            );
        }

        fn its_discriminant_to(&mut self, variant_index: &VariantIdx) {
            self.add_bb_for_assign_call(
                sym::set_discriminant,
                vec![operand::const_from_uint(
                    self.context.tcx(),
                    variant_index.as_u32(),
                )],
            )
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
                    vec![operand::copy_for_local(self.context.dest_ref().into())],
                    args,
                ]
                .concat(),
            )
        }

        fn add_and_set_local_for_enum(
            &mut self,
            enum_ty: Ty<'tcx>,
            discr: impl Into<u128>,
        ) -> (Local, Vec<Statement<'tcx>>) {
            let local = self.context.add_local(enum_ty);
            let statements = enums::set_variant_to_local(self.tcx(), enum_ty, discr.into(), local);
            (local, statements)
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

                self.add_bb_for_cast_assign_call_with_args(
                    sym::assign_cast_integer,
                    vec![
                        operand::const_from_uint(tcx, bits),
                        operand::const_from_bool(tcx, is_signed),
                    ],
                )
            }
        }

        fn to_float(&mut self, ty: Ty<'tcx>) {
            let (e_bits, s_bits) = ty::ebit_sbit_size(ty);
            self.add_bb_for_cast_assign_call_with_args(
                sym::assign_cast_float,
                vec![
                    operand::const_from_uint(self.context.tcx(), e_bits),
                    operand::const_from_uint(self.context.tcx(), s_bits),
                ],
            )
        }

        fn through_unsizing(&mut self) {
            self.add_bb_for_cast_assign_call(sym::assign_cast_unsize)
        }

        fn through_fn_ptr_coercion(&mut self) {
            if cfg!(abs_concrete) {
                self.to_some_concrete()
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
            use rustc_middle::ty::adjustment::PointerCoercion::*;
            use CastKind::*;
            debug_assert_matches!(
                kind,
                PtrToPtr | FnPtrToPtr | PointerCoercion(MutToConstPointer | ArrayToPointer)
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
            self.add_bb_for_cast_assign_call_with_args(
                sym::assign_cast_transmute,
                vec![operand::move_for_local(id_local)],
            )
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
            self.add_bb_for_cast_assign_call_with_args(
                func_name,
                vec![operand::move_for_local(id_local)],
            );
        }
    }

    impl<'tcx, C> BranchingReferencer<'tcx> for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx> + OperandReferencer<'tcx>,
        C: ForBranching<'tcx>,
    {
        fn store_branching_info(&mut self, discr: &Operand<'tcx>) -> SwitchInfo<'tcx> {
            let tcx = self.context.tcx();
            let operand_ref = self.reference_operand(discr);
            let ty = discr.ty(self.context.local_decls(), tcx);
            let discr_size = ty.primitive_size(tcx).bits();
            let node_index = self.context.block_index();
            let (block, info_store_var) = self.make_bb_for_call_with_ret(
                sym::new_branching_info,
                vec![
                    operand::const_from_uint(tcx, u32::from(node_index)),
                    operand::copy_for_local(operand_ref.into()),
                    operand::const_from_uint(tcx, discr_size),
                    operand::const_from_bool(tcx, ty.is_signed()),
                ],
            );
            self.insert_blocks([block]);
            SwitchInfo {
                node_index,
                discr_ty: ty,
                runtime_info_store_var: info_store_var,
            }
        }
    }

    impl<'tcx, C> BranchingHandler for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: SwitchInfoProvider<'tcx> + ForBranching<'tcx>,
    {
        fn take_by_value(&mut self, value: u128) {
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
                // TODO: Distinguish enum discriminant
                (
                    sym::take_branch_int,
                    vec![operand::const_from_uint(self.context.tcx(), value)],
                )
            } else if discr_ty.is_char() {
                (
                    sym::take_branch_char,
                    vec![operand::const_from_char(
                        self.context.tcx(),
                        char::from_u32(value.try_into().unwrap()).unwrap(),
                    )],
                )
            } else {
                unreachable!(
                    "Branching node discriminant is supposed to be either bool, int, char, or enum discriminant."
                )
            };

            let block = self.make_bb_for_call_with_target(
                func_name,
                [
                    vec![operand::move_for_local(switch_info.runtime_info_store_var)],
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
            let switch_info = self.context.switch_info();
            let discr_ty = switch_info.discr_ty;
            let (additional_stmts, func_name, additional_args) = if discr_ty.is_bool() {
                let mut non_values = non_values.into_iter();
                debug_assert!(non_values.len() == 1);
                debug_assert!(non_values.next().unwrap() == 0);

                (vec![], sym::take_branch_true, vec![])
            } else {
                let tcx = self.context.tcx();

                let (func_name, value_type, value_to_operand): (
                    LeafSymbol,
                    Ty<'tcx>,
                    Box<dyn Fn(u128) -> Operand<'tcx>>,
                ) = if discr_ty.is_integral() {
                    // TODO: Distinguish enum discriminant
                    (
                        sym::take_branch_ow_int,
                        tcx.types.u128,
                        Box::new(|nv: u128| operand::const_from_uint(tcx, nv)),
                    )
                } else if discr_ty.is_char() {
                    (
                        sym::take_branch_ow_char,
                        tcx.types.char,
                        Box::new(|nv: u128| {
                            operand::const_from_char(
                                tcx,
                                char::from_u32(nv.try_into().unwrap()).unwrap(),
                            )
                        }),
                    )
                } else {
                    unreachable!(
                        "Branching node discriminant is supposed to be either bool, int, char, or enum discriminant."
                    )
                };

                let (non_values_local, assign_stmts) = self.add_and_assign_local_for_ow_non_values(
                    non_values.into_iter(),
                    value_type,
                    value_to_operand,
                );
                (
                    assign_stmts.to_vec(),
                    func_name,
                    vec![operand::move_for_local(non_values_local)],
                )
            };

            let mut block = self.make_bb_for_call_with_target(
                func_name,
                [
                    vec![operand::move_for_local(switch_info.runtime_info_store_var)],
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
    impl<'tcx, C> RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: SwitchInfoProvider<'tcx> + ForBranching<'tcx>,
    {
        fn add_and_assign_local_for_ow_non_values(
            &mut self,
            non_values: impl ExactSizeIterator<Item = u128>,
            value_ty: Ty<'tcx>,
            value_to_operand: impl Fn(u128) -> Operand<'tcx>,
        ) -> (Local, [Statement<'tcx>; 3]) {
            prepare_operand_for_slice(
                self.context.tcx(),
                &mut self.context,
                value_ty,
                non_values.map(value_to_operand).collect(),
            )
        }
    }

    impl<'tcx, C> FunctionHandler<'tcx> for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForFunctionCalling<'tcx>,
    {
        fn reference_func(&mut self, func: &Operand<'tcx>) -> OperandRef {
            self.debug_info(&format!("{}", func.ty(self, self.tcx())));
            let BlocksAndResult(new_blocks, ref_local) = self.internal_reference_func(func);
            self.insert_blocks(new_blocks);
            ref_local.into()
        }

        fn reference_special_func(&mut self, func: &Operand<'tcx>) -> OperandRef {
            self.debug_info(&format!("{}", func.ty(self, self.tcx())));
            // Use `special_func_placeholder` instead.
            let func = operand::func(
                self.tcx(),
                *self.pri_helper_funcs().special_func_placeholder,
                iter::empty(),
            );
            let BlocksAndResult(new_blocks, ref_local) = self.internal_reference_func(&func);
            self.insert_blocks(new_blocks);
            ref_local.into()
        }

        fn before_call_func(
            &mut self,
            func: OperandRef,
            arguments: impl Iterator<Item = OperandRef>,
            are_args_tupled: bool,
        ) {
            let tcx = self.tcx();
            let operand_ref_ty = self.context.pri_types().operand_ref(tcx);
            let (arguments_local, additional_stmts) = prepare_operand_for_slice(
                tcx,
                &mut self.context,
                operand_ref_ty,
                arguments
                    .map(|a| operand::copy_for_local(a.into()))
                    .collect(),
            );
            let mut block = self.make_bb_for_call(
                sym::before_call_func,
                vec![
                    operand::copy_for_local(func.into()),
                    operand::move_for_local(arguments_local),
                    operand::const_from_bool(tcx, are_args_tupled),
                ],
            );
            block.statements.extend(additional_stmts);
            debug_assert_matches!(
                self.context.insertion_loc(),
                Before(..),
                "Inserting before_call after a block is not expected."
            );
            self.insert_blocks([block]);
        }

        fn enter_func(&mut self) {
            let mut blocks = vec![];

            if cfg!(place_addr) {
                blocks.extend(self.make_bb_for_func_preserve_metadata());
            }

            let tcx = self.tcx();

            if let TyKind::Closure(_, args) = tcx
                .type_of(self.current_func_id())
                .instantiate_identity()
                .kind()
            {
                blocks.extend(self.make_bb_for_try_untuple_args_in_closure(args.as_closure()));
            }

            let func_ref = self.reference_func(&self.current_func());

            blocks.push(self.make_bb_for_call(
                sym::enter_func,
                vec![operand::copy_for_local(func_ref.into())],
            ));
            self.insert_blocks(blocks);
        }

        fn return_from_func(&mut self) {
            let block = self.make_bb_for_call(sym::return_from_func, vec![]);
            self.insert_blocks([block]);
        }

        fn after_call_func(&mut self, destination: &Place<'tcx>) {
            // we want the place reference to be after the function call as well
            let BlocksAndResult(mut blocks, dest_ref) = self.internal_reference_place(destination);
            let after_call_block = self.make_bb_for_call(
                sym::after_call_func,
                vec![operand::copy_for_local(dest_ref)],
            );
            blocks.push(after_call_block);
            debug_assert_matches!(
                self.context.insertion_loc(),
                After(..),
                "Inserting after_call before a block is not expected."
            );
            self.insert_blocks(blocks);
        }
    }

    impl<'tcx, C> RuntimeCallAdder<C>
    where
        C: Basic<'tcx>,
    {
        pub(crate) fn are_args_tupled<'a>(
            &self,
            callee: &Operand<'tcx>,
            args: impl Iterator<Item = &'a Operand<'tcx>>,
        ) -> bool
        where
            'tcx: 'a,
        {
            let tcx = self.tcx();
            utils::are_args_tupled(tcx, &self.context, callee, args, self.current_param_env())
        }
    }

    impl<'tcx, C> RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: Basic<'tcx> + SourceInfoProvider,
    {
        fn make_bb_for_func_preserve_metadata(&mut self) -> Vec<BasicBlockData<'tcx>>
        where
            C: ForPlaceRef<'tcx>,
        {
            let mut blocks = vec![];

            let preserve = |this: &mut Self, place_ref: Local| {
                this.make_bb_for_call(
                    sym::preserve_special_local_metadata,
                    vec![operand::move_for_local(place_ref)],
                )
            };

            blocks.extend(self.body().args_iter().flat_map(|local| {
                let BlocksAndResult(mut ref_blocks, place_ref) =
                    self.internal_reference_place(&Place::from(local));
                ref_blocks.push(preserve(self, place_ref));
                ref_blocks
            }));

            let ret_ty = self.body().return_ty();
            if !ret_ty.is_unit() {
                let BlocksAndResult(mut ref_blocks, place_ref) =
                    self.internal_reference_place(&Place::return_place());
                ref_blocks.extend(self.set_place_size(place_ref, ret_ty));
                ref_blocks.push(preserve(self, place_ref));
                blocks.extend(ref_blocks);
            }

            blocks
        }

        fn make_bb_for_try_untuple_args_in_closure(
            &mut self,
            args: mir_ty::ClosureArgs<TyCtxt<'tcx>>,
        ) -> Vec<BasicBlockData<'tcx>> {
            let mut blocks = vec![];

            let tuple_id_local = {
                let (block, id_local) =
                    self.make_type_id_of_bb(ty::erased_tupled_closure_inputs(self.tcx(), args));
                blocks.push(block);
                id_local
            };

            let call_block = self.make_bb_for_call(
                sym::try_untuple_argument,
                vec![
                    operand::const_from_uint(self.tcx(), 2 as common::types::LocalIndex),
                    operand::move_for_local(tuple_id_local),
                ],
            );
            blocks.push(call_block);

            blocks
        }

        fn internal_reference_func(&mut self, func: &Operand<'tcx>) -> BlocksAndResult<'tcx> {
            let tcx = self.tcx();
            log_debug!("Referencing function: {:?}", func.ty(self, tcx).kind());
            let id_ty = self.context.pri_types().func_id(tcx);
            let (stmts, id_local) = id_of_func(tcx, &mut self.context, func, id_ty);
            let (mut new_block, ref_local) = self.make_bb_for_operand_ref_call(
                sym::ref_operand_const_func,
                vec![operand::move_for_local(id_local)],
            );
            new_block.statements.extend(stmts);
            (new_block, ref_local).into()
        }
    }

    impl<'tcx, C> IntrinsicHandler<'tcx> for RuntimeCallAdder<C>
    where
        Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
        C: ForAssignment<'tcx>,
    {
        fn perform_intrinsic_by(
            &mut self,
            intrinsic_func: DefId,
            pri_func: LeafIntrinsicSymbol,
            args: impl Iterator<Item = OperandRef>,
        ) {
            let has_return_value = !self.context.dest_ty().is_unit();
            self.assert_pri_intrinsic_consistency(intrinsic_func, pri_func, has_return_value);

            let pri_name = *pri_func;
            let args = args.map(Into::into).map(operand::move_for_local).collect();
            let block = if has_return_value {
                self.make_bb_for_assign_call(pri_name, args)
            } else {
                self.make_bb_for_call(pri_name, args)
            };
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
            has_return_value: bool,
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
                pri_func_arg_num,
                intrinsic_arg_num + if has_return_value { 1 } else { 0 },
                "Inconsistent number of arguments between intrinsic and its corresponding PRI function. {:?} -x-> {:?}",
                intrinsic_func,
                pri_func_info.def_id
            );
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
            let Some((func_name, mut additional_operands, additional_stmts)) =
                self.reference_assert_kind(msg)
            else {
                return;
            };

            let mut operands = vec![
                operand::move_for_local(cond.into()),
                // this is a compile-time known value, so we can just pass it!
                // NOTE: we could call different functions based on the value of this to improve
                //       performance, but it wouldn't really affect much...
                operand::const_from_bool(self.context.tcx(), expected),
            ];
            operands.append(&mut additional_operands);

            let mut block = self.make_bb_for_call(func_name, operands);
            block.statements.extend(additional_stmts);
            self.insert_blocks([block]);
        }
    }
    impl<'tcx, C> RuntimeCallAdder<C>
    where
        C: ForAssertion<'tcx>,
    {
        fn reference_assert_kind(
            &mut self,
            msg: &rustc_middle::mir::AssertMessage<'tcx>,
        ) -> Option<(LeafSymbol, Vec<Operand<'tcx>>, Vec<Statement<'tcx>>)> {
            use rustc_middle::mir::AssertKind;
            match msg {
                AssertKind::BoundsCheck { len, index } => {
                    let len_ref = self.reference_operand(len);
                    let index_ref = self.reference_operand(index);
                    Some((
                        sym::check_assert_bounds_check,
                        vec![
                            operand::copy_for_local(len_ref.into()),
                            operand::copy_for_local(index_ref.into()),
                        ],
                        vec![],
                    ))
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
                    Some((
                        sym::check_assert_overflow,
                        vec![
                            // TODO: double check that moves and copies here are correct
                            operand::move_for_local(operator_local),
                            operand::copy_for_local(op1_ref.into()),
                            operand::copy_for_local(op2_ref.into()),
                        ],
                        vec![],
                    ))
                }
                AssertKind::OverflowNeg(op) => {
                    let op_ref = self.reference_operand(op);
                    Some((
                        sym::check_assert_overflow_neg,
                        vec![operand::copy_for_local(op_ref.into())],
                        vec![],
                    ))
                }
                AssertKind::DivisionByZero(op) => {
                    let op_ref = self.reference_operand(op);
                    Some((
                        sym::check_assert_div_by_zero,
                        vec![operand::copy_for_local(op_ref.into())],
                        vec![],
                    ))
                }
                AssertKind::RemainderByZero(op) => {
                    let op_ref = self.reference_operand(op);
                    Some((
                        sym::check_assert_rem_by_zero,
                        vec![operand::copy_for_local(op_ref.into())],
                        vec![],
                    ))
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
                    Some((
                        sym::check_assert_misaligned_ptr_deref,
                        vec![
                            operand::copy_for_local(required_ref.into()),
                            operand::copy_for_local(found_ref.into()),
                        ],
                        vec![],
                    ))
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
            let serialized = ron::to_string(info).unwrap();
            let block = self.make_bb_for_call(
                sym::debug_info,
                vec![operand::const_from_byte_str(
                    self.context.tcx(),
                    serialized.as_bytes(),
                )],
            );
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
            ty::{adjustment::PointerCoercion, Ty, TyCtxt, TyKind},
        };
        use rustc_span::DUMMY_SP;

        use crate::mir_transform::{BodyLocalManager, NEXT_BLOCK};
        use crate::utils::mir::TyCtxtExt;

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
                    const_: Const::from_ty_const(
                        mir_ty::Const::new_value(
                            tcx,
                            mir_ty::ValTree::from_raw_bytes(tcx, value),
                            ty,
                        ),
                        ty,
                        tcx,
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
            ) -> Operand {
                Operand::function_handle(tcx, def_id, substs, DUMMY_SP)
            }
        }

        pub(super) mod enums {
            use rustc_target::abi::VariantIdx;

            use super::*;

            pub fn set_variant_to_local<'tcx>(
                tcx: TyCtxt<'tcx>,
                enum_ty: Ty<'tcx>,
                variant_discr: u128,
                local: Local,
            ) -> Vec<Statement<'tcx>> {
                let place = Place::from(local);

                let deinit = Statement {
                    source_info: SourceInfo::outermost(DUMMY_SP),
                    kind: rustc_middle::mir::StatementKind::Deinit(Box::new(place)),
                };

                let disc = Statement {
                    source_info: SourceInfo::outermost(DUMMY_SP),
                    kind: rustc_middle::mir::StatementKind::SetDiscriminant {
                        place: Box::new(place),
                        variant_index: get_variant_index_by_discr(tcx, enum_ty, variant_discr),
                    },
                };

                vec![deinit, disc]
            }

            pub fn get_variant_index_by_discr<'tcx>(
                tcx: TyCtxt<'tcx>,
                ty: Ty<'tcx>,
                discr: u128,
            ) -> VariantIdx {
                let adt_def = match ty.kind() {
                    TyKind::Adt(def, _) => def,
                    _ => unreachable!(),
                };
                adt_def
                    .discriminants(tcx)
                    .find(|(_, d)| d.val == discr)
                    .expect("Discriminant value is not valid.")
                    .0
            }
        }

        pub(super) mod assignment {
            use rustc_middle::mir::StatementKind;

            use super::*;

            pub mod rvalue {
                use rustc_index::IndexVec;
                use rustc_middle::mir::{AggregateKind, BorrowKind, CastKind};

                use super::*;

                pub fn ref_of<'tcx>(target: Place<'tcx>, tcx: TyCtxt<'tcx>) -> Rvalue<'tcx> {
                    Rvalue::Ref(tcx.lifetimes.re_erased, BorrowKind::Shared, target)
                }

                pub fn cast_to_unsize<'tcx>(
                    operand: Operand<'tcx>,
                    to_ty: Ty<'tcx>,
                ) -> Rvalue<'tcx> {
                    cast_to_coerced(PointerCoercion::Unsize, operand, to_ty)
                }

                pub fn cast_to_coerced<'tcx>(
                    coercion: PointerCoercion,
                    operand: Operand<'tcx>,
                    to_ty: Ty<'tcx>,
                ) -> Rvalue<'tcx> {
                    Rvalue::Cast(CastKind::PointerCoercion(coercion), operand, to_ty)
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
            use rustc_middle::ty::ParamEnv;

            use super::*;

            pub trait TyExt<'tcx> {
                fn is_trivially_tuple(self) -> bool;
                fn is_tuple(self, tcx: TyCtxt<'tcx>, param_env: ParamEnv<'tcx>) -> bool;
            }

            impl<'tcx> TyExt<'tcx> for Ty<'tcx> {
                #[inline]
                fn is_trivially_tuple(self) -> bool {
                    matches!(self.kind(), TyKind::Tuple(..))
                }

                fn is_tuple(self, tcx: TyCtxt<'tcx>, param_env: ParamEnv<'tcx>) -> bool {
                    self.is_trivially_tuple()
                        || is_trait(
                            tcx,
                            self,
                            param_env,
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
                param_env: ParamEnv<'tcx>,
            ) -> Size {
                tcx.layout_of(param_env.and(ty)).unwrap().size
            }

            pub fn ebit_sbit_size<'tcx>(ty: Ty<'tcx>) -> (u64, u64) {
                use rustc_apfloat::{ieee::*, Float};
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

            #[inline]
            pub fn is_ref_assignable<'tcx>(ty: &Ty<'tcx>, from: &Ty<'tcx>) -> bool {
                match (ty.kind(), from.kind()) {
                    (TyKind::Ref(_, ty, _), TyKind::Ref(_, from, _)) => is_ref_assignable(ty, from),
                    _ => ty == from,
                }
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

            fn is_trait<'tcx>(
                tcx: TyCtxt<'tcx>,
                ty: Ty<'tcx>,
                param_env: ParamEnv<'tcx>,
                trait_def_id: DefId,
            ) -> bool {
                use rustc_infer::infer::TyCtxtInferExt;
                use rustc_trait_selection::infer::InferCtxtExt;

                tcx.infer_ctxt()
                    .build()
                    .type_implements_trait(trait_def_id, [ty], param_env)
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
            let block = BasicBlockData::new(Some(terminator::call(
                tcx,
                conversion_func,
                iter::empty(),
                vec![operand::const_from_existing(constant)],
                bit_rep_local.into(),
                None,
                context.source_info(),
            )));
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
                mir::Rvalue::RawPtr(rustc_ast::Mutability::Not, place.clone()),
            );

            (ptr_assignment, ptr_local)
        }

        /// Returns a local having a value equal to the function address.
        /// Effectively, it should be a unique value for each function.
        ///
        /// Three assignment statements are returned:
        /// 1. Function operand to function pointer.
        /// 2. Function pointer to raw pointer.
        /// 3. Raw pointer to id.
        pub(super) fn id_of_func<'tcx>(
            tcx: TyCtxt<'tcx>,
            local_manager: &mut (impl BodyLocalManager<'tcx> + HasLocalDecls<'tcx>),
            func: &Operand<'tcx>,
            id_ty: Ty<'tcx>,
        ) -> ([Statement<'tcx>; 3], Local) {
            log_debug!("Getting id (pointer) of function {:?}.", func);

            let fn_ty = func.ty(local_manager, tcx);
            debug_assert_matches!(fn_ty.kind(), TyKind::FnDef(..) | TyKind::FnPtr(..));
            let ptr_ty = Ty::new_fn_ptr(tcx, ty::fn_ptr_sig(tcx, fn_ty));
            let ptr_local = local_manager.add_local(ptr_ty);
            let ptr_assignment = assignment::create(
                Place::from(ptr_local),
                if fn_ty.is_fn_ptr() {
                    Rvalue::Use(func.clone())
                } else {
                    let TyKind::FnDef(def_id, ..) = fn_ty.kind() else {
                        unreachable!()
                    };
                    assert!(
                        !(tcx.intrinsic(def_id).is_some() || tcx.is_llvm_intrinsic(*def_id)),
                        "Cannot extract function pointer (as id) of intrinsic functions."
                    );
                    rvalue::cast_to_coerced(PointerCoercion::ReifyFnPointer, func.clone(), ptr_ty)
                },
            );

            /* This is an additional step just for the sake of semantics.
             * (FnPtr -> RawPointer -> FuncId)
             */
            let raw_ptr_local = local_manager.add_local(tcx.types.usize);
            let raw_ptr_assignment = assignment::create(
                Place::from(raw_ptr_local),
                Rvalue::Cast(
                    mir::CastKind::PointerExposeProvenance,
                    operand::move_for_local(ptr_local),
                    tcx.types.usize,
                ),
            );

            let id_local = local_manager.add_local(id_ty);
            let id_assignment = assignment::create(
                Place::from(id_local),
                Rvalue::Cast(
                    mir::CastKind::IntToInt,
                    operand::move_for_local(raw_ptr_local),
                    id_ty,
                ),
            );
            (
                [ptr_assignment, raw_ptr_assignment, id_assignment],
                id_local,
            )
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

        pub(super) fn are_args_tupled<'tcx: 'a, 'a>(
            tcx: TyCtxt<'tcx>,
            local_manager: &impl HasLocalDecls<'tcx>,
            callee: &Operand<'tcx>,
            args: impl Iterator<Item = &'a Operand<'tcx>>,
            param_env: mir_ty::ParamEnv<'tcx>,
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
                    .is_tuple(tcx, param_env),
                "Fn trait method call without tupled arguments observed. {:?}, {:?}",
                callee,
                args.iter()
                    .map(|a| a.ty(local_manager, tcx))
                    .collect::<Vec<_>>()
            );
            true
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
            ForInsertion<'tcx> + DestinationProvider<'tcx>
        {
        }
        impl<'tcx, C> ForAssignment<'tcx> for C where C: ForInsertion<'tcx> + DestinationProvider<'tcx> {}

        pub(crate) trait ForCasting<'tcx>:
            CastOperandProvider + ForAssignment<'tcx>
        {
        }
        impl<'tcx, C> ForCasting<'tcx> for C where C: CastOperandProvider + ForAssignment<'tcx> {}

        pub(crate) trait ForBranching<'tcx>:
            ForInsertion<'tcx> + JumpTargetModifier
        {
        }
        impl<'tcx, C> ForBranching<'tcx> for C where C: ForInsertion<'tcx> + JumpTargetModifier {}

        pub(crate) trait ForAssertion<'tcx>: ForOperandRef<'tcx> {}
        impl<'tcx, C> ForAssertion<'tcx> for C where C: ForOperandRef<'tcx> {}

        pub(crate) trait ForFunctionCalling<'tcx>:
            ForInsertion<'tcx> + JumpTargetModifier
        {
        }
        impl<'tcx, C> ForFunctionCalling<'tcx> for C where C: ForInsertion<'tcx> + JumpTargetModifier {}

        pub(crate) trait ForReturning<'tcx>: ForInsertion<'tcx> {}
        impl<'tcx, C> ForReturning<'tcx> for C where C: ForInsertion<'tcx> {}

        pub(crate) trait ForEntryFunction<'tcx>:
            ForInsertion<'tcx> + InEntryFunction
        {
        }
        impl<'tcx, C> ForEntryFunction<'tcx> for C where C: ForInsertion<'tcx> + InEntryFunction {}
    }
}

pub(super) use implementation::context_requirements as ctxtreqs;

use crate::pri_utils::sym::intrinsics::LeafIntrinsicSymbol;
