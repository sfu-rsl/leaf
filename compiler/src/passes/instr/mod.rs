mod call;
mod decision;

use const_format::concatcp;

use rustc_index::IndexVec;
use rustc_middle::{
    mir::{
        self, BasicBlock, BasicBlockData, Body, BorrowKind, CastKind, HasLocalDecls, Location,
        MirSource, Operand, Place, Rvalue, SourceInfo, Statement, TerminatorKind, UnwindAction,
        visit::Visitor,
    },
    ty::{IntrinsicDef, TyCtxt},
};
use rustc_span::{Span, def_id::DefId, source_map::Spanned};
use rustc_target::abi::{FieldIdx, VariantIdx};

use common::{log_debug, log_info, log_warn};
use std::{collections::HashSet, num::NonZeroUsize, sync::atomic};

use crate::{
    config::InstrumentationRules,
    mir_transform::{self, BodyInstrumentationUnit, JumpTargetModifier},
    passes::{StorageExt, instr::call::context::PriItems},
    utils::mir::TyCtxtExt,
    visit::*,
};

use super::{CompilationPass, OverrideFlags, Storage};

use call::{
    AssertionHandler, Assigner, AtomicIntrinsicHandler, BranchingHandler, BranchingReferencer,
    CastAssigner, EntryFunctionHandler, FunctionHandler,
    InsertionLocation::*,
    IntrinsicHandler, OperandRef, OperandReferencer, PlaceReferencer, RuntimeCallAdder,
    context::{
        AtLocationContext, BlockIndexProvider, PriItemsProvider, SourceInfoProvider,
        TyContextProvider,
    },
    ctxtreqs,
};

const TAG_INSTRUMENTATION: &str = "instrumentation";
use TAG_INSTRUMENTATION as TAG_INSTR;
const TAG_INSTR_COUNTER: &str = concatcp!(TAG_INSTRUMENTATION, "::counter");

const KEY_PRI_ITEMS: &str = "pri_items";
const KEY_ENABLED: &str = "instr_enabled";
const KEY_TOTAL_COUNT: &str = "total_body_count";

#[derive(Default)]
pub(crate) struct Instrumentor {
    enabled: bool,
    total_body_count: Option<NonZeroUsize>,
    rules: Option<InstrumentationRules>,
}

impl Instrumentor {
    pub(crate) fn new(
        enabled: bool,
        total_body_count: Option<NonZeroUsize>,
        filters: InstrumentationRules,
    ) -> Self {
        Self {
            enabled,
            total_body_count,
            rules: Some(filters),
        }
    }
}

impl CompilationPass for Instrumentor {
    fn override_flags() -> OverrideFlags {
        OverrideFlags::OPTIMIZED_MIR
            | OverrideFlags::EXTERN_OPTIMIZED_MIR
            | OverrideFlags::MIR_SHIMS
    }

    fn visit_ast_before(
        &mut self,
        _krate: &rustc_ast::Crate,
        storage: &mut dyn Storage,
    ) -> rustc_driver::Compilation {
        // As early as possible, we use transform_ast to set the enabled flag.
        storage.get_or_insert_with(KEY_ENABLED.to_owned(), || self.enabled);
        storage.get_or_insert_with(KEY_TOTAL_COUNT.to_owned(), || self.total_body_count);
        storage.get_or_insert_with(decision::KEY_RULES.to_owned(), || {
            self.rules.take().unwrap()
        });
        rustc_driver::Compilation::Continue
    }

    fn transform_mir_body<'tcx>(
        tcx: TyCtxt<'tcx>,
        body: &mut Body<'tcx>,
        storage: &mut dyn Storage,
    ) {
        if !*storage.get_mut::<bool>(&KEY_ENABLED.to_owned()).unwrap() {
            return;
        }

        transform(tcx, body, storage);
    }
}

