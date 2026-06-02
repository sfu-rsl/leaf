use common::pri::{AtomicBinaryOp, AtomicOrdering};

use super::{
    Assigner, AtomicIntrinsicHandler, IntrinsicHandler,
    context::{AssignmentInfoProvider, PointerInfoProvider},
    ctxt_reqs::{Basic, ForAssignment, ForAtomicIntrinsic, ForMemoryIntrinsic},
    prelude::{mir::*, *},
    pri_utils::sym::intrinsics::{
        LeafIntrinsicSymbol, atomic::LeafAtomicIntrinsicSymbol, memory::LeafMemoryIntrinsicSymbol,
    },
    utils::operand,
};

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

    fn set(&mut self, val: OperandRef, count_ref: OperandRef, count_value: &Operand<'tcx>) {
        self.add_bb_for_memory_op_intrinsic_call(
            sym::intrinsics::memory::intrinsic_memory_set,
            vec![
                operand::move_for_local(val.into()),
                operand::move_for_local(count_ref.into()),
                count_value.to_copy(),
                operand::const_from_bool(self.tcx(), self.context.is_volatile()),
            ],
            Default::default(),
            Default::default(),
        )
    }

    fn swap(&mut self, second_ref: OperandRef, second_value: &Operand<'tcx>) {
        let mut stmts = Vec::new();

        let conc_second_ptr_local = {
            let (stmt, id_local) = self.make_conc_ptr_assignment(second_value.clone());
            stmts.push(stmt);
            id_local
        };

        self.add_bb_for_memory_op_intrinsic_call(
            sym::intrinsics::memory::intrinsic_memory_swap,
            vec![
                operand::move_for_local(second_ref.into()),
                operand::move_for_local(conc_second_ptr_local),
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
                vec![operand::const_from_uint(tcx, operator.to_raw())],
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
    fn make_ptr_pack_args(&mut self) -> (BasicBlockData<'tcx>, Statement<'tcx>, [Operand<'tcx>; 3])
    where
        C: PointerInfoProvider<'tcx>,
    {
        self.make_ptr_triple_args(
            self.context.ptr_operand_ref(),
            self.context.ptr_value().clone(),
            self.context.ptr_ty(),
        )
    }

    /// Makes three arguments of for a pointer to be passed to PRI:
    /// the operand reference to the pointer,
    /// the concrete value of the pointer,
    /// and the type id of the pointer.
    ///
    /// # Returns
    /// The type id block, the concrete pointer assignment, and the three arguments as operand array.
    pub fn make_ptr_triple_args(
        &mut self,
        operand_ref: OperandRef,
        ptr_value: Operand<'tcx>,
        ptr_ty: Ty<'tcx>,
    ) -> (BasicBlockData<'tcx>, Statement<'tcx>, [Operand<'tcx>; 3]) {
        let (conc_ptr_assignment, conc_ptr_local) = self.make_conc_ptr_assignment(ptr_value);

        let (ptr_type_id_block, ptr_type_id_local) = self.make_type_id_of_bb(ptr_ty);

        (
            ptr_type_id_block,
            conc_ptr_assignment,
            [
                operand::move_for_local(operand_ref.into()),
                operand::move_for_local(conc_ptr_local),
                operand::move_for_local(ptr_type_id_local),
            ],
        )
    }

    fn make_conc_ptr_assignment(&mut self, ptr_value: Operand<'tcx>) -> (Statement<'tcx>, Local) {
        let tcx = self.tcx();
        let raw_addr_ty = Ty::new_imm_ptr(tcx, tcx.types.unit);
        let local = self.add_local(raw_addr_ty);
        let assignment = assignment::create(
            Place::from(local),
            rvalue::cast_ptr_to_ptr(ptr_value, raw_addr_ty),
        );

        (assignment, local)
    }
}

impl<'tcx, C> RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx>,
    C: Basic<'tcx>,
{
    fn make_bb_for_atomic_ordering(&mut self, ordering: AtomicOrdering) -> BlocksAndResult<'tcx> {
        let tcx = self.tcx();

        self.make_bb_for_helper_call_with_all(
            self.pri_helper_funcs().const_atomic_ord_of,
            vec![],
            vec![operand::const_from_uint(tcx, ordering.to_raw())],
            Default::default(),
        )
        .into()
    }
}

mod utils {
    pub(super) use super::super::utils::{assignment, rvalue};
}
use utils::*;
