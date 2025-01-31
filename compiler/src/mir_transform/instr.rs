use std::collections::HashMap;

use rustc_ast::Mutability;
use rustc_index::IndexVec;
use rustc_middle::{
    mir::{
        BasicBlock, BasicBlockData, Body, ClearCrossCrate, HasLocalDecls, Local, LocalDecl,
        LocalDecls, SourceInfo, UnwindAction,
    },
    ty::Ty,
};

use super::*;
use common::{log_debug, log_warn};

#[derive(Debug, Clone)]
struct NewBasicBlock<'tcx> {
    pseudo_index: BasicBlock,
    data: BasicBlockData<'tcx>,
    is_sticky: bool,
}

pub(crate) struct BodyInstrumentationUnit<'tcx> {
    all_locals: IndexVec<Local, LocalDecl<'tcx>>,
    first_new_local: Local,
    // new_blocks_before maps BasicBlocks from MIR already in the AST to a list of new basic blocks
    // we'll insert just before it.
    new_blocks_before: HashMap<BasicBlock, Vec<NewBasicBlock<'tcx>>>,
    new_blocks_after: HashMap<BasicBlock, Vec<NewBasicBlock<'tcx>>>,
    new_blocks_count: u32, // this count is used to
    jump_modifications:
        HashMap<BasicBlock, Vec<(BasicBlock, JumpModificationConstraint, BasicBlock)>>,
}

impl<'tcx> BodyInstrumentationUnit<'tcx> {
    pub fn new(local_decls: &LocalDecls<'tcx>) -> Self {
        Self {
            /* We clone locals, so we implement HasLocalDecl with both original
             * and new locals included  */
            all_locals: local_decls.iter().cloned().collect(),
            first_new_local: local_decls.next_index(),
            new_blocks_before: HashMap::new(),
            new_blocks_after: HashMap::new(),
            new_blocks_count: 0,
            jump_modifications: HashMap::new(),
        }
    }

    fn insert_blocks_internal<I>(
        &mut self,
        index: BasicBlock,
        blocks: I,
        sticky: bool,
        is_before: bool,
    ) -> Vec<BasicBlock>
    where
        I: IntoIterator<Item = BasicBlockData<'tcx>>,
    {
        let map = if is_before {
            &mut self.new_blocks_before
        } else {
            &mut self.new_blocks_after
        };
        let chunk = map.entry(index).or_insert_with(Vec::new);
        let block_count: u32 = {
            let starting_count = chunk.len();
            // Associating temporary indices to the new blocks, so they can be referenced if needed.
            chunk.extend(blocks.into_iter().enumerate().map(|(i, b)| NewBasicBlock {
                pseudo_index: BasicBlock::from(
                    BasicBlock::MAX_AS_U32 - 1 - self.new_blocks_count - i as u32,
                ),
                data: b,
                is_sticky: sticky,
            }));
            (chunk.len() - starting_count).try_into().unwrap()
        };
        self.new_blocks_count += block_count;
        Vec::from_iter(
            chunk[(chunk.len() - block_count as usize)..]
                .iter()
                .inspect(|nbb| {
                    assert_ne!(
                        nbb.pseudo_index, index,
                        "Cannot insert a block before/after itself!"
                    )
                })
                .map(|nbb| nbb.pseudo_index),
        )
    }
}

impl<'tcx> From<(Mutability, Ty<'tcx>, SourceInfo)> for NewLocalDecl<'tcx> {
    fn from(value: (Mutability, Ty<'tcx>, SourceInfo)) -> Self {
        LocalDecl {
            mutability: value.0,
            local_info: ClearCrossCrate::Clear,
            ty: value.1,
            user_ty: None,
            source_info: value.2,
        }
        .into()
    }
}

impl<'tcx> From<(Ty<'tcx>, SourceInfo)> for NewLocalDecl<'tcx> {
    fn from(value: (Ty<'tcx>, SourceInfo)) -> Self {
        (Mutability::Not, value.0, value.1).into()
    }
}

impl<'tcx> From<Ty<'tcx>> for NewLocalDecl<'tcx> {
    fn from(value: Ty<'tcx>) -> Self {
        (value, SourceInfo::outermost(rustc_span::DUMMY_SP)).into()
    }
}

impl JumpModificationConstraint {
    /// Checks if this constraint satisfies a target situation.
    /// Returns `None` if the target is not consistent with this constraint;
    /// otherwise, a number representing the satisfaction score is returned.
    /// For example, an exact match will get a `MAX` score while the most general
    /// constraint (`None`) will get a 0 score.
    /// In simpler words, if self is less constraining it is considered more
    /// general and satisfying for a target.
    fn sat_score(&self, target: &JumpTargetAttribute) -> Option<u32> {
        match self {
            JumpModificationConstraint::None => Some(0),
            _ => self.eq(target).then_some(u32::MAX),
        }
    }
}

