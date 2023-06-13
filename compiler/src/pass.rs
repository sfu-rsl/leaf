use rustc_abi::{FieldIdx, VariantIdx};
use rustc_index::IndexVec;
use rustc_middle::mir::{
    self, visit::Visitor, BasicBlock, BasicBlockData, CastKind, HasLocalDecls, Location, MirPass,
    Operand, Place, Rvalue, UnwindAction,
};

use crate::{
    mir_transform::{
        call_addition::{
            context::{BodyProvider, TyContextProvider},
            context_requirements as ctxtreqs, *,
        },
        modification::{BodyModificationUnit, JumpTargetModifier},
    },
    visit::*,
};

pub struct LeafPass;

impl<'tcx> MirPass<'tcx> for LeafPass {
    // NOTE: this function is called for every Body (function) in the program
    fn run_pass(
        &self,
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
        body: &mut rustc_middle::mir::Body<'tcx>,
    ) {
        log::info!("Running leaf pass on body at {:#?}", body.span);

        let mut modification = BodyModificationUnit::new(body.local_decls().next_index());
        let mut call_adder = RuntimeCallAdder::new(tcx, &mut modification);
        let mut call_adder = call_adder.in_body(body);
        if tcx.entry_fn(()).expect("No entry function was found").0 == body.source.def_id() {
            Self::handle_entry_function(
                &mut call_adder
                    .at(body.basic_blocks.indices().next().unwrap())
                    .in_entry_fn(),
            );

            // report that the entry function was "called" as a special case
            let func = Operand::function_handle(
                tcx,
                body.source.def_id(),
                ::std::iter::empty(),
                body.span, // for error handling
            );
            let func_ref = call_adder
                .at(body.basic_blocks.indices().next().unwrap())
                .reference_operand(&func);
            call_adder
                .at(body.basic_blocks.indices().next().unwrap())
                .before_call_func(func_ref, ::std::iter::empty());
        }

        // TODO: determine if body will ever be a promoted block
        let _is_promoted_block = body.source.promoted.is_some();
        call_adder
            .at(body.basic_blocks.indices().next().unwrap())
            .enter_func();

        VisitorFactory::make_body_visitor(&mut call_adder).visit_body(body);
        modification.commit(body);
    }
}

impl LeafPass {
    fn handle_entry_function(call_adder: &mut impl EntryFunctionHandler) {
        call_adder.init_runtime_lib();
    }
}

struct VisitorFactory;

impl VisitorFactory {
    fn make_body_visitor<'tcx, 'c, BC>(
        call_adder: &'c mut RuntimeCallAdder<BC>,
    ) -> impl Visitor<'tcx> + 'c
    where
        BC: ctxtreqs::Basic<'tcx> + JumpTargetModifier + BodyProvider<'tcx>,
    {
        LeafBodyVisitor {
            call_adder: RuntimeCallAdder::borrow_from(call_adder),
        }
    }

    fn make_basic_block_visitor<'tcx, 'c, BC>(
        call_adder: &'c mut RuntimeCallAdder<BC>,
        block: BasicBlock,
    ) -> impl Visitor<'tcx> + 'c
    where
        BC: ctxtreqs::Basic<'tcx> + JumpTargetModifier + BodyProvider<'tcx>,
    {
        LeafBasicBlockVisitor {
            call_adder: call_adder.at(block),
        }
    }

    fn make_statement_kind_visitor<'tcx, 'b, BC>(
        call_adder: &'b mut RuntimeCallAdder<BC>,
    ) -> impl StatementKindVisitor<'tcx, ()> + 'b
    where
        BC: ctxtreqs::ForPlaceRef<'tcx> + ctxtreqs::ForOperandRef<'tcx>,
    {
        LeafStatementKindVisitor {
            call_adder: RuntimeCallAdder::borrow_from(call_adder),
        }
    }

