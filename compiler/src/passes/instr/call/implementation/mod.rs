use core::debug_assert_matches;
use std::collections::HashMap;

use rustc_middle::{
    mir::{self, BasicBlock, BasicBlockData, HasLocalDecls},
    ty::{self as mir_ty},
};

use delegate::delegate;

use crate::mir_transform::*;
use crate::passes::Storage;
use crate::utils::mir::TyCtxtExt;

use super::{
    context::*,
    pri_utils::{
        FunctionInfo, PriHelperFunctions, PriTypes,
        sym::{self, LeafSymbol},
    },
    *,
};

use utils::*;

mod assertion;
mod assign;
mod branch;
pub(crate) mod ctxt_reqs;
mod func;
mod intrinsics;
mod operand;
mod place;
mod storage;

use ctxt_reqs::{ForEntryFunction, ForInsertion};

pub(crate) struct RuntimeCallAdder<C> {
    context: C,
}

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

    fn make_bb_for_helper_call_with_ret(
        &mut self,
        func_info: FunctionInfo,
        args: Vec<Operand<'tcx>>,
    ) -> (BasicBlockData<'tcx>, Local) {
        self.make_bb_for_helper_call_with_all(func_info, iter::empty(), args, None)
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
        config: Config,
    ) -> Self {
        Self {
            context: DefaultContext::new(tcx, modification_unit, pri, storage, config),
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

    pub fn at<'b>(
        &'b mut self,
        location: InsertionLocation,
    ) -> RuntimeCallAdder<AtLocationContext<'b, C>> {
        self.with_context(|base| AtLocationContext { base, location })
    }

    pub fn before<'b>(&'b mut self) -> RuntimeCallAdder<AtLocationContext<'b, C>>
    where
        C: BlockIndexProvider,
    {
        let index = self.context.block_index();
        self.with_context(|base| AtLocationContext {
            base,
            location: InsertionLocation::Before(index),
        })
    }

    pub fn after<'b>(&'b mut self) -> RuntimeCallAdder<AtLocationContext<'b, C>>
    where
        C: BlockIndexProvider,
    {
        let index = self.context.block_index();
        self.with_context(|base| AtLocationContext {
            base,
            location: InsertionLocation::After(index),
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
    ) -> RuntimeCallAdder<AssignmentContext<'b, C>> {
        self.with_context(|base| AssignmentContext { base, id, dest_ref })
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

    pub fn in_entry_fn<'b>(&'b mut self) -> RuntimeCallAdder<EntryFunctionMarkerContext<'b, C>> {
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

    pub fn borrow_from<'b>(
        other: &'b mut RuntimeCallAdder<C>,
    ) -> RuntimeCallAdder<TransparentContext<'b, C>> {
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
            fn pri_types(&self) -> &PriTypes;
            fn pri_helper_funcs(&self) -> &PriHelperFunctions;
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
impl<'tcx, C> ConfigProvider for RuntimeCallAdder<C>
where
    C: ConfigProvider,
{
    delegate! {
        to self.context {
            fn config(&self) -> &Config;
        }
    }
}
impl<'tcx, C> AssignmentInfoProvider for RuntimeCallAdder<C>
where
    C: AssignmentInfoProvider,
{
    delegate! {
        to self.context {
            fn assignment_id(&self) -> AssignmentId;
            fn dest_ref(&self) -> PlaceRef;
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
    fn current_typing_env(&self) -> mir_ty::TypingEnv<'tcx> {
        self.tcx().typing_env_in_body(self.current_func_id())
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
        assert_eq!(
            func_info.num_inputs(self.tcx()),
            args.len(),
            "Argument number mismatch for function {:?}",
            func_info,
        );
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
    C: TyContextProvider<'tcx> + SourceInfoProvider,
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
            InsertionLocation::Before(_) => self.insert_blocks_with_stickiness(blocks, true),
            InsertionLocation::After(index) => self.context.insert_blocks_after(index, blocks),
        }
    }

    fn insert_blocks_with_stickiness(
        &mut self,
        blocks: impl IntoIterator<Item = BasicBlockData<'tcx>>,
        sticky: bool,
    ) -> Vec<BasicBlock> {
        debug_assert_matches!(
            self.context.insertion_loc(),
            InsertionLocation::Before(..),
            "Stickiness is only defined for insertions before a block."
        );
        self.context
            .insert_blocks_before(self.context.block_index(), blocks, sticky)
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
        let ty = self.context.tcx().erase_and_anonymize_regions(ty);

        self.make_bb_for_helper_call_with_all(
            self.context.pri_helper_funcs().type_id_of,
            vec![ty.into()],
            Vec::default(),
            None,
        )
    }

    fn make_primitive_type_of_bb(&mut self, ty: Ty<'tcx>) -> (BasicBlockData<'tcx>, Local) {
        let tcx = self.tcx();
        let pri_ty = convert_primitive_ty_to_pri(tcx, ty);
        self.make_bb_for_helper_call_with_all(
            self.context.pri_helper_funcs().const_primitive_type_of,
            vec![],
            vec![utils::operand::const_from_scalar_int(
                tcx,
                pri_ty.to_raw().into(),
                tcx.types.i8,
            )],
            Default::default(),
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
        let block = self.make_bb_for_call(
            sym::debug_info,
            vec![utils::operand::const_from_byte_str(
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
            self, BasicBlock, HasLocalDecls, Local, Operand, Place, Rvalue, SourceInfo, Statement,
        },
        ty::{Ty, TyCtxt, TyKind, TypingEnv, adjustment::PointerCoercion},
    };
    use rustc_span::DUMMY_SP;

    use crate::mir_transform::{BodyLocalManager, NEXT_BLOCK};

    pub(super) use self::assignment::rvalue;

    use super::*;

    pub(super) mod operand {
        use std::mem::size_of;

        use rustc_const_eval::interpret::Scalar;
        use rustc_middle::{mir::Const, ty::ScalarInt};
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
                    tcx.valtree_to_const_val(mir_ty::Value {
                        ty,
                        valtree: mir_ty::ValTree::from_raw_bytes(tcx, value),
                    }),
                    ty,
                ),
            }))
        }

        pub fn copy_for_local<'tcx>(value: Local) -> Operand<'tcx> {
            for_local(value, true)
        }

        pub fn move_for_local<'tcx>(value: Local) -> Operand<'tcx> {
            for_local(value, false)
        }

        fn for_local<'tcx>(value: Local, copy: bool) -> Operand<'tcx> {
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

            pub fn cast_ptr_to_ptr<'tcx>(operand: Operand<'tcx>, to_ty: Ty<'tcx>) -> Rvalue<'tcx> {
                Rvalue::Cast(CastKind::PtrToPtr, operand, to_ty)
            }

            pub(in super::super) fn cast_to_unsize<'tcx>(
                operand: Operand<'tcx>,
                to_ty: Ty<'tcx>,
            ) -> Rvalue<'tcx> {
                cast_to_coerced_as(PointerCoercion::Unsize, operand, to_ty)
            }

            pub(in super::super) fn array<'tcx>(
                ty: Ty<'tcx>,
                items: Vec<Operand<'tcx>>,
            ) -> Rvalue<'tcx> {
                Rvalue::Aggregate(
                    Box::new(AggregateKind::Array(ty)),
                    IndexVec::from_raw(items),
                )
            }
        }

        pub fn create<'tcx>(destination: Place<'tcx>, value: Rvalue<'tcx>) -> Statement<'tcx> {
            Statement::new(
                /* NOTE: The source info can be propagated here too.
                 * However, as these statements end with a function call which has source info,
                 * we avoid the expense passing it around. */
                SourceInfo::outermost(DUMMY_SP),
                StatementKind::Assign(Box::new((destination, value))),
            )
        }
    }

    pub(super) mod ty {
        use rustc_abi::Size;

        use super::*;

        pub trait TyExt<'tcx> {
            fn is_trivially_tuple(self) -> bool;
            fn is_tuple(self, tcx: TyCtxt<'tcx>, typing_env: TypingEnv<'tcx>) -> bool;
            fn size(self, tcx: TyCtxt<'tcx>, typing_env: TypingEnv<'tcx>) -> Size;
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

            fn size(self, tcx: TyCtxt<'tcx>, typing_env: TypingEnv<'tcx>) -> Size {
                tcx.layout_of(typing_env.as_query_input(self)).unwrap().size
            }
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

        pub(super) fn is_trait<'tcx>(
            tcx: TyCtxt<'tcx>,
            ty: Ty<'tcx>,
            typing_env: TypingEnv<'tcx>,
            trait_def_id: DefId,
        ) -> bool {
            use rustc_infer::infer::TyCtxtInferExt;
            use rustc_trait_selection::infer::InferCtxtExt;

            tcx.infer_ctxt()
                .build(typing_env.typing_mode())
                .type_implements_trait(trait_def_id, [ty], typing_env.param_env)
                .must_apply_modulo_regions()
        }
    }

    pub(super) mod terminator {
        use rustc_middle::mir::{Terminator, TerminatorKind, UnwindAction};
        use rustc_span::Spanned;

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
                attributes: Default::default(),
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

        let ref_ty = Ty::new_imm_ref(tcx, tcx.lifetimes.re_erased, array_ty);
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

    pub(super) fn convert_primitive_ty_to_pri<'tcx>(
        tcx: TyCtxt<'tcx>,
        ty: Ty<'tcx>,
    ) -> common::pri::PrimitiveType {
        use common::pri::PrimitiveType;
        match ty.kind() {
            TyKind::Bool => PrimitiveType::BOOL,
            TyKind::Char => PrimitiveType::CHAR,
            TyKind::Int(_) => match ty.primitive_size(tcx).bytes() {
                1 => PrimitiveType::I8,
                2 => PrimitiveType::I16,
                4 => PrimitiveType::I32,
                8 => PrimitiveType::I64,
                16 => PrimitiveType::I128,
                s => panic!("Unexpected integer size: {s}"),
            },
            TyKind::Uint(_) => match ty.primitive_size(tcx).bytes() {
                1 => PrimitiveType::U8,
                2 => PrimitiveType::U16,
                4 => PrimitiveType::U32,
                8 => PrimitiveType::U64,
                16 => PrimitiveType::U128,
                s => panic!("Unexpected integer size: {s}"),
            },
            TyKind::Float(float_ty) => {
                use mir_ty::FloatTy;
                match float_ty {
                    FloatTy::F16 => PrimitiveType::F16,
                    FloatTy::F32 => PrimitiveType::F32,
                    FloatTy::F64 => PrimitiveType::F64,
                    FloatTy::F128 => PrimitiveType::F128,
                }
            }
            _ if ty.is_primitive() => panic!("Unexpected primitive type: {}", ty),
            _ => panic!("Unexpected non-primitive type."),
        }
    }

    pub(super) fn ptr_to_place<'tcx>(
        tcx: TyCtxt<'tcx>,
        local_manager: &mut impl BodyLocalManager<'tcx>,
        place: Place<'tcx>,
        place_ty: Ty<'tcx>,
    ) -> (Statement<'tcx>, Local) {
        let ptr_local = local_manager.add_local(Ty::new_imm_ptr(tcx, place_ty));
        let ptr_assignment = assignment::create(
            Place::from(ptr_local),
            Rvalue::RawPtr(mir::RawPtrKind::Const, place),
        );

        (ptr_assignment, ptr_local)
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
                .skip_normalization()
                .output()
                .no_bound_vars()
                .expect(
                    "PRI functions are not expected to have bound vars (generics) for the return type.",
                )
        }
    }
}

mod prelude {
    pub(super) use super::{
        BlockInserter, BlocksAndResult, DebugInfoHandler, MemoryIntrinsicHandler, MirCallAdder,
        RuntimeCallAdder,
    };

    pub(super) use super::{
        BodyLocalManager, HasLocalDecls, PriItemsProvider, SourceInfoProvider, TyContextProvider,
    };

    pub(super) use super::{OperandRef, PlaceRef};

    pub(super) use super::pri_utils::{sym, sym::LeafSymbol};

    pub(super) mod mir {
        pub use rustc_middle::{
            mir::{BasicBlockData, Local, Operand, Place, ProjectionElem, Rvalue, Statement},
            ty::{Ty, TyCtxt, TyKind},
        };
        pub use rustc_span::def_id::DefId;
    }
}