fn transform<'tcx>(tcx: TyCtxt<'tcx>, body: &mut Body<'tcx>, storage: &mut dyn Storage) {
    on_start(tcx, storage);

    let def_id = body.source.def_id();

    if !decision::should_instrument(tcx, body, storage) {
        log_info!(
            target: decision::TAG_INSTR_DECISION,
            "Skipping instrumentation for {:#?}",
            body.source.to_log_str(),
        );
        return;
    }

    log_info!(
        target: TAG_INSTR,
        "Running instrumentation pass on body of {} at {:?}",
        body.source.to_log_str(),
        body.span,
    );

    let pri_items = storage
        .get_or_insert_with(KEY_PRI_ITEMS.to_owned(), || make_pri_items(tcx))
        .leak();

    /* NOTE: Ideally this should not happen.
     * However, it is observed that in some cases (presumably because of inlining),
     * there are existing calls to our instrumentation functions, and they are
     * treated as regular blocks.
     * Clearing existing instrumentation (and redoing them) is not supposed to change
     * the behavior, at least as long as the control flow is changed by the instrumentation. */
    clear_existing_instrumentation(body, &pri_items.all_items);
    mir_transform::split_blocks_with(body, requires_immediate_instr_after);

    let mut modification = BodyInstrumentationUnit::new(body.local_decls());
    let mut call_adder = RuntimeCallAdder::new(tcx, &mut modification, &pri_items, storage);
    let mut call_adder = call_adder.in_body(body);

    let is_entry = tcx.entry_fn(()).is_some_and(|(id, _)| id == def_id);

    if is_entry {
        handle_entry_function_pre(&mut call_adder, body);
    }

    // TODO: determine if body will ever be a promoted block
    let _is_promoted_block = body.source.promoted.is_some();
    call_adder
        .at(Before(body.basic_blocks.indices().next().unwrap()))
        .with_source_info(*body.source_info(Location::START))
        .enter_func();

    VisitorFactory::make_body_visitor(&mut call_adder).visit_body(body);

    if is_entry {
        handle_entry_function_post(&mut call_adder, body);
    }

    modification.commit(
        body,
        Some(|bb: &BasicBlockData<'tcx>| sanity_check_inserted_block(bb, &pri_items.all_items)),
    );

    pri_items.return_to(storage);
}

fn on_start(_tcx: TyCtxt, storage: &mut dyn Storage) {
    {
        static COUNTER: atomic::AtomicUsize = atomic::AtomicUsize::new(0);
        let counter = COUNTER.fetch_add(1, atomic::Ordering::SeqCst);
        let total = *storage
            .get_mut::<Option<NonZeroUsize>>(&KEY_TOTAL_COUNT.to_owned())
            .unwrap();
        let total_num: usize = total.unwrap_or(NonZeroUsize::MAX).into();
        let update_interval = total.map_or(100, |t| usize::from(t) / 100);
        if total_num - update_interval < counter || counter % update_interval == 0 {
            log_info!(
                target: TAG_INSTR_COUNTER,
                "Transforming {} / {}",
                counter,
                total.as_ref().map(NonZeroUsize::to_string).unwrap_or("?".to_owned()),
            );
        }
    }
}

fn make_pri_items(tcx: TyCtxt) -> PriItems {
    use crate::pri_utils::*;
    let all_items = all_pri_items(tcx);
    let main_funcs = filter_main_funcs(tcx, &all_items);
    let helper_items = filter_helper_items(tcx, &all_items);
    PriItems {
        funcs: main_funcs,
        types: collect_helper_types(&helper_items),
        helper_funcs: collect_helper_funcs(&helper_items),
        all_items: all_items.into_iter().collect(),
    }
}

fn clear_existing_instrumentation(body: &mut Body<'_>, pri_funcs: &HashSet<DefId>) {
    #[cfg(debug_assertions)]
    let original_body = body.clone();

    mir_transform::noop_blocks_with(body, |bb| is_instrumentation_block(bb, pri_funcs));

    // Ensure that the control flow is not modified.
    #[cfg(debug_assertions)]
    {
        assert_eq!(
            original_body.basic_blocks.len(),
            body.basic_blocks.len(),
            "Blocks are not expected to be added or removed."
        );

        for (original_bb, bb) in original_body
            .basic_blocks
            .iter()
            .zip(body.basic_blocks.iter())
        {
            assert!(
                original_bb
                    .terminator()
                    .successors()
                    .eq(bb.terminator().successors())
            );
        }
    }
}

fn handle_entry_function_pre<'tcx, C>(call_adder: &mut RuntimeCallAdder<C>, body: &Body<'tcx>)
where
    C: ctxtreqs::Basic<'tcx>,
{
    let mut call_adder = call_adder.in_entry_fn();
    let first_block = body.basic_blocks.indices().next().unwrap();
    let mut call_adder = call_adder.with_source_info(*body.source_info(Location::START));
    let mut call_adder = call_adder.at(Before(first_block));
    call_adder.init_runtime_lib();
}

