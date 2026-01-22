use std::collections::HashMap;

use rustc_middle::mir::{BasicBlock, BasicBlockData, Body, Statement, Terminator, TerminatorKind};

use super::update_jumps;

const NEXT_BLOCK: BasicBlock = super::NEXT_BLOCK;

/// Splits all basic blocks and updates jump targets in a MIR body
/// at statements that the given predicate holds.
pub(crate) fn split_blocks_with(body: &mut Body, predicate: impl Fn(&Statement) -> bool) {
    let blocks = body.basic_blocks_mut();
    let original_blocks = Vec::from_iter(blocks.drain(..));
    let mut jump_map = HashMap::new();
    for (i, block) in original_blocks.into_iter().enumerate() {
        jump_map.insert(BasicBlock::from(i), blocks.next_index());

        let new_blocks = block.split_with(&predicate);
        for block in new_blocks {
            blocks.push(block);
        }
    }

    update_jumps(
        blocks.iter_enumerated_mut(),
        |_, target, _| jump_map.get(&target).cloned(),
        true,
        None,
        false,
    );
}

trait BasicBlockDataSplitExt<'tcx> {
    fn split_with(self, predicate: impl Fn(&Statement) -> bool) -> Vec<BasicBlockData<'tcx>>;
}

/// Splits a basic block at statements that the given predicate holds.
/// The statements passing the predicate will be the last statements in the resulting blocks.
/// The last block will have the same terminator as the original block.
impl<'tcx> BasicBlockDataSplitExt<'tcx> for BasicBlockData<'tcx> {
    fn split_with(self, predicate: impl Fn(&Statement) -> bool) -> Vec<BasicBlockData<'tcx>> {
        let mut result = Vec::new();

        let mut empty_block = BasicBlockData::new(None, false);
        empty_block.is_cleanup = self.is_cleanup;

        let mut new_block = empty_block.clone();
        for statement in self.statements {
            new_block.statements.push(statement);
            let statement = new_block.statements.last().unwrap();
            if predicate(&statement) {
                new_block.terminator = Some(Terminator {
                    kind: TerminatorKind::Goto { target: NEXT_BLOCK },
                    source_info: (&statement.source_info).clone(),
                });
                result.push(new_block);
                new_block = empty_block.clone();
            }
        }

        new_block.terminator = self.terminator;
        result.push(new_block);

        result
    }
}

pub(crate) fn noop_blocks_with<'tcx>(
    body: &mut Body<'tcx>,
    predicate: impl Fn(&BasicBlockData<'tcx>) -> bool,
) -> usize {
    let mut count = 0;
    for block in body.basic_blocks_mut().iter_mut() {
        if predicate(block) {
            block.statements.iter_mut().for_each(|s| s.make_nop(true));

            let terminator = block.terminator.take().unwrap();

            if terminator.successors().count() > 1 {
                panic!(
                    "Erasing a block with a terminator that has more than one successor: {:?}",
                    terminator
                );
            }

            let target = match terminator.kind {
                TerminatorKind::Goto { target } => target,
                TerminatorKind::Call {
                    target: Some(target),
                    ..
                } => target,
                _ => panic!(
                    "Erasing a block with an unsupported/unexpected terminator: {:?}",
                    terminator
                ),
            };

            block.terminator = Some(Terminator {
                kind: TerminatorKind::Goto { target },
                ..terminator
            });
            count += 1;
        }
    }
    count
}
