use rustc_ast::{InlineAsmOptions, InlineAsmTemplatePiece, Mutability};
use rustc_index::IndexVec;
use rustc_middle::{
    mir::{
        coverage::CoverageKind, AggregateKind, AssertMessage, BasicBlock, BinOp, BorrowKind,
        CallSource, CastKind, FakeReadCause, InlineAsmOperand, Local, NonDivergingIntrinsic,
        NullOp, Operand, Place, RetagKind, Rvalue, StatementKind, SwitchTargets, TerminatorKind,
        UnOp, UnwindAction, UnwindTerminateReason, UserTypeProjection,
    },
    ty::{Const, Region, Ty, Variance},
};
use rustc_span::{source_map::Spanned, Span};
use rustc_target::abi::FieldIdx;
use rustc_target::abi::VariantIdx;

macro_rules! make_statement_kind_visitor {
    ($visitor_trait_name:ident, $($mutability:ident)?) => {
        #[allow(unused_variables)]
        pub trait $visitor_trait_name<'tcx, T: Default> {
            fn visit_statement_kind(&mut self, kind: & $($mutability)? StatementKind<'tcx>) -> T {
                self.super_statement_kind(kind)
            }

            fn visit_assign(&mut self, place: & $($mutability)? Place<'tcx>, rvalue: & $($mutability)? Rvalue<'tcx>) -> T {
                Default::default()
            }

            fn visit_fake_read(&mut self, cause: & $($mutability)? FakeReadCause, place: & $($mutability)? Place<'tcx>) -> T {
                Default::default()
            }

            fn visit_set_discriminant(
                &mut self,
                place: &Place<'tcx>,
                variant_index: & $($mutability)? VariantIdx,
            ) -> T {
                Default::default()
            }

            fn visit_deinit(&mut self, place: & $($mutability)? Place<'tcx>) -> T {
                Default::default()
            }

            fn visit_storage_live(&mut self, local: & $($mutability)? Local) -> T {
                Default::default()
            }

            fn visit_storage_dead(&mut self, local: & $($mutability)? Local) -> T {
                Default::default()
            }

            fn visit_retag(&mut self, kind: & $($mutability)? RetagKind, place: & $($mutability)? Place<'tcx>) -> T {
                Default::default()
            }

            fn visit_place_mention(&mut self, place: & $($mutability)? Place<'tcx>) -> T {
                Default::default()
            }

            fn visit_ascribe_user_type(
                &mut self,
                place: & $($mutability)? Place<'tcx>,
                user_type_proj: & $($mutability)? UserTypeProjection,
                variance: & $($mutability)? Variance,
            ) -> T {
                Default::default()
            }

            fn visit_coverage(&mut self, coverage: & $($mutability)? CoverageKind) -> T {
                Default::default()
            }

            fn visit_intrinsic(&mut self, intrinsic: & $($mutability)? NonDivergingIntrinsic<'tcx>) -> T {
                Default::default()
            }

            fn visit_const_eval_counter(&mut self) -> T {
                Default::default()
            }

            fn visit_nop(&mut self) -> T {
                Default::default()
            }

            fn super_statement_kind(&mut self, kind: & $($mutability)? StatementKind<'tcx>) -> T {
                match kind {
                    StatementKind::Assign(box (place, rvalue)) => self.visit_assign(place, rvalue),
                    StatementKind::FakeRead(box (cause, place)) => {
                        self.visit_fake_read(cause, place)
                    }
                    StatementKind::SetDiscriminant {
                        place,
                        variant_index,
                    } => self.visit_set_discriminant(place, variant_index),
                    StatementKind::Deinit(place) => self.visit_deinit(place),
                    StatementKind::StorageLive(local) => self.visit_storage_live(local),
                    StatementKind::StorageDead(local) => self.visit_storage_dead(local),
                    StatementKind::Retag(kind, place) => self.visit_retag(kind, place),
                    StatementKind::PlaceMention(place) => self.visit_place_mention(place),
                    StatementKind::AscribeUserType(box (place, user_type_proj), variance) => {
                        self.visit_ascribe_user_type(place, user_type_proj, variance)
                    }
                    StatementKind::Coverage(coverage) => self.visit_coverage(coverage),
                    StatementKind::Intrinsic(intrinsic) => self.visit_intrinsic(intrinsic),
                    StatementKind::ConstEvalCounter => self.visit_const_eval_counter(),
                    StatementKind::Nop => self.visit_nop(),
                }
            }
        }
    };
}

