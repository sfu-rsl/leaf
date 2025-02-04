use common::log_debug;
use std::marker::PhantomData;

use rustc_middle::mir::{BasicBlock, Operand, Place, Terminator, UnwindAction};
use rustc_span::Span;

use crate::visit::TerminatorKindMutVisitor;

pub(crate) const TAG_BB_JUMP: &str = "bb_jump";

pub(super) type JumpTargetAttribute = super::JumpModificationConstraint;

pub(super) trait MapFunc:
    Fn(BasicBlock, &JumpTargetAttribute) -> Option<BasicBlock>
{
}
impl<T: Fn(BasicBlock, &JumpTargetAttribute) -> Option<BasicBlock>> MapFunc for T {}

pub(super) struct JumpUpdater<'tcx, M>
where
    M: MapFunc,
{
    index_mapping: M,
    count: usize,
    recursive: bool,
    phantom: PhantomData<&'tcx ()>,
}

impl<'tcx, M> JumpUpdater<'tcx, M>
where
    M: MapFunc,
{
    pub(super) fn new(index_mapping: M, recursive: bool) -> Self {
        Self {
            index_mapping,
            count: 0,
            recursive,
            phantom: PhantomData,
        }
    }
}

impl<'tcx, M> TerminatorKindMutVisitor<'tcx, ()> for JumpUpdater<'tcx, M>
where
    M: MapFunc,
{
    fn visit_goto(&mut self, target: &mut BasicBlock) {
        self.update(target);
    }

    fn visit_switch_int(
        &mut self,
        _discr: &mut Operand<'tcx>,
        targets: &mut rustc_middle::mir::SwitchTargets,
    ) {
        // Because of API limitations we have to take this weird approach.
        let values: Vec<u128> = targets.iter().map(|(v, _)| v).collect();
        for (index, target) in targets.all_targets_mut().iter_mut().enumerate() {
            if index < values.len() {
                self.update_with_attr(
                    &mut *target,
                    JumpTargetAttribute::SwitchValue(values[index]),
                );
            } else {
                self.update_with_attr(&mut *target, JumpTargetAttribute::SwitchOtherwise);
            }
        }
    }

    fn visit_drop(
        &mut self,
        _place: &mut Place<'tcx>,
        target: &mut BasicBlock,
        unwind: &mut UnwindAction,
        _replace: &mut bool,
    ) {
        self.update(target);
        self.update_maybe(unwind.basic_block());
    }

    fn visit_call(
        &mut self,
        _func: &mut Operand<'tcx>,
        _args: &mut [rustc_span::source_map::Spanned<Operand<'tcx>>],
        _destination: &mut Place<'tcx>,
        target: &mut Option<BasicBlock>,
        unwind: &mut UnwindAction,
        _call_source: &mut rustc_middle::mir::CallSource,
        _fn_span: Span,
    ) {
        self.update_maybe(target.as_mut());
        self.update_maybe(unwind.basic_block());
    }

    fn visit_assert(
        &mut self,
        _cond: &mut Operand<'tcx>,
        _expected: &mut bool,
        _msg: &mut rustc_middle::mir::AssertMessage<'tcx>,
        target: &mut BasicBlock,
        unwind: &mut UnwindAction,
    ) {
        self.update(target);
        self.update_maybe(unwind.basic_block());
    }

    fn visit_yield(
        &mut self,
        _value: &mut Operand<'tcx>,
        resume: &mut BasicBlock,
        _resume_arg: &mut Place<'tcx>,
        drop: &mut Option<BasicBlock>,
    ) {
        self.update(resume);
        self.update_maybe(drop.as_mut());
    }

    fn visit_false_edge(
        &mut self,
        real_target: &mut BasicBlock,
        imaginary_target: &mut BasicBlock,
    ) {
        self.update(real_target);
        self.update(imaginary_target);
    }

    fn visit_false_unwind(&mut self, real_target: &mut BasicBlock, unwind: &mut UnwindAction) {
        self.update(real_target);
        self.update_maybe(unwind.basic_block());
    }

    fn visit_inline_asm(
        &mut self,
        _asm_macro: &mut rustc_middle::mir::InlineAsmMacro,
        _template: &'tcx [rustc_ast::InlineAsmTemplatePiece],
        _operands: &mut [rustc_middle::mir::InlineAsmOperand<'tcx>],
        _options: &mut rustc_ast::InlineAsmOptions,
        _line_spans: &'tcx [Span],
        targets: &mut Box<[BasicBlock]>,
        unwind: &mut UnwindAction,
    ) {
        targets.iter_mut().for_each(|target| self.update(target));
        self.update_maybe(unwind.basic_block());
    }
}

impl<'tcx, M> JumpUpdater<'tcx, M>
where
    M: MapFunc,
{
    pub fn update_terminator(&mut self, terminator: &mut Terminator<'tcx>) -> usize {
        self.count = 0;
        Self::visit_terminator_kind(self, &mut terminator.kind);
        self.count
    }

    fn update(&mut self, target: &mut BasicBlock) {
        self.update_with_attr(target, JumpTargetAttribute::None)
    }

    fn update_maybe(&mut self, target: Option<&mut BasicBlock>) {
        self.update_maybe_with_attr(target, JumpTargetAttribute::None)
    }

    fn update_with_attr(&mut self, target: &mut BasicBlock, target_attr: JumpTargetAttribute) {
        let new_index = (self.index_mapping)(*target, &target_attr);
        let Some(new_index) = new_index else { return };
        log_debug!(target: TAG_BB_JUMP, "Updating jump target from {:?} to {:?}", target, new_index);
        *target = new_index;
        self.count += 1;
        if self.recursive {
            self.update(target);
        }
    }

    fn update_maybe_with_attr(
        &mut self,
        target: Option<&mut BasicBlock>,
        target_attr: JumpTargetAttribute,
    ) {
        if let Some(t) = target {
            self.update_with_attr(t, target_attr);
        }
    }
}

trait UnwindActionExt {
    fn basic_block(&mut self) -> Option<&mut BasicBlock>;
}

impl UnwindActionExt for UnwindAction {
    fn basic_block(&mut self) -> Option<&mut BasicBlock> {
        match self {
            UnwindAction::Cleanup(target) => Some(target),
            _ => None,
        }
    }
}