impl<'tcx> HasLocalDecls<'tcx> for BodyInstrumentationUnit<'tcx> {
    fn local_decls(&self) -> &LocalDecls<'tcx> {
        &self.all_locals
    }
}

impl<'tcx> BodyLocalManager<'tcx> for BodyInstrumentationUnit<'tcx> {
    fn add_local<T>(&mut self, decl_info: T) -> Local
    where
        T: Into<NewLocalDecl<'tcx>>,
    {
        self.all_locals.push(decl_info.into().0)
    }
}

impl<'tcx> BodyBlockManager<'tcx> for BodyInstrumentationUnit<'tcx> {
    fn insert_blocks_before<I>(
        &mut self,
        index: BasicBlock,
        blocks: I,
        sticky: bool,
    ) -> Vec<BasicBlock>
    where
        I: IntoIterator<Item = BasicBlockData<'tcx>>,
    {
        self.insert_blocks_internal(index, blocks, sticky, true)
    }

    // blocks will be inserted after index, and index will jump to block. The last block inserted
    // jumps to index's jump target.
    fn insert_blocks_after<I>(&mut self, index: BasicBlock, blocks: I) -> Vec<BasicBlock>
    where
        I: IntoIterator<Item = BasicBlockData<'tcx>>,
    {
        self.insert_blocks_internal(index, blocks, true, false)
    }
}

impl JumpTargetModifier for BodyInstrumentationUnit<'_> {
    fn modify_jump_target_where(
        &mut self,
        terminator_location: BasicBlock,
        from: BasicBlock,
        to: BasicBlock,
        constraint: JumpModificationConstraint,
    ) {
        if from == to {
            log_warn!(
                "Ignoring modification of jump target to the same index. from == to == {:?}",
                from
            );
            return;
        }

        self.jump_modifications
            .entry(terminator_location)
            .or_insert_with(|| Vec::with_capacity(1))
            .push((from, constraint, to));
    }
}

