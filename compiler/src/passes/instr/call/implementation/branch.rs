use crate::{
    mir_transform::JumpModificationConstraint, passes::instr::call::context::ConfigProvider,
};

use super::{
    BodyProvider, BranchingHandler, BranchingReferencer, OperandReferencer, SwitchInfo,
    context::{BaseContext, BlockIndexProvider, BlockOriginalIndexProvider, SwitchInfoProvider},
    ctxt_reqs::ForBranching,
    prelude::{mir::*, *},
};

impl<'tcx, C> BranchingReferencer<'tcx> for RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx> + OperandReferencer<'tcx>,
    C: ForBranching<'tcx>,
{
    fn store_branching_info(&mut self, discr: &Operand<'tcx>) -> SwitchInfo<'tcx> {
        let discr_ref = if self.config().switch_filter.data {
            Some(self.reference_operand(discr))
        } else {
            None
        };

        SwitchInfo {
            node_index: self.context.block_index(),
            original_node_index: self.original_bb_index_as_arg(),
            discr_ty: discr.ty(self.context.local_decls(), self.tcx()),
            discr: discr_ref,
        }
    }
}

impl<'tcx, C> RuntimeCallAdder<C>
where
    Self: BodyProvider<'tcx>,
    C: BaseContext<'tcx> + BlockIndexProvider + BlockOriginalIndexProvider,
{
    pub(super) fn original_bb_index_as_arg(&self) -> Operand<'tcx> {
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
    fn take_case(&mut self, index: usize, value: u128) {
        let tcx = self.context.tcx();

        let index = SwitchCaseIndex::try_from(index).unwrap_or_else(|_| {
            panic!(
                concat!(
                    "The number of cases in a switch terminator is unexpectedly large. ",
                    "Change the size of `SwitchCaseIndex`. ",
                    "Observed at {}"
                ),
                tcx.sess
                    .source_map()
                    .span_to_diagnostic_string(self.source_info().span),
            )
        });

        let mut blocks = Vec::new();

        let switch_info = self.context.switch_info();

        if self.config().switch_filter.data {
            let discr_ty = switch_info.discr_ty;
            let (func_name, add_index_arg, value_arg, additional_arg) = if discr_ty.is_bool() {
                const FALSE_SWITCH_VALUE: u128 = 0;
                if value == FALSE_SWITCH_VALUE {
                    (sym::take_branch_false, false, None, None)
                } else {
                    unreachable!(
                        "SwitchInts for booleans are expected to provide only the value 0 (false)."
                    )
                }
            } else if discr_ty.is_integral() {
                let primitive_ty_local = {
                    let (block, local) = self.make_primitive_type_of_bb(discr_ty);
                    blocks.push(block);
                    local
                };
                let (func_name, value_arg) = if is_case_value_large(value, discr_ty.is_signed()) {
                    (
                        sym::take_branch_int_lg,
                        operand::const_from_uint(tcx, value),
                    )
                } else {
                    (
                        sym::take_branch_int,
                        operand::const_from_uint(tcx, value as u32),
                    )
                };
                (
                    func_name,
                    true,
                    Some(value_arg),
                    Some(operand::move_for_local(primitive_ty_local)),
                )
            } else if discr_ty.is_char() {
                let value_arg = operand::const_from_char(
                    tcx,
                    char::from_u32(value.try_into().unwrap()).unwrap(),
                );
                (sym::take_branch_char, true, Some(value_arg), None)
            } else {
                unreachable!(
                    "Branching node discriminant is supposed to be either bool, int, char, or enum discriminant."
                )
            };

            blocks.push(
                self.make_bb_for_call_with_target(
                    func_name,
                    [
                        Some(switch_info.original_node_index),
                        add_index_arg.then(|| operand::const_from_uint(tcx, index)),
                        Some(operand::move_for_local(switch_info.discr.unwrap().into())),
                        value_arg,
                        additional_arg,
                    ]
                    .into_iter()
                    .flatten()
                    .collect(),
                    Some(self.context.block_index()),
                ),
            );
        } else if self.config().switch_filter.control {
            blocks.push(self.make_bb_for_call_with_target(
                sym::take_branch,
                vec![
                    switch_info.original_node_index,
                    operand::const_from_uint(tcx, index),
                ],
                Some(self.context.block_index()),
            ));
        }

        if blocks.is_empty() {
            return;
        }

        let new_block_index = self.insert_blocks_with_stickiness(blocks, false)[0];
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
        let mut blocks = Vec::new();

        let tcx = self.context.tcx();
        let switch_info = self.context.switch_info();

        if self.config().switch_filter.data {
            let discr_ty = switch_info.discr_ty;
            let mut non_values = non_values.into_iter();
            let (additional_stmts, func_name, additional_args) = if discr_ty.is_bool() {
                debug_assert!(non_values.len() == 1);
                debug_assert!(non_values.next().unwrap() == 0);

                (vec![], sym::take_branch_ow_bool, vec![])
            } else {
                let (func_name, value_ty, non_values, additional_args) = if discr_ty.is_integral() {
                    let primitive_ty_local = {
                        let (block, local) = self.make_primitive_type_of_bb(discr_ty);
                        blocks.push(block);
                        local
                    };

                    let non_values = non_values.collect::<Vec<_>>();
                    let (func_name, slice_elem_ty, non_values_operands) = if non_values
                        .iter()
                        .any(|nv| is_case_value_large(*nv, discr_ty.is_signed()))
                    {
                        (
                            sym::take_branch_ow_int_lg,
                            tcx.types.u128,
                            non_values
                                .into_iter()
                                .map(|nv: u128| operand::const_from_uint(tcx, nv))
                                .collect(),
                        )
                    } else {
                        (
                            sym::take_branch_ow_int,
                            tcx.types.u32,
                            non_values
                                .into_iter()
                                .map(|nv: u128| operand::const_from_uint(tcx, nv as u32))
                                .collect(),
                        )
                    };
                    (
                        func_name,
                        slice_elem_ty,
                        non_values_operands,
                        vec![operand::move_for_local(primitive_ty_local)],
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
                    utils::prepare_operand_for_slice(tcx, &mut self.context, value_ty, non_values);
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
                    vec![
                        switch_info.original_node_index,
                        operand::move_for_local(switch_info.discr.unwrap().into()),
                    ],
                    additional_args,
                ]
                .concat(),
                Some(self.context.block_index()),
            );
            block.statements.extend(additional_stmts);
            blocks.push(block);
        } else if self.config().switch_filter.control {
            blocks.push(self.make_bb_for_call_with_target(
                sym::take_branch_ow,
                vec![switch_info.original_node_index],
                Some(self.context.block_index()),
            ));
        }

        if blocks.is_empty() {
            return;
        }

        let new_block_index = self.insert_blocks_with_stickiness(blocks, false)[0];
        self.context.modify_jump_target_where(
            switch_info.node_index,
            self.context.block_index(),
            new_block_index,
            JumpModificationConstraint::SwitchOtherwise,
        );
    }
}

mod utils {
    pub(super) use super::super::utils::{operand, prepare_operand_for_slice};

    pub(super) fn is_case_value_large(value: u128, is_signed: bool) -> bool {
        type NonLargeU = u32;
        type NonLargeI = i32;

        if is_signed {
            let value = value as i128;
            value < i128::from(NonLargeI::MIN) || value > i128::from(NonLargeI::MAX)
        } else {
            value > u128::from(NonLargeU::MAX)
        }
    }
}
use common::types::SwitchCaseIndex;
use utils::*;