fn handle_entry_function_post<'tcx, C>(call_adder: &mut RuntimeCallAdder<C>, body: &Body<'tcx>)
where
    C: ctxtreqs::Basic<'tcx>,
{
    let mut call_adder = call_adder.in_entry_fn();
    body.basic_blocks
        .iter_enumerated()
        .filter(|(_, bb)| bb.terminator().kind == mir::TerminatorKind::Return)
        .for_each(|(index, bb)| {
            call_adder
                .at(Before(index))
                .with_source_info(bb.terminator().source_info)
                .shutdown_runtime_lib();
        });
}

fn requires_immediate_instr_after(stmt: &Statement) -> bool {
    use rustc_middle::mir::StatementKind::*;
    matches!(&stmt.kind, Assign(..) | SetDiscriminant { .. })
}

struct VisitorFactory;

impl VisitorFactory {
    fn make_body_visitor<'tcx, 'c, C>(
        call_adder: &'c mut RuntimeCallAdder<C>,
    ) -> impl Visitor<'tcx> + 'c
    where
        C: ctxtreqs::Basic<'tcx> + JumpTargetModifier,
    {
        LeafBodyVisitor {
            call_adder: RuntimeCallAdder::borrow_from(call_adder),
        }
    }

    fn make_basic_block_visitor<'tcx, 'c, C>(
        call_adder: &'c mut RuntimeCallAdder<C>,
        block: BasicBlock,
    ) -> impl Visitor<'tcx> + 'c
    where
        C: ctxtreqs::Basic<'tcx> + JumpTargetModifier,
    {
        LeafBasicBlockVisitor {
            call_adder: call_adder.at(Before(block)),
        }
    }

    fn make_statement_kind_visitor<'tcx, 'b, C>(
        call_adder: &'b mut RuntimeCallAdder<C>,
    ) -> impl StatementKindVisitor<'tcx, ()> + 'b
    where
        C: ctxtreqs::ForPlaceRef<'tcx> + ctxtreqs::ForOperandRef<'tcx>,
    {
        LeafStatementKindVisitor {
            call_adder: RuntimeCallAdder::borrow_from(call_adder),
        }
    }

    fn make_terminator_kind_visitor<'tcx, 'b, C>(
        call_adder: &'b mut RuntimeCallAdder<C>,
    ) -> impl TerminatorKindVisitor<'tcx, ()> + 'b
    where
        C: ctxtreqs::ForOperandRef<'tcx>
            + ctxtreqs::ForPlaceRef<'tcx>
            + ctxtreqs::ForBranching<'tcx>
            + ctxtreqs::ForFunctionCalling<'tcx>
            + ctxtreqs::ForReturning<'tcx>,
    {
        LeafTerminatorKindVisitor {
            call_adder: RuntimeCallAdder::borrow_from(call_adder),
        }
    }

    fn make_assignment_visitor<'tcx, 'b, C>(
        call_adder: &'b mut RuntimeCallAdder<C>,
        destination: &Place<'tcx>,
    ) -> impl RvalueVisitor<'tcx, ()> + 'b
    where
        C: ctxtreqs::ForPlaceRef<'tcx> + ctxtreqs::ForOperandRef<'tcx>,
        'tcx: 'b,
    {
        let dest_ref = call_adder.reference_place(destination);
        let dest_ty = destination.ty(call_adder, call_adder.tcx()).ty;
        LeafAssignmentVisitor {
            call_adder: call_adder.assign(dest_ref, dest_ty),
        }
    }
}

macro_rules! make_general_visitor {
    ($vis:vis $name:ident $({ $($field_name: ident : $field_ty: ty),* })?) => {
        $vis struct $name<C> {
            call_adder: RuntimeCallAdder<C>,
            $($($field_name: $field_ty),*)?
        }
    };
}

make_general_visitor!(LeafBodyVisitor);

impl<'tcx, C> Visitor<'tcx> for LeafBodyVisitor<C>
where
    C: ctxtreqs::Basic<'tcx> + JumpTargetModifier,
{
    fn visit_basic_block_data(&mut self, block: BasicBlock, data: &BasicBlockData<'tcx>) {
        if data.is_cleanup {
            // NOTE: Cleanup blocks will be investigated in #206.
            log_debug!(target: TAG_INSTR, "Skipping instrumenting cleanup block: {:?}", block);
            return;
        }

        VisitorFactory::make_basic_block_visitor(&mut self.call_adder, block)
            .visit_basic_block_data(block, data);
    }
}

make_general_visitor!(LeafBasicBlockVisitor);