type InsertionPair<'tcx> = (BasicBlock, Vec<NewBasicBlock<'tcx>>);
impl<'tcx> BodyInstrumentationUnit<'tcx> {
    // No blocks actually get added to the MIR of the current body until this function gets called.
    pub fn commit(
        mut self,
        body: &mut Body<'tcx>,
        check_bb_pre_insert: Option<impl Fn(&BasicBlockData<'tcx>)>,
    ) {
        self.add_new_locals(&mut body.local_decls);

        if let Some(check_bb_pre_insert) = check_bb_pre_insert {
            for bb in self
                .new_blocks_before
                .values()
                .chain(self.new_blocks_after.values())
                .flatten()
            {
                check_bb_pre_insert(&bb.data);
            }
        }

        // this function applies any jump modifications to terminators of blocks as specified
        Self::update_jumps_pre_insert(
            Iterator::chain(
                body.basic_blocks_mut().iter_enumerated_mut(),
                self.new_blocks_before
                    .values_mut()
                    .chain(self.new_blocks_after.values_mut())
                    .flatten()
                    .map(|p| (p.pseudo_index, &mut p.data)),
            ),
            &self.jump_modifications,
        );

        if !(self.new_blocks_before.is_empty() && self.new_blocks_after.is_empty()) {
            let index_mapping = Self::insert_new_blocks(
                body.basic_blocks_mut(),
                self.new_blocks_before,
                self.new_blocks_after,
            );
            Self::update_jumps_post_insert(body.basic_blocks_mut(), index_mapping);
        }
    }

    fn add_new_locals(&mut self, original_locals: &mut IndexVec<Local, LocalDecl<'tcx>>) {
        if self.first_new_local == self.all_locals.next_index() {
            // No new locals were added
            return;
        }

        original_locals.extend(self.all_locals.drain(self.first_new_local.as_usize()..))
    }

    fn insert_new_blocks(
        blocks: &mut IndexVec<BasicBlock, BasicBlockData<'tcx>>,
        new_blocks_before: impl IntoIterator<Item = (BasicBlock, Vec<NewBasicBlock<'tcx>>)>,
        new_blocks_after: impl IntoIterator<Item = (BasicBlock, Vec<NewBasicBlock<'tcx>>)>,
    ) -> HashMap<BasicBlock, BasicBlock> {
        fn sort<'tcx>(
            into_iter: impl IntoIterator<Item = InsertionPair<'tcx>>,
        ) -> Vec<InsertionPair<'tcx>> {
            let mut vec = Vec::from_iter(into_iter);
            vec.sort_by_key(|p: &InsertionPair<'_>| p.0);
            vec
        }

        let mut new_blocks_before = sort(new_blocks_before);
        let mut new_blocks_after = sort(new_blocks_after);

        Self::resolve_indirect_new_blocks(
            &mut new_blocks_before,
            &mut new_blocks_after,
            blocks.next_index(),
        );

        let mut new_blocks_before = new_blocks_before.into_iter().peekable();
        let mut new_blocks_after = new_blocks_after.into_iter().peekable();
        let mut index_mapping = HashMap::<BasicBlock, BasicBlock>::new();

        let current_blocks = Vec::from_iter(blocks.drain(..));
        for (i, original_block) in current_blocks.into_iter().enumerate() {
            let i = BasicBlock::from(i);
            let mut top_index = blocks.next_index();

            if new_blocks_before
                .peek()
                .is_some_and(|(index, _)| *index == i)
            {
                Self::insert_blocks_before(
                    &mut index_mapping,
                    blocks,
                    new_blocks_before.next().unwrap().1,
                    &mut top_index,
                );
            }

            // top_index is the new place that any jumps will target instead of i
            let original_block_index = blocks.next_index();
            Self::push_with_index_mapping(
                &mut index_mapping,
                blocks,
                original_block,
                i,
                Some(top_index),
            );

            if new_blocks_after
                .peek()
                .is_some_and(|(index, _)| *index == i)
            {
                Self::insert_blocks_after(
                    &mut index_mapping,
                    blocks,
                    new_blocks_after.next().unwrap().1,
                    original_block_index,
                );
            }
        }

        // We only consider the insertion of blocks before the last block in this body (usually a return)
        assert!(
            new_blocks_before.peek().is_none() && new_blocks_after.peek().is_none(),
            "Found unexpected blocks that would be inserted after the last basic block"
        );

        index_mapping
    }

    /// Finds and flattens indirect new blocks.
    /// An indirect new block is a new block that its target is another new block.
    ///
    /// For these blocks we just flatten them by putting them in the same chunk as their target.
    /// In other words, we make them direct new blocks.
    fn resolve_indirect_new_blocks(
        before_chunks: &mut Vec<InsertionPair<'tcx>>,
        after_chunks: &mut Vec<InsertionPair<'tcx>>,
        min_new_block_index: BasicBlock,
    ) {
        /* The number of new indirect blocks is not expected to be large,
         * so this inefficient search should be fine. */
        let target_of_last = |x: &[InsertionPair]| x.last().map(|(target, _)| *target);
        let pop_if_target = |x: &mut Vec<InsertionPair<'tcx>>, target: BasicBlock| {
            if target_of_last(x) == Some(target) {
                Some(x.pop().unwrap().1)
            } else {
                None
            }
        };

        while let Some(target) = std::cmp::max(
            target_of_last(before_chunks),
            target_of_last(after_chunks),
        )
        // This is an indirect target
        && target >= min_new_block_index
        {
            log_debug!("Moving indirect new blocks targeting {:?}", target);
            let before_chunk = pop_if_target(before_chunks, target);
            let after_chunk = pop_if_target(after_chunks, target);

            let all_chunks = [before_chunks.as_mut_slice(), after_chunks.as_mut_slice()]
                .into_iter()
                .flatten()
                .map(|(_, chunk)| chunk);

            Self::insert_indirect_blocks(target, all_chunks, before_chunk, after_chunk);
        }
    }

    /// Inserts blocks that have to be inserted before/after a newly created block.
    ///
    /// # Arguments
    /// * target - The newly created block index that the chunks should be inserted before/after.
    /// * all_chunks - All chunks out there that one of them holds a new block
    ///   with pseudo index of `target`.
    /// * before - The blocks that should be inserted before `target`.
    /// * after - The blocks that should be inserted after `target`.
    fn insert_indirect_blocks<'a>(
        target: BasicBlock,
        mut all_chunks: impl Iterator<Item = &'a mut Vec<NewBasicBlock<'tcx>>>,
        before: Option<Vec<NewBasicBlock<'tcx>>>,
        after: Option<Vec<NewBasicBlock<'tcx>>>,
    ) where
        'tcx: 'a,
    {
        let (target_container, target_index) = all_chunks
            .find_map(|chunk| {
                chunk
                    .iter()
                    .position(|new_block| new_block.pseudo_index == target)
                    .map(|index| (chunk, index))
            })
            .unwrap();

        // Copy the stickiness of the target block
        let is_sticky = target_container[target_index].is_sticky;

        let mut insert = |index, chunk: Option<Vec<NewBasicBlock<'tcx>>>| {
            let Some(mut chunk) = chunk else { return };
            chunk.iter_mut().for_each(|nbb| nbb.is_sticky = is_sticky);
            target_container.splice(index..index, chunk);
        };