    fn make_terminator_kind_visitor<'tcx, 'b, BC>(
        call_adder: &'b mut RuntimeCallAdder<BC>,
    ) -> impl TerminatorKindVisitor<'tcx, ()> + 'b
    where
        BC: ctxtreqs::ForPlaceRef<'tcx>
            + ctxtreqs::ForOperandRef<'tcx>
            + ctxtreqs::ForBranching<'tcx>,
    {
        LeafTerminatorKindVisitor {
            call_adder: RuntimeCallAdder::borrow_from(call_adder),
        }
    }

    fn make_assignment_visitor<'tcx, 'b, BC>(
        call_adder: &'b mut RuntimeCallAdder<BC>,
        destination: &Place<'tcx>,
    ) -> impl RvalueVisitor<'tcx, ()> + 'b
    where
        BC: ctxtreqs::ForPlaceRef<'tcx> + ctxtreqs::ForOperandRef<'tcx>,
    {
        let dest_ref = call_adder.reference_place(destination);
        LeafAssignmentVisitor {
            call_adder: call_adder.assign(dest_ref),
        }
    }
}

macro_rules! make_general_visitor {
    ($name:ident) => {
        struct $name<C> {
            call_adder: RuntimeCallAdder<C>,
        }
    };
}

make_general_visitor!(LeafBodyVisitor);

impl<'tcx, C> Visitor<'tcx> for LeafBodyVisitor<C>
where
    C: ctxtreqs::Basic<'tcx> + JumpTargetModifier + BodyProvider<'tcx>,
{
    fn visit_basic_block_data(&mut self, block: BasicBlock, data: &BasicBlockData<'tcx>) {
        VisitorFactory::make_basic_block_visitor(&mut self.call_adder, block)
            .visit_basic_block_data(block, data);
    }
}

make_general_visitor!(LeafBasicBlockVisitor);

impl<'tcx, C> Visitor<'tcx> for LeafBasicBlockVisitor<C>
where
    C: ctxtreqs::ForPlaceRef<'tcx> + ctxtreqs::ForOperandRef<'tcx> + JumpTargetModifier,
{
    fn visit_statement(
        &mut self,
        statement: &rustc_middle::mir::Statement<'tcx>,
        _location: Location,
    ) {
        VisitorFactory::make_statement_kind_visitor(&mut self.call_adder)
            .visit_statement_kind(&statement.kind);
    }

    fn visit_terminator(&mut self, terminator: &mir::Terminator<'tcx>, _location: Location) {
        VisitorFactory::make_terminator_kind_visitor(&mut self.call_adder)
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
            .assign(destination)
            .its_discriminant_to(variant_index)
    }

    fn visit_deinit(&mut self, _place: &Place<'tcx>) {
        Default::default()
    }

    fn visit_intrinsic(&mut self, _intrinsic: &rustc_middle::mir::NonDivergingIntrinsic<'tcx>) {
        Default::default()
    }
}