impl<'tcx, C> Visitor<'tcx> for LeafBasicBlockVisitor<C>
where
    C: ctxtreqs::Basic<'tcx> + BlockIndexProvider + JumpTargetModifier,
{
    fn visit_statement(
        &mut self,
        statement: &rustc_middle::mir::Statement<'tcx>,
        location: Location,
    ) {
        log_debug!(
            target: TAG_INSTR,
            "Visiting statement: {:?} at {:?}",
            statement.kind,
            location
        );
        VisitorFactory::make_statement_kind_visitor(
            &mut self
                .call_adder
                .with_source_info(statement.source_info)
                .before(),
        )
        .visit_statement_kind(&statement.kind);
    }

    fn visit_terminator(&mut self, terminator: &mir::Terminator<'tcx>, _location: Location) {
        VisitorFactory::make_terminator_kind_visitor(
            &mut self
                .call_adder
                .with_source_info(terminator.source_info)
                .before(),
        )
        .visit_terminator_kind(&terminator.kind);
    }
}

make_general_visitor!(LeafStatementKindVisitor);

impl<'tcx, C> StatementKindVisitor<'tcx, ()> for LeafStatementKindVisitor<C>
where
    C: ctxtreqs::ForPlaceRef<'tcx> + ctxtreqs::ForOperandRef<'tcx>,
{
    fn visit_assign(&mut self, place: &Place<'tcx>, rvalue: &Rvalue<'tcx>) {
        VisitorFactory::make_assignment_visitor(&mut self.call_adder, place).visit_rvalue(rvalue)
    }

    fn visit_set_discriminant(&mut self, place: &Place<'tcx>, variant_index: &VariantIdx) {
        let destination = self.call_adder.reference_place(place);
        self.call_adder
            .assign(
                destination,
                place.ty(&self.call_adder, self.call_adder.tcx()).ty,
            )
            .its_discriminant_to(variant_index)
    }

    fn visit_deinit(&mut self, _place: &Place<'tcx>) {
        Default::default()
    }

    fn visit_intrinsic(&mut self, _intrinsic: &rustc_middle::mir::NonDivergingIntrinsic<'tcx>) {
        // TODO
        Default::default()
    }
}

make_general_visitor!(pub(crate) LeafTerminatorKindVisitor);

