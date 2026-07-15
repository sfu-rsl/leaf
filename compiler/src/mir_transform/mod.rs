mod instr;
/// This module hosts general utilities for modifying MIR bodies.
mod jump;
mod misc;

use rustc_middle::mir::{BasicBlock, BasicBlockData, HasLocalDecls, Local, LocalDecl};

pub(crate) use self::instr::BodyInstrumentationUnit;
use self::jump::JumpTargetAttribute;
pub(crate) use self::jump::TAG_BB_JUMP;
pub(crate) use self::misc::{noop_blocks_with, split_blocks_with};

pub(crate) const NEXT_BLOCK: BasicBlock = BasicBlock::MAX;

#[derive(derive_more::From)]
pub(crate) struct NewLocalDecl<'tcx>(LocalDecl<'tcx>);

pub(crate) trait BodyLocalManager<'tcx>: HasLocalDecls<'tcx> {
    fn add_local<T>(&mut self, decl_info: T) -> Local
    where
        T: Into<NewLocalDecl<'tcx>>;
}

pub(crate) trait BodyBlockManager<'tcx> {
    fn insert_blocks_before<I>(
        &mut self,
        index: BasicBlock,
        blocks: I,
        sticky: bool,
    ) -> Vec<BasicBlock>
    where
        I: IntoIterator<Item = BasicBlockData<'tcx>>;

    fn insert_blocks_after<I>(&mut self, index: BasicBlock, blocks: I) -> Vec<BasicBlock>
    where
        I: IntoIterator<Item = BasicBlockData<'tcx>>;
}

#[derive(PartialEq, Eq)]
pub(crate) enum JumpModificationConstraint {
    None,
    SwitchValue(u128),
    SwitchOtherwise,
}

pub(crate) trait JumpTargetModifier {
    fn modify_jump_target_where(
        &mut self,
        terminator_location: BasicBlock,
        from: BasicBlock,
        to: BasicBlock,
        constraint: JumpModificationConstraint,
    );
}

fn update_jumps<'tcx, 'b>(
    blocks: impl Iterator<Item = (BasicBlock, &'b mut BasicBlockData<'tcx>)>,
    index_mapping: impl Fn(BasicBlock, BasicBlock, &JumpTargetAttribute) -> Option<BasicBlock>,
    update_next: bool,
    sanity_check: Option<&dyn Fn(BasicBlock, usize) -> bool>,
    recursive: bool,
) where
    'tcx: 'b,
{
    let index_rc = std::cell::RefCell::new(BasicBlock::from(0_u32));
    let map = |target, attr: &JumpTargetAttribute| {
        if update_next && target == NEXT_BLOCK {
            Some(*index_rc.borrow() + 1)
        } else {
            index_mapping(*index_rc.borrow(), target, attr)
        }
    };
    let mut updater = jump::JumpUpdater::new(Box::new(map), recursive);

    let sanity_check = sanity_check.unwrap_or(&|_, _| true);
    for (index, block) in blocks.filter(|(_, b)| b.terminator.is_some()) {
        *index_rc.borrow_mut() = index;
        let update_count = updater.update_terminator(block.terminator_mut());
        if !sanity_check(index, update_count) {
            panic!("Update count of {update_count} was not acceptable at index {index:?}");
        }
    }
}