        insert(target_index + 1, after);
        insert(target_index, before);
    }

    fn insert_blocks_before(
        index_mapping: &mut HashMap<BasicBlock, BasicBlock>,
        blocks: &mut IndexVec<BasicBlock, BasicBlockData<'tcx>>,
        mut chunk: Vec<NewBasicBlock<'tcx>>,
        top_index: &mut BasicBlock,
    ) {
        blocks.extend_reserve(chunk.len());
        for non_sticky in chunk.extract_if(.., |b| !b.is_sticky) {
            Self::push_with_index_mapping(
                index_mapping,
                blocks,
                non_sticky.data,
                non_sticky.pseudo_index,
                None,
            );
        }
        *top_index = blocks.next_index();
        for sticky in chunk.extract_if(.., |b| b.is_sticky) {
            Self::push_with_index_mapping(
                index_mapping,
                blocks,
                sticky.data,
                sticky.pseudo_index,
                None,
            );
        }
        debug_assert!(chunk.is_empty());
    }

    fn insert_blocks_after(
        index_mapping: &mut HashMap<BasicBlock, BasicBlock>,
        blocks: &mut IndexVec<BasicBlock, BasicBlockData<'tcx>>,
        mut chunk: Vec<NewBasicBlock<'tcx>>,
        original_block_index: BasicBlock,
    ) {
        let original_block_target: BasicBlock = {
            let terminator = blocks
                .get_mut(original_block_index)
                .unwrap()
                .terminator_mut();
            let unwind = terminator.unwind().cloned();
            let mut successors = terminator.successors_mut();

            let target: &mut BasicBlock = successors.next().unwrap();
            let original_block_target = *target;
            *target = NEXT_BLOCK;

            // More than one successor?
            if let Some(other_target) = successors.next() {
                if let Some(UnwindAction::Cleanup(block)) = unwind
                    && *other_target == block
                {
                    // TODO: #206
                } else {
                    drop(successors);
                    panic!(
                        concat!(
                            "Insertion after a block with unexpected with multiple successors. ",
                            "Basic Block: {:?}, Terminator: {:#?}",
                        ),
                        original_block_index, terminator,
                    );
                }
            }

            original_block_target
        };

        let chunk_len = chunk.len();
        blocks.extend_reserve(chunk_len);
        for (i, mut bb) in chunk.drain(..).enumerate() {
            if i == chunk_len - 1 {
                let mut successors = bb.data.terminator.as_mut().unwrap().successors_mut();
                *successors.next().unwrap() = original_block_target;

                assert!(
                    successors.next().is_none(),
                    "Expected block with single successor"
                );
            }

            Self::push_with_index_mapping(index_mapping, blocks, bb.data, bb.pseudo_index, None);
        }
    }

    fn push_with_index_mapping(
        index_mapping: &mut HashMap<BasicBlock, BasicBlock>,
        blocks: &mut IndexVec<BasicBlock, BasicBlockData<'tcx>>,
        block: BasicBlockData<'tcx>,
        before_index: BasicBlock,
        after_index: Option<BasicBlock>,
    ) {
        let new_index = blocks.push(block);
        let new_index = after_index.unwrap_or(new_index);
        if new_index != before_index {
            index_mapping.insert(before_index, new_index);
        }
    }

    fn update_jumps_pre_insert<'b>(
        blocks: impl Iterator<Item = (BasicBlock, &'b mut BasicBlockData<'tcx>)>,
        jump_modifications: &HashMap<
            BasicBlock,
            Vec<(BasicBlock, JumpModificationConstraint, BasicBlock)>,
        >,
    ) where
        'tcx: 'b,
    {
        let blocks = blocks.filter(|(i, _)| jump_modifications.contains_key(i));

        update_jumps(
            blocks,
            |i, target, attr| {
                jump_modifications
                    .get(&i)
                    .unwrap()
                    .iter()
                    .filter(|(from, _, _)| *from == target)
                    .filter_map(|(_, c, to)| c.sat_score(attr).map(|s| (s, to)))
                    .max_by_key(|(score, _)| *score)
                    .map(|(_, to)| to)
                    .cloned()
            },
            false,
            Some(&|i, c| jump_modifications.get(&i).unwrap().len() == c),
            true,
        );
    }

    fn update_jumps_post_insert(
        blocks: &mut IndexVec<BasicBlock, BasicBlockData<'tcx>>,
        index_mapping: HashMap<BasicBlock, BasicBlock>,
    ) {
        update_jumps(
            blocks.iter_enumerated_mut(),
            |_, target, _| index_mapping.get(&target).cloned(),
            true,
            None,
            false,
        );
    }
}