impl<'tcx, C> TerminatorKindVisitor<'tcx, ()> for LeafTerminatorKindVisitor<C>
where
    C: ctxtreqs::ForOperandRef<'tcx>
        + ctxtreqs::ForPlaceRef<'tcx>
        + ctxtreqs::ForBranching<'tcx>
        + ctxtreqs::ForFunctionCalling<'tcx>
        + ctxtreqs::ForReturning<'tcx>,
{
    fn visit_switch_int(&mut self, discr: &Operand<'tcx>, targets: &mir::SwitchTargets) {
        let switch_info = self.call_adder.store_branching_info(discr);
        let mut call_adder = self.call_adder.branch(switch_info);
        for (value, target) in targets.iter() {
            call_adder.at(Before(target)).take_by_value(value);
        }
        call_adder
            .at(Before(targets.otherwise()))
            .take_otherwise(targets.iter().map(|v| v.0));
    }

    fn visit_return(&mut self) {
        self.call_adder.return_from_func();
    }

    fn visit_unreachable(&mut self) {
        Default::default()
    }

    fn visit_drop(
        &mut self,
        _place: &Place<'tcx>,
        _target: &BasicBlock,
        _unwind: &UnwindAction,
        _replace: &bool,
    ) {
        Default::default()
    }

    fn visit_call(
        &mut self,
        func: &Operand<'tcx>,
        args: &[Spanned<Operand<'tcx>>],
        destination: &Place<'tcx>,
        target: &Option<BasicBlock>,
        _unwind: &UnwindAction,
        _call_source: &mir::CallSource,
        fn_span: Span,
    ) {
        let tcx = self.call_adder.tcx();
        let opt_def_id = if let rustc_middle::ty::TyKind::FnDef(def_id, ..) =
            func.ty(&self.call_adder, tcx).kind()
        {
            assert!(
                !self.call_adder.all_pri_items().contains(def_id),
                "Instrumenting our own instrumentation."
            );
            Some(*def_id)
        } else {
            None
        };

        let params = CallParams {
            func,
            args,
            destination,
            target,
            fn_span,
        };

        match opt_def_id {
            Some(def_id) if tcx.is_llvm_intrinsic(def_id) => {
                self.instrument_llvm_intrinsic_call(params)
            }
            Some(def_id) if let Some(intrinsic) = tcx.intrinsic(def_id) => {
                self.instrument_intrinsic_call((def_id, intrinsic), params)
            }
            _ => self.instrument_regular_call(params),
        }
    }

    fn visit_tail_call(
        &mut self,
        _func: &Operand<'tcx>,
        _args: &[Spanned<Operand<'tcx>>],
        _fn_span: Span,
    ) -> () {
        // NOTE: https://github.com/rust-lang/rust/issues/112788
        unimplemented!(
            "This is still an experimental feature in the compiler and is not expected to appear in target projects."
        )
    }

    fn visit_assert(
        &mut self,
        cond: &Operand<'tcx>,
        expected: &bool,
        msg: &mir::AssertMessage<'tcx>,
        // we ignore target because this is concolic execution, not symbolic (program execution guides location)
        _target: &BasicBlock,
        _unwind: &UnwindAction,
    ) {
        let cond_ref = self.call_adder.reference_operand(cond);
        self.call_adder.check_assert(cond_ref, *expected, msg);
    }

    fn visit_yield(
        &mut self,
        _value: &Operand<'tcx>,
        _resume: &BasicBlock,
        _resume_arg: &Place<'tcx>,
        _drop: &Option<BasicBlock>,
    ) {
        Default::default()
    }

    fn visit_coroutine_drop(&mut self) {
        Default::default()
    }

    fn visit_inline_asm(
        &mut self,
        _asm_macro: &mir::InlineAsmMacro,
        _template: &[rustc_ast::InlineAsmTemplatePiece],
        _operands: &[mir::InlineAsmOperand<'tcx>],
        _options: &rustc_ast::InlineAsmOptions,
        _line_spans: &'tcx [Span],
        _destination: &Box<[BasicBlock]>,
        _unwind: &UnwindAction,
    ) {
        Default::default()
    }
}

struct CallParams<'a, 'tcx> {
    func: &'a Operand<'tcx>,
    args: &'a [Spanned<Operand<'tcx>>],
    destination: &'a Place<'tcx>,
    target: &'a Option<BasicBlock>,
    fn_span: Span,
}

impl<'tcx, C> LeafTerminatorKindVisitor<C>
where
    C: ctxtreqs::ForOperandRef<'tcx>
        + ctxtreqs::ForPlaceRef<'tcx>
        + ctxtreqs::ForFunctionCalling<'tcx>,
{
    fn instrument_intrinsic_call(
        &mut self,
        (def_id, def): (DefId, IntrinsicDef),
        params: CallParams<'_, 'tcx>,
    ) {
        use decision::IntrinsicDecision::*;
        match decision::decide_intrinsic_call(def) {
            OneToOneAssign(func_name) => {
                let mut call_adder = self.call_adder.before();
                let dest_ref = call_adder.reference_place(params.destination);
                let dest_ty = params.destination.ty(&call_adder, call_adder.tcx()).ty;
                let args = Self::ref_args(&mut call_adder, params.args);
                call_adder
                    .before()
                    .assign(dest_ref, dest_ty)
                    .intrinsic_one_to_one_by(def_id, func_name, args.into_iter());
            }
            Atomic(ordering, kind) => {
                self.instrument_atomic_intrinsic_call(&params, ordering, kind);
            }
            NoOp | ConstEvaluated | Contract => {
                // Currently, no instrumentation
                Default::default()
            }
            ToDo => {
                log_warn!(
                    target: TAG_INSTR,
                    "Intrinsic call to {:?} observed.",
                    def.name
                );
                self.instrument_unsupported_call(params);
            }
            NotPlanned => {
                log_warn!(
                    target: TAG_INSTR,
                    concat!(
                        "Intrinsic call to {:?} observed, which is not planned to be supported.",
                        "You might want to revise the target program.",
                    ),
                    def.name
                );
                self.instrument_unsupported_call(params);
            }
            Unsupported => {
                log_info!(
                    target: TAG_INSTR,
                    "Intrinsic call to {:?} observed, which is not yet supported.",
                    def.name
                );
                self.instrument_unsupported_call(params)
            }
            Unexpected => {
                panic!(
                    "Unexpected intrinsic call to {:?} observed at {:?}.",
                    def.name, params.fn_span,
                );
            }
        }
    }

    fn instrument_atomic_intrinsic_call(
        &mut self,
        params: &CallParams<'_, 'tcx>,
        ordering: common::pri::AtomicOrdering,
        kind: decision::AtomicIntrinsicKind,
    ) {
        let mut call_adder = self.call_adder.before();
        let ptr_arg = params.args.get(0);
        let ptr_ref = ptr_arg.map(|a| call_adder.reference_operand_spanned(a));
        let ptr_ty = ptr_arg.map(|a| a.node.ty(&call_adder, call_adder.tcx()));
        let mut call_adder = call_adder.perform_atomic_op(ordering, ptr_ref.zip(ptr_ty));
        use decision::AtomicIntrinsicKind::*;
        match kind {
            Load | Exchange | CompareExchange { .. } | BinOp(..) => {
                let dest_ref = call_adder.reference_place(params.destination);
                let dest_ty = params.destination.ty(&call_adder, call_adder.tcx()).ty;
                let mut call_adder = call_adder.assign(dest_ref, dest_ty);
                match kind {
                    Load => call_adder.load(),
                    BinOp(binop) => {
                        let src = call_adder.reference_operand_spanned(&params.args[1]);
                        call_adder.binary_op(binop, src);
                    }
                    Exchange => {
                        let src = call_adder.reference_operand_spanned(&params.args[1]);
                        call_adder.exchange(src)
                    }
                    CompareExchange {
                        fail_ordering,
                        weak,
                    } => {
                        let old = call_adder.reference_operand_spanned(&params.args[1]);
                        let src = call_adder.reference_operand_spanned(&params.args[2]);
                        call_adder.compare_exchange(fail_ordering, weak, old, src)
                    }
                    _ => unreachable!(),
                }
            }
            Store => {
                let val_ref = call_adder.reference_operand_spanned(&params.args[1]);
                call_adder.store(val_ref)
            }
            Fence { single_thread } => call_adder.fence(single_thread),
        };
    }

    fn instrument_llvm_intrinsic_call(&mut self, params: CallParams<'_, 'tcx>) {
        // Currently, we do not support for LLVM intrinsics.
        self.instrument_unsupported_call(params);
    }

    fn instrument_regular_call(&mut self, params: CallParams<'_, 'tcx>) {
        self.instrument_call_general(params, false);
    }

    fn instrument_unsupported_call(&mut self, params: CallParams<'_, 'tcx>) {
        self.instrument_call_general(params, true);
    }

    fn instrument_call_general(
        &mut self,
        CallParams {
            func,
            args,
            destination,
            target,
            fn_span: _,
        }: CallParams<'_, 'tcx>,
        no_definition: bool,
    ) {
        let mut call_adder = self.call_adder.before();

        call_adder.before_call_func(func, args, no_definition);

        if target.is_some() {
            call_adder.after().after_call_func(destination);
        } else {
            // This branch is only triggered by hitting a divergent function:
            // https://doc.rust-lang.org/rust-by-example/fn/diverging.html
            // (this means the program will exit immediately)
        }
    }

    fn ref_args(
        call_adder: &mut RuntimeCallAdder<AtLocationContext<C>>,
        args: &[Spanned<Operand<'tcx>>],
    ) -> Vec<OperandRef> {
        args.iter()
            .map(|arg| call_adder.reference_operand_spanned(arg))
            .collect()
    }
}

make_general_visitor!(LeafAssignmentVisitor);

impl<'tcx, C> RvalueVisitor<'tcx, ()> for LeafAssignmentVisitor<C>
where
    C: ctxtreqs::ForPlaceRef<'tcx> + ctxtreqs::ForOperandRef<'tcx> + ctxtreqs::ForAssignment<'tcx>,
{
    fn visit_rvalue(&mut self, rvalue: &Rvalue<'tcx>) {
        log_debug!(target: TAG_INSTR, "Visiting Rvalue: {:#?}", rvalue);
        self.super_rvalue(rvalue)
    }

    fn visit_use(&mut self, operand: &Operand<'tcx>) {
        let operand_ref = self.call_adder.reference_operand(operand);
        self.call_adder.by_use(operand_ref)
    }

    fn visit_repeat(&mut self, operand: &Operand<'tcx>, count: &rustc_middle::ty::Const<'tcx>) {
        let operand_ref = self.call_adder.reference_operand(operand);
        self.call_adder.by_repeat(operand_ref, count)
    }

    fn visit_ref(
        &mut self,
        _region: &rustc_middle::ty::Region,
        borrow_kind: &BorrowKind,
        place: &Place<'tcx>,
    ) {
        Self::instrument_ref(&mut self.call_adder, borrow_kind, place)
    }

    fn visit_thread_local_ref(&mut self, def_id: &DefId) {
        self.call_adder.by_thread_local_ref(def_id);
    }

    fn visit_raw_ptr(&mut self, kind: &mir::RawPtrKind, place: &Place<'tcx>) {
        let place_ref = self.call_adder.reference_place(place);
        self.call_adder
            .by_raw_ptr(place_ref, kind.to_mutbl_lossy().is_mut());
    }

    fn visit_len(&mut self, place: &Place<'tcx>) {
        let place_ref = self.call_adder.reference_place(place);
        self.call_adder.by_len(place_ref)
    }

    fn visit_cast(
        &mut self,
        kind: &CastKind,
        operand: &Operand<'tcx>,
        ty: &rustc_middle::ty::Ty<'tcx>,
    ) {
        let operand_ref = self.call_adder.reference_operand(operand);
        let call_adder = &mut self.call_adder.by_cast(operand_ref);
        use CastKind::*;
        match kind {
            IntToInt | FloatToInt => call_adder.to_int(*ty),
            IntToFloat | FloatToFloat => call_adder.to_float(*ty),
            PointerCoercion(coercion, _source) => {
                use rustc_middle::ty::adjustment::PointerCoercion::*;
                match coercion {
                    Unsize => call_adder.through_unsizing(),
                    ReifyFnPointer | UnsafeFnPointer | ClosureFnPointer(_) => {
                        call_adder.through_fn_ptr_coercion()
                    }
                    MutToConstPointer => call_adder.to_another_ptr(*ty, *kind),
                    ArrayToPointer => {
                        log_warn!(
                            target: TAG_INSTR,
                            concat!(
                            "ArrayToPointer casts are expected to be optimized away by at this point.",
                            "Sending it to runtime as a regular pointer cast."
                        ));
                        call_adder.to_another_ptr(*ty, *kind)
                    }
                    DynStar => call_adder.through_sized_dynamization(*ty),
                }
            }
            PointerExposeProvenance => call_adder.expose_prov(),
            PointerWithExposedProvenance => call_adder.with_exposed_prov(*ty),
            PtrToPtr | FnPtrToPtr => call_adder.to_another_ptr(*ty, *kind),
            Transmute => call_adder.transmuted(*ty),
        }
    }

    fn visit_binary_op(&mut self, op: &mir::BinOp, operands: &Box<(Operand<'tcx>, Operand<'tcx>)>) {
        self.visit_binary_op_general(op, operands)
    }

    fn visit_nullary_op(
        &mut self,
        _op: &rustc_middle::mir::NullOp,
        _ty: &rustc_middle::ty::Ty<'tcx>,
    ) {
        self.call_adder.by_nullary_op();
    }

    fn visit_unary_op(&mut self, op: &rustc_middle::mir::UnOp, operand: &Operand<'tcx>) {
        let operand_ref = self.call_adder.reference_operand(operand);
        self.call_adder.by_unary_op(op, operand_ref)
    }

    fn visit_discriminant(&mut self, place: &Place<'tcx>) {
        let place_ref = self.call_adder.reference_place(place);
        self.call_adder.by_discriminant(place_ref)
    }

    fn visit_aggregate(
        &mut self,
        kind: &Box<mir::AggregateKind>,
        operands: &IndexVec<FieldIdx, Operand<'tcx>>,
    ) {
        let operands: Vec<OperandRef> = operands
            .iter()
            .map(|o| self.call_adder.reference_operand(o))
            .collect();

        use mir::AggregateKind::*;
        #[allow(clippy::type_complexity)]
        let mut add_call: Box<dyn FnMut(&[OperandRef])> = match kind.as_ref() {
            Array(_) => {
                Box::new(|items| self.call_adder.by_aggregate_array(items))
            }
            Tuple => Box::new(|fields| {
                self.call_adder.by_aggregate_tuple(fields);
            }),
            Adt(def_id, variant, _, _, None) => {
                use rustc_hir::def::DefKind;
                match self.call_adder.tcx().def_kind(def_id) {
                    DefKind::Enum => Box::new(|fields| {self.call_adder.by_aggregate_enum(fields, *variant)}),
                    DefKind::Struct => Box::new(|fields| {
                        self.call_adder.by_aggregate_struct(fields)
                    }),
                    _ => unreachable!("Only enums and structs are supposed to be ADT.")
                }
            }
            Adt(_, _, _, _, Some(active_field)) /* Union */ => Box::new(|fields| {
                assert_eq!(
                    fields.len(),
                    1,
                    "For a union, there should only be one field."
                );
                self.call_adder.by_aggregate_union(*active_field, fields[0])
            }),
            Closure(..) => Box::new(|fields| {
                self.call_adder.by_aggregate_closure(fields)
            }),
            Coroutine(..) => Box::new(|fields| {
                self.call_adder.by_aggregate_coroutine(fields)
            }),
            CoroutineClosure(..) => Box::new(|fields| {
                self.call_adder.by_aggregate_coroutine_closure(fields)
            }),
            RawPtr(_, mutability) => Box::new(|fields| {
                match fields {
                    [data_ptr, metadata] => {
                        self.call_adder.by_aggregate_raw_ptr(*data_ptr, *metadata, mutability.is_mut())
                    },
                    _ => unreachable!()
            }}),
        };

        add_call(operands.as_slice())
    }

    fn visit_shallow_init_box(&mut self, operand: &Operand<'tcx>, ty: &rustc_middle::ty::Ty<'tcx>) {
        let operand_ref = self.call_adder.reference_operand(operand);
        self.call_adder.by_shallow_init_box(operand_ref, ty);
    }

    fn visit_copy_for_deref(&mut self, place: &Place<'tcx>) {
        let operand = Operand::Copy(*place);
        self.visit_use(&operand)
    }
}

impl<'tcx, C> LeafAssignmentVisitor<C>
where
    C: ctxtreqs::ForAssignment<'tcx>,
{
    fn instrument_ref(
        call_adder: &mut RuntimeCallAdder<C>,
        borrow_kind: &BorrowKind,
        place: &Place<'tcx>,
    ) where
        C: ctxtreqs::ForPlaceRef<'tcx>,
    {
        let place_ref = call_adder.reference_place(place);
        call_adder.by_ref(place_ref, matches!(borrow_kind, BorrowKind::Mut { .. }))
    }

    fn visit_binary_op_general(
        &mut self,
        op: &mir::BinOp,
        operands: &(Operand<'tcx>, Operand<'tcx>),
    ) where
        C: ctxtreqs::ForOperandRef<'tcx>,
    {
        let first_ref = self.call_adder.reference_operand(&operands.0);
        let second_ref = self.call_adder.reference_operand(&operands.1);
        self.call_adder.by_binary_op(op, first_ref, second_ref)
    }
}

impl<'tcx, C: ctxtreqs::ForOperandRef<'tcx>> RuntimeCallAdder<C> {
    fn reference_operand_spanned(&mut self, operand: &Spanned<Operand<'tcx>>) -> OperandRef {
        let source_scope = self.source_info().scope;
        let mut call_adder = self.before();
        call_adder
            .with_source_info(SourceInfo {
                span: operand.span,
                scope: source_scope,
            })
            .reference_operand(&operand.node)
    }
}

fn sanity_check_inserted_block<'tcx>(
    bb: &BasicBlockData<'tcx>,
    expected_called_funcs: &HashSet<DefId>,
) {
    if !is_instrumentation_block(bb, expected_called_funcs) {
        panic!(
            "Unexpected block inserted during instrumentation: {:#?}",
            bb
        );
    }
}

fn is_instrumentation_block<'tcx>(
    bb: &BasicBlockData<'tcx>,
    all_pri_funcs: &HashSet<DefId>,
) -> bool {
    let terminator = bb.terminator.as_ref().unwrap();
    match &terminator.kind {
        TerminatorKind::Call { .. } => is_call_to_any_of(&terminator.kind, all_pri_funcs),
        TerminatorKind::Goto { target } => {
            bb.statements.is_empty() && *target == mir_transform::NEXT_BLOCK
        }
        _ => false,
    }
}

#[inline]
fn is_call_to_any_of<'tcx>(terminator: &TerminatorKind, all_funcs: &HashSet<DefId>) -> bool {
    let TerminatorKind::Call { func, .. } = terminator else {
        return false;
    };
    func.const_fn_def()
        .is_some_and(|(def_id, _)| all_funcs.contains(&def_id))
}

trait MirSourceExt {
    fn to_log_str(&self) -> String;
}
impl MirSourceExt for MirSource<'_> {
    fn to_log_str(&self) -> String {
        format!(
            "{:?}{}",
            self.instance,
            self.promoted
                .map(|p| format!("::promoted[{:?}]", p))
                .unwrap_or_default()
        )
    }
}