make_statement_kind_visitor!(StatementKindVisitor,);
make_statement_kind_visitor!(StatementKindMutVisitor, mut);

macro_rules! make_terminator_kind_visitor {
    ($visitor_trait_name:ident, $($mutability:ident)?) => {
        #[allow(unused_variables)]
        pub trait $visitor_trait_name<'tcx, T: Default> {
            fn visit_terminator_kind(&mut self, kind: & $($mutability)? TerminatorKind<'tcx>) -> T {
                self.super_terminator_kind(kind)
            }

            fn visit_goto(&mut self, target: & $($mutability)? BasicBlock) -> T {
                Default::default()
            }

            fn visit_switch_int(
                &mut self,
                discr: & $($mutability)? Operand<'tcx>,
                targets: & $($mutability)? SwitchTargets
            ) -> T {
                Default::default()
            }

            fn visit_unwind_resume(&mut self) -> T {
                Default::default()
            }

            fn visit_unwind_terminate(
                &mut self,
                reason: & $($mutability)? UnwindTerminateReason
            ) -> T {
                Default::default()
            }

            fn visit_return(&mut self) -> T {
                Default::default()
            }

            fn visit_unreachable(&mut self) -> T {
                Default::default()
            }

            fn visit_drop(
                &mut self,
                place: & $($mutability)? Place<'tcx>,
                target: & $($mutability)? BasicBlock,
                unwind: & $($mutability)? UnwindAction,
                replace: & $($mutability)? bool,
            ) -> T {
                Default::default()
            }

            #[allow(clippy::too_many_arguments)]
            fn visit_call(
                &mut self,
                func: & $($mutability)? Operand<'tcx>,
                args: & $($mutability)? [Spanned<Operand<'tcx>>],
                destination: & $($mutability)? Place<'tcx>,
                target: & $($mutability)? Option<BasicBlock>,
                unwind: & $($mutability)? UnwindAction,
                call_source: & $($mutability)? CallSource,
                fn_span: Span,
            ) -> T {
                Default::default()
            }

            fn visit_assert(
                &mut self,
                cond: & $($mutability)? Operand<'tcx>,
                expected: & $($mutability)? bool,
                msg: & $($mutability)? AssertMessage<'tcx>,
                target: & $($mutability)? BasicBlock,
                unwind: & $($mutability)? UnwindAction,
            ) -> T {
                Default::default()
            }

            fn visit_yield(
                &mut self,
                value: & $($mutability)? Operand<'tcx>,
                resume: & $($mutability)? BasicBlock,
                resume_arg: & $($mutability)? Place<'tcx>,
                drop: & $($mutability)? Option<BasicBlock>,
            ) -> T {
                Default::default()
            }

            fn visit_coroutine_drop(&mut self) -> T {
                Default::default()
            }

            fn visit_false_edge(
                &mut self,
                real_target: & $($mutability)? BasicBlock,
                imaginary_target: & $($mutability)? BasicBlock,
            ) -> T {
                Default::default()
            }

            fn visit_false_unwind(
                &mut self,
                real_target: & $($mutability)? BasicBlock,
                unwind: & $($mutability)? UnwindAction,
            ) -> T {
                Default::default()
            }

            fn visit_inline_asm(
                &mut self,
                template: &'tcx [InlineAsmTemplatePiece],
                operands: & $($mutability)? [InlineAsmOperand<'tcx>],
                options: & $($mutability)? InlineAsmOptions,
                line_spans: &'tcx [Span],
                destination: & $($mutability)? Vec<BasicBlock>,
                unwind: & $($mutability)? UnwindAction,
            ) -> T {
                Default::default()
            }

            fn super_terminator_kind(&mut self, kind: & $($mutability)? TerminatorKind<'tcx>) -> T {
                match kind {
                    TerminatorKind::Goto { ref $($mutability)? target } => self.visit_goto(target),
                    TerminatorKind::SwitchInt {
                        discr,
                        ref $($mutability)? targets,
                    } => self.visit_switch_int(discr, targets),
                    TerminatorKind::UnwindResume => self.visit_unwind_resume(),
                    TerminatorKind::UnwindTerminate(ref $($mutability)? reason) => self.visit_unwind_terminate(reason),
                    TerminatorKind::Return => self.visit_return(),
                    TerminatorKind::Unreachable => self.visit_unreachable(),
                    TerminatorKind::Drop {
                        ref $($mutability)? place,
                        ref $($mutability)? target,
                        ref $($mutability)? unwind,
                        ref $($mutability)? replace,
                    } => self.visit_drop(place, target, unwind, replace),
                    TerminatorKind::Call {
                        func,
                        args,
                        ref $($mutability)? destination,
                        ref $($mutability)? target,
                        ref $($mutability)? unwind,
                        ref $($mutability)? call_source,
                        fn_span,
                    } => self.visit_call(
                        func,
                        args,
                        destination,
                        target,
                        unwind,
                        call_source,
                        *fn_span,
                    ),
                    TerminatorKind::Assert {
                        ref $($mutability)? cond,
                        ref $($mutability)? expected,
                        ref $($mutability)? msg,
                        ref $($mutability)? target,
                        ref $($mutability)? unwind,
                    } => self.visit_assert(cond, expected, msg, target, unwind),
                    TerminatorKind::Yield {
                        ref $($mutability)? value,
                        ref $($mutability)? resume,
                        ref $($mutability)? resume_arg,
                        ref $($mutability)? drop,
                    } => self.visit_yield(value, resume, resume_arg, drop),
                    TerminatorKind::CoroutineDrop => self.visit_coroutine_drop(),
                    TerminatorKind::FalseEdge {
                        ref $($mutability)? real_target,
                        ref $($mutability)? imaginary_target,
                    } => self.visit_false_edge(real_target, imaginary_target),
                    TerminatorKind::FalseUnwind {
                        ref $($mutability)? real_target,
                        ref $($mutability)? unwind,
                    } => self.visit_false_unwind(real_target, unwind),
                    TerminatorKind::InlineAsm {
                        ref template,
                        ref $($mutability)? operands,
                        ref $($mutability)? options,
                        line_spans,
                        ref $($mutability)? targets,
                        ref $($mutability)? unwind,
                    } => self.visit_inline_asm(
                        template,
                        operands,
                        options,
                        line_spans,
                        targets,
                        unwind,
                    ),
                }
            }
        }
    }
}

make_terminator_kind_visitor!(TerminatorKindVisitor,);
make_terminator_kind_visitor!(TerminatorKindMutVisitor, mut);

macro_rules! make_rvalue_visitor {
    ($visitor_trait_name:ident, $($mutability:ident)?) => {
        #[allow(unused_variables)]
        pub trait $visitor_trait_name<'tcx, T: Default> {
            fn visit_rvalue(&mut self, rvalue: & $($mutability)? Rvalue<'tcx>) -> T {
                self.super_rvalue(rvalue)
            }

            fn visit_use(&mut self, operand: & $($mutability)? Operand<'tcx>) -> T {
                Default::default()
            }

            fn visit_repeat(&mut self, operand: & $($mutability)? Operand<'tcx>, count: & $($mutability)? Const<'tcx>) -> T {
                Default::default()
            }

            fn visit_ref(
                &mut self,
                region: & $($mutability)? Region,
                borrow_kind: & $($mutability)? BorrowKind,
                place: & $($mutability)? Place<'tcx>,
            ) -> T {
                Default::default()
            }

            fn visit_thread_local_ref(
                &mut self,
                def_id: & $($mutability)? rustc_span::def_id::DefId,
            ) -> T {
                Default::default()
            }

            fn visit_address_of(
                &mut self,
                mutability: & $($mutability)? Mutability,
                place: & $($mutability)? Place<'tcx>,
            ) -> T {
                Default::default()
            }

            fn visit_len(&mut self, place: & $($mutability)? Place<'tcx>) -> T {
                Default::default()
            }

            fn visit_cast(
                &mut self,
                kind: & $($mutability)? CastKind,
                operand: & $($mutability)? Operand<'tcx>,
                ty: & $($mutability)? Ty<'tcx>,
            ) -> T {
                Default::default()
            }

            fn visit_binary_op(
                &mut self,
                op: & $($mutability)? BinOp,
                operands: & $($mutability)? Box<(Operand<'tcx>, Operand<'tcx>)>,
            ) -> T {
                Default::default()
            }

            fn visit_checked_binary_op(
                &mut self,
                op: & $($mutability)? BinOp,
                operands: & $($mutability)? Box<(Operand<'tcx>, Operand<'tcx>)>,
            ) -> T {
                Default::default()
            }

            fn visit_nullary_op(&mut self, op: & $($mutability)? NullOp, ty: & $($mutability)? Ty<'tcx>) -> T {
                Default::default()
            }

            fn visit_unary_op(&mut self, op: & $($mutability)? UnOp, operand: & $($mutability)? Operand<'tcx>) -> T {
                Default::default()
            }

            fn visit_discriminant(&mut self, place: & $($mutability)? Place<'tcx>) -> T {
                Default::default()
            }

            fn visit_aggregate(
                &mut self,
                kind: & $($mutability)? Box<AggregateKind>,
                operands: & $($mutability)? IndexVec<FieldIdx, Operand<'tcx>>,
            ) -> T {
                Default::default()
            }

            fn visit_shallow_init_box(
                &mut self,
                operand: & $($mutability)? Operand<'tcx>,
                ty: & $($mutability)? Ty<'tcx>,
            ) -> T {
                Default::default()
            }

            fn visit_copy_for_deref(&mut self, place: & $($mutability)? Place<'tcx>) -> T {
                Default::default()
            }

            fn super_rvalue(&mut self, rvalue: & $($mutability)? Rvalue<'tcx>) -> T {
                match rvalue {
                    Rvalue::Use(operand) => self.visit_use(operand),
                    Rvalue::Repeat(operand, count) => self.visit_repeat(operand, count),
                    Rvalue::Ref(region, borrow_kind, place) => {
                        self.visit_ref(region, borrow_kind, place)
                    }
                    Rvalue::ThreadLocalRef(def_id) => self.visit_thread_local_ref(def_id),
                    Rvalue::AddressOf(mutability, place) => {
                        self.visit_address_of(mutability, place)
                    }
                    Rvalue::Len(place) => self.visit_len(place),
                    Rvalue::Cast(kind, operand, ty) => self.visit_cast(kind, operand, ty),
                    Rvalue::BinaryOp(op, operands) => self.visit_binary_op(op, operands),
                    Rvalue::CheckedBinaryOp(op, operands) => {
                        self.visit_checked_binary_op(op, operands)
                    }
                    Rvalue::NullaryOp(op, ty) => self.visit_nullary_op(op, ty),
                    Rvalue::UnaryOp(op, operand) => self.visit_unary_op(op, operand),
                    Rvalue::Discriminant(place) => self.visit_discriminant(place),
                    Rvalue::Aggregate(kind, operands) => self.visit_aggregate(kind, operands),
                    Rvalue::ShallowInitBox(operand, ty) => self.visit_shallow_init_box(operand, ty),
                    Rvalue::CopyForDeref(place) => self.visit_copy_for_deref(place),
                }
            }
        }
    };
}

make_rvalue_visitor!(RvalueVisitor,);
make_rvalue_visitor!(RvalueMutVisitor, mut);