make_general_visitor!(LeafTerminatorKindVisitor);

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
            call_adder.at(target).take_by_value(value);
        }
        call_adder
            .at(targets.otherwise())
            .take_otherwise(targets.iter().map(|v| v.0));
    }

    fn visit_return(&mut self) {
        self.call_adder.return_from_func();
    }

    fn visit_unreachable(&mut self) {
        Default::default()
    }

    fn visit_drop(&mut self, _place: &Place<'tcx>, _target: &BasicBlock, _unwind: &UnwindAction) {
        Default::default()
    }

    fn visit_call(
        &mut self,
        func: &Operand<'tcx>,
        args: &[Operand<'tcx>],
        destination: &Place<'tcx>,
        target: &Option<BasicBlock>,
        _unwind: &UnwindAction,
        _from_hir_call: bool,
        _fn_span: rustc_span::Span,
    ) {
        let func_ref = self.call_adder.reference_operand(func);
        let arg_refs = args
            .iter()
            .map(|a| self.call_adder.reference_operand(a))
            .collect::<Vec<OperandRef>>();
        self.call_adder
            .before_call_func(func_ref, arg_refs.iter().copied());

        if target.is_some() {
            self.call_adder.after_call_func(destination);
        } else {
            // This branch is only triggered by hitting a divergent function:
            // https://doc.rust-lang.org/rust-by-example/fn/diverging.html
            // (this means the program will exit immediately)
            log::warn!("visit_call() had no target, so couldn't insert block");
        }
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
        log::debug!("looking at assert message: '{:?}'", msg);
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

    fn visit_generator_drop(&mut self) {
        Default::default()
    }

    fn visit_inline_asm(
        &mut self,
        _template: &&[rustc_ast::InlineAsmTemplatePiece],
        _operands: &[mir::InlineAsmOperand<'tcx>],
        _options: &rustc_ast::InlineAsmOptions,
        _line_spans: &'tcx [rustc_span::Span],
        _destination: &Option<BasicBlock>,
        _unwind: &UnwindAction,
    ) {
        Default::default()
    }
}

make_general_visitor!(LeafAssignmentVisitor);

impl<'tcx, C> RvalueVisitor<'tcx, ()> for LeafAssignmentVisitor<C>
where
    C: ctxtreqs::ForPlaceRef<'tcx> + ctxtreqs::ForOperandRef<'tcx> + ctxtreqs::ForAssignment<'tcx>,
{
    fn visit_rvalue(&mut self, rvalue: &Rvalue<'tcx>) {
        log::debug!("Visiting Rvalue: {:#?}", rvalue);
        self.super_rvalue(rvalue)
    }

    fn visit_use(&mut self, operand: &Operand<'tcx>) {
        let operand_ref = self.call_adder.reference_operand(operand);
        self.call_adder.by_use(operand_ref)
    }

    /// `Repeat(...)` Creates an array where each element is the value of the operand.
    fn visit_repeat(&mut self, operand: &Operand<'tcx>, count: &rustc_middle::ty::Const<'tcx>) {
        // Array specification: https://doc.rust-lang.org/std/primitive.array.html
        // - Spec requires count is a non-negative compile-time constant size, so it must be
        //   of type usize https://doc.rust-lang.org/std/primitive.usize.html
        let scalar_int = match count.kind() {
            rustc_middle::ty::ConstKind::Param(_) => {
                todo!("used in const generics, may come up again when supporting generic functions")
            }
            rustc_middle::ty::ConstKind::Infer(_) => {
                todo!("used in const generics, may come up again when supporting generic functions")
            }
            rustc_middle::ty::ConstKind::Bound(_, _) => unreachable!(
                "relates to trait queries & type checking, so likely done in HIR; also MIRAI ignores this"
            ),
            rustc_middle::ty::ConstKind::Placeholder(_) => unreachable!(
                "used in the borrow checker before `optimized_mir`; also MIRAI ignores this"
            ),
            rustc_middle::ty::ConstKind::Unevaluated(_) => {
                unreachable!("this is only used in the HIR")
            }
            rustc_middle::ty::ConstKind::Value(val_tree) => match val_tree {
                rustc_middle::ty::ValTree::Leaf(scalar_int) => scalar_int,
                rustc_middle::ty::ValTree::Branch(_) => {
                    unreachable!("these are only for aggregate constants")
                }
            },
            rustc_middle::ty::ConstKind::Error(_) => panic!("The const here could not be computed"),
            rustc_middle::ty::ConstKind::Expr(_) => {
                unreachable!("this is unreachable because constant propagation")
            }
        };
        let operand_ref = self.call_adder.reference_operand(operand);
        self.call_adder.by_repeat(operand_ref, scalar_int)
    }

    fn visit_ref(
        &mut self,
        _region: &rustc_middle::ty::Region,
        borrow_kind: &rustc_middle::mir::BorrowKind,
        place: &Place<'tcx>,
    ) {
        let place_ref = self.call_adder.reference_place(place);
        self.call_adder.by_ref(
            place_ref,
            matches!(borrow_kind, rustc_middle::mir::BorrowKind::Mut { .. }),
        )
    }

    fn visit_thread_local_ref(&mut self) {
        self.call_adder.by_thread_local_ref();
    }

    fn visit_address_of(&mut self, mutability: &rustc_ast::Mutability, place: &Place<'tcx>) {
        let place_ref = self.call_adder.reference_place(place);
        self.call_adder
            .by_address_of(place_ref, mutability.is_mut());
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
            Pointer(kind) => {
                use rustc_middle::ty::adjustment::PointerCast::*;
                match kind {
                    Unsize => call_adder.through_unsizing(),
                    ReifyFnPointer | UnsafeFnPointer | ClosureFnPointer(_) => {
                        todo!("Support FnPointer casts")
                    }
                    MutToConstPointer | ArrayToPointer => todo!("Support raw pointer casts"),
                }
            }
            PointerExposeAddress => todo!("Support PointerExposeAddress casts"),
            PointerFromExposedAddress => todo!("Support PointerFromExposedAddress casts"),
            PtrToPtr => todo!("Support PtrToPtr casts"),
            FnPtrToPtr => todo!("Support FnPtrToPtr casts"),
            DynStar => todo!("Support DynStar casts"),
            Transmute => todo!("Support transmute casts"),
        }
    }

    fn visit_binary_op(&mut self, op: &mir::BinOp, operands: &Box<(Operand<'tcx>, Operand<'tcx>)>) {
        self.visit_binary_op_general(op, operands, false)
    }

    fn visit_checked_binary_op(
        &mut self,
        op: &mir::BinOp,
        operands: &Box<(Operand<'tcx>, Operand<'tcx>)>,
    ) {
        self.visit_binary_op_general(op, operands, true)
    }

    fn visit_nullary_op(
        &mut self,
        _op: &rustc_middle::mir::NullOp,
        _ty: &rustc_middle::ty::Ty<'tcx>,
    ) {
        // Nothing to do as they get computed and converted to constants before this stage.
        Default::default()
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

        let mut add_call: Box<dyn FnMut(&[OperandRef])> = match kind.as_ref() {
            mir::AggregateKind::Array(_) => {
                Box::new(|items| self.call_adder.by_aggregate_array(items))
            }
            mir::AggregateKind::Tuple => Box::new(|fields| {
                self.call_adder.by_aggregate_tuple(fields);
            }),
            mir::AggregateKind::Adt(x, variant, _, _, None) => {
                use rustc_hir::def::DefKind;
                match self.call_adder.tcx().def_kind(x) {
                    DefKind::Enum => Box::new(|fields| {self.call_adder.by_aggregate_enum(fields, *variant)}),
                    DefKind::Struct => Box::new(|fields| {
                        self.call_adder.by_aggregate_struct(fields)
                    }),
                    _ => unreachable!("Only enums and structs are supposed to be ADT.")
                }
            }
            mir::AggregateKind::Adt(_, _, _, _, Some(active_field)) /* Union */ => Box::new(|fields| {
                assert_eq!(
                    fields.len(),
                    1,
                    "For a union, there should only be one field."
                );
                self.call_adder.by_aggregate_union(*active_field, fields[0])
            }),
            mir::AggregateKind::Closure(_, _) => todo!("Closures are not supported yet."),
            mir::AggregateKind::Generator(_, _, _) => todo!("Generators are not supported yet."),
        };

        add_call(operands.as_slice())
    }

    fn visit_shallow_init_box(
        &mut self,
        _operand: &Operand<'tcx>,
        _ty: &rustc_middle::ty::Ty<'tcx>,
    ) {
        todo!("Not sure yet.")
    }

    fn visit_copy_for_deref(&mut self, _place: &Place<'tcx>) {
        todo!("Not sure yet.")
    }
}

impl<'tcx, C> LeafAssignmentVisitor<C>
where
    C: ctxtreqs::ForOperandRef<'tcx> + ctxtreqs::ForAssignment<'tcx>,
{
    fn visit_binary_op_general(
        &mut self,
        op: &mir::BinOp,
        operands: &(Operand<'tcx>, Operand<'tcx>),
        checked: bool,
    ) {
        let first_ref = self.call_adder.reference_operand(&operands.0);
        let second_ref = self.call_adder.reference_operand(&operands.1);
        self.call_adder
            .by_binary_op(op, first_ref, second_ref, checked)
    }
}
