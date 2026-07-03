use std::debug_assert_matches;

use rustc_abi::{FieldIdx, VariantIdx};
use rustc_middle::{
    mir::{BinOp, CastKind, UnOp},
    ty::Const,
};

use super::{
    Assigner, CastAssigner,
    context::{CastAssignmentContext, CastOperandProvider},
    ctxt_reqs::{ForAssignment, ForCasting},
    prelude::{mir::*, *},
};

impl<'tcx, C> Assigner<'tcx> for RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: ForAssignment<'tcx>,
{
    type Cast<'b>
        = RuntimeCallAdder<CastAssignmentContext<'b, C>>
    where
        CastAssignmentContext<'b, C>: ForCasting<'tcx> + 'b;

    fn by_use(&mut self, operand: OperandRef) {
        self.add_bb_for_assign_call(
            sym::assign_use,
            vec![operand::copy_for_local(operand.into())],
        )
    }

    fn by_repeat(&mut self, operand: OperandRef, count: &Const<'tcx>) {
        self.add_bb_for_assign_call(
            sym::assign_repeat,
            vec![
                operand::copy_for_local(operand.into()),
                #[allow(clippy::clone_on_copy)]
                operand::const_from_existing_ty_const(self.tcx().types.usize, count.clone()),
            ],
        )
    }

    fn by_ref(&mut self, place: PlaceRef, is_mutable: bool) {
        self.add_bb_for_assign_call(
            sym::assign_ref,
            vec![
                operand::copy_for_local(place.into()),
                operand::const_from_bool(self.context.tcx(), is_mutable),
            ],
        )
    }

    fn by_thread_local_ref(&mut self, _def_id: &DefId) {
        if cfg!(feature = "abs_concrete") {
            self.to_some_concrete()
        } else {
            self.add_bb_for_assign_call(sym::assign_thread_local_ref, vec![])
        }
    }

    fn by_raw_ptr(&mut self, place: PlaceRef, is_mutable: bool) {
        self.add_bb_for_assign_call(
            sym::assign_raw_ptr_of,
            vec![
                operand::copy_for_local(place.into()),
                operand::const_from_bool(self.context.tcx(), is_mutable),
            ],
        )
    }

    fn by_cast(&mut self, operand: OperandRef) -> Self::Cast<'_> {
        RuntimeCallAdder {
            context: CastAssignmentContext {
                base: &mut self.context,
                operand_ref: operand,
            },
        }
    }

    fn by_binary_op(&mut self, operator: &BinOp, first: OperandRef, second: OperandRef) {
        let tcx = self.tcx();
        let operator = convert_mir_binop_to_pri(operator);
        let operator_local = {
            let (block, local) = self.make_bb_for_helper_call_with_all(
                self.context.pri_helper_funcs().const_binary_op_of,
                vec![],
                vec![operand::const_from_uint(tcx, operator.to_raw())],
                Default::default(),
            );
            self.insert_blocks([block]);
            local
        };

        self.add_bb_for_assign_call(
            sym::assign_binary_op,
            vec![
                operand::move_for_local(operator_local),
                operand::copy_for_local(first.into()),
                operand::copy_for_local(second.into()),
            ],
        )
    }

    fn by_unary_op(&mut self, operator: &UnOp, operand: OperandRef) {
        let tcx = self.tcx();
        let operator = convert_mir_unop_to_pri(operator);
        let operator_local = {
            let (block, local) = self.make_bb_for_helper_call_with_all(
                self.context.pri_helper_funcs().const_unary_op_of,
                vec![],
                vec![operand::const_from_uint(tcx, operator.to_raw())],
                Default::default(),
            );
            self.insert_blocks([block]);
            local
        };

        self.add_bb_for_assign_call(
            sym::assign_unary_op,
            vec![
                operand::move_for_local(operator_local),
                operand::copy_for_local(operand.into()),
            ],
        )
    }

    fn by_discriminant(&mut self, place: PlaceRef) {
        self.add_bb_for_assign_call(
            sym::assign_discriminant,
            vec![operand::copy_for_local(place.into())],
        )
    }

    fn by_aggregate_array(&mut self, items: &[OperandRef]) {
        self.add_bb_for_aggregate_assign_call(sym::assign_aggregate_array, items, vec![])
    }

    fn by_aggregate_tuple(&mut self, fields: &[OperandRef]) {
        self.add_bb_for_adt_assign_call(sym::assign_aggregate_tuple, fields, vec![])
    }

    fn by_aggregate_struct(&mut self, fields: &[OperandRef]) {
        self.add_bb_for_adt_assign_call(sym::assign_aggregate_struct, fields, vec![])
    }

    fn by_aggregate_enum(&mut self, fields: &[OperandRef], variant: VariantIdx) {
        self.add_bb_for_adt_assign_call(
            sym::assign_aggregate_enum,
            fields,
            vec![operand::const_from_uint(
                self.context.tcx(),
                variant.as_u32(),
            )],
        )
    }

    fn by_aggregate_union(&mut self, active_field: FieldIdx, value: OperandRef) {
        self.add_bb_for_assign_call_with_statements(
            sym::assign_aggregate_union,
            vec![
                operand::const_from_uint(self.context.tcx(), active_field.as_u32()),
                operand::copy_for_local(value.into()),
            ],
            vec![],
        )
    }

    fn by_aggregate_closure(&mut self, upvars: &[OperandRef]) {
        self.add_bb_for_aggregate_assign_call(sym::assign_aggregate_closure, upvars, vec![])
    }

    fn by_aggregate_coroutine(&mut self, upvars: &[OperandRef]) {
        self.add_bb_for_aggregate_assign_call(sym::assign_aggregate_coroutine, upvars, vec![])
    }

    fn by_aggregate_coroutine_closure(&mut self, upvars: &[OperandRef]) {
        self.add_bb_for_aggregate_assign_call(
            sym::assign_aggregate_coroutine_closure,
            upvars,
            vec![],
        )
    }

    fn by_aggregate_raw_ptr(
        &mut self,
        data_ptr: OperandRef,
        metadata: OperandRef,
        is_mutable: bool,
    ) {
        self.add_bb_for_assign_call_with_statements(
            sym::assign_aggregate_raw_ptr,
            vec![
                operand::move_for_local(data_ptr.into()),
                operand::move_for_local(metadata.into()),
                operand::const_from_bool(self.tcx(), is_mutable),
            ],
            vec![],
        )
    }

    fn by_wrap_unsafe_binder(&mut self, operand: OperandRef, ty: &Ty<'tcx>) {
        let id_local = {
            let (block, id_local) = self.make_type_id_of_bb(*ty);
            self.insert_blocks([block]);
            id_local
        };
        self.add_bb_for_assign_call(
            sym::assign_wrap_unsafe_binder,
            vec![
                operand::copy_for_local(operand.into()),
                operand::move_for_local(id_local),
            ],
        );
    }

    fn its_discriminant_to(&mut self, variant_index: &VariantIdx) {
        self.add_bb_for_assign_call(
            sym::set_discriminant,
            vec![operand::const_from_uint(
                self.context.tcx(),
                variant_index.as_u32(),
            )],
        )
    }

    fn by_some(&mut self) {
        self.add_bb_for_assign_call(sym::assign_some, vec![])
    }
}

impl<'tcx, C> RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: ForAssignment<'tcx>,
{
    fn add_bb_for_aggregate_assign_call(
        &mut self,
        func_name: LeafSymbol,
        elements: &[OperandRef],
        additional_args: Vec<Operand<'tcx>>,
    ) {
        let (elements_local, additional_stmts) = self.make_slice_for_adt_elements(elements);

        self.add_bb_for_assign_call_with_statements(
            func_name,
            [
                vec![operand::move_for_local(elements_local)],
                additional_args,
            ]
            .concat(),
            additional_stmts.to_vec(),
        )
    }

    fn add_bb_for_adt_assign_call(
        &mut self,
        func_name: LeafSymbol,
        fields: &[OperandRef],
        additional_args: Vec<Operand<'tcx>>,
    ) {
        let mut args = Vec::new();
        let mut additional_stmts = Vec::new();

        let (fields_local, fields_stmts) = self.make_slice_for_adt_elements(fields);
        args.push(operand::move_for_local(fields_local));
        additional_stmts.extend(fields_stmts);

        args.extend(additional_args);

        self.add_bb_for_assign_call_with_statements(func_name, args, additional_stmts)
    }

    fn make_slice_for_adt_elements(
        &mut self,
        elements: &[OperandRef],
    ) -> (Local, [Statement<'tcx>; 3]) {
        let operand_ref_ty = self.context.pri_types().operand_ref(self.tcx());
        let (items_local, additional_stmts) = prepare_operand_for_slice(
            self.context.tcx(),
            &mut self.context,
            operand_ref_ty,
            elements
                .iter()
                .map(|i| operand::move_for_local((*i).into()))
                .collect(),
        );
        (items_local, additional_stmts)
    }

    fn add_bb_for_assign_call(&mut self, func_name: LeafSymbol, args: Vec<Operand<'tcx>>) {
        self.add_bb_for_assign_call_with_statements(func_name, args, vec![])
    }

    fn add_bb_for_assign_call_with_statements(
        &mut self,
        func_name: LeafSymbol,
        args: Vec<Operand<'tcx>>,
        statements: Vec<Statement<'tcx>>,
    ) {
        let mut block = self.make_bb_for_assign_call(func_name, args);
        block.statements.extend(statements);
        self.insert_blocks([block]);
    }

    pub(super) fn make_bb_for_assign_call(
        &mut self,
        func_name: LeafSymbol,
        args: Vec<Operand<'tcx>>,
    ) -> BasicBlockData<'tcx> {
        self.make_bb_for_call(
            func_name,
            [
                vec![
                    operand::const_from_uint(self.tcx(), self.context.assignment_id()),
                    operand::copy_for_local(self.context.dest_ref().into()),
                ],
                args,
            ]
            .concat(),
        )
    }
}

impl<'tcx, C> CastAssigner<'tcx> for RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: ForCasting<'tcx>,
{
    fn to_int(&mut self, ty: Ty<'tcx>) {
        if ty.is_char() {
            self.add_bb_for_cast_assign_call(sym::assign_cast_char)
        } else {
            assert!(ty.is_integral());

            let tcx = self.context.tcx();
            let is_signed = ty.is_signed();
            let bits = ty.primitive_size(tcx).bits();

            self.add_bb_for_cast_assign_call_with_args(
                sym::assign_cast_integer,
                vec![
                    operand::const_from_uint(tcx, bits),
                    operand::const_from_bool(tcx, is_signed),
                ],
            )
        }
    }

    fn to_float(&mut self, ty: Ty<'tcx>) {
        let (e_bits, s_bits) = ty::ebit_sbit_size(ty);
        self.add_bb_for_cast_assign_call_with_args(
            sym::assign_cast_float,
            vec![
                operand::const_from_uint(self.context.tcx(), e_bits),
                operand::const_from_uint(self.context.tcx(), s_bits),
            ],
        )
    }

    fn through_unsizing(&mut self) {
        self.add_bb_for_cast_assign_call(sym::assign_cast_unsize)
    }

    fn through_fn_ptr_coercion(&mut self) {
        if cfg!(feature = "abs_concrete") {
            // Effective only at compile time, no operational effect.
            self.by_use(self.context.operand_ref())
        } else {
            unimplemented!("Function pointer coercion is not supported in this configuration.")
        }
    }

    fn expose_prov(&mut self) {
        self.add_bb_for_cast_assign_call(sym::assign_cast_expose_prov);
    }

    fn with_exposed_prov(&mut self, ty: Ty<'tcx>) {
        self.add_bb_for_pointer_cast_assign_call(ty, sym::assign_cast_with_exposed_prov);
    }

    fn to_another_ptr(&mut self, ty: Ty<'tcx>, kind: CastKind) {
        use CastKind::*;
        use rustc_middle::ty::adjustment::PointerCoercion::*;
        debug_assert_matches!(
            kind,
            PtrToPtr | FnPtrToPtr | PointerCoercion(MutToConstPointer | ArrayToPointer, _)
        );
        /* NOTE: Currently, we do not distinguish between different pointer casts.
         * This is because they all keep the data untouched and are just about
         * semantics. We can add support for them later if interested. */
        self.add_bb_for_pointer_cast_assign_call(ty, sym::assign_cast_to_another_ptr);
    }

    fn transmuted(&mut self, ty: Ty<'tcx>) {
        let id_local = {
            let (block, id_local) = self.make_type_id_of_bb(ty);
            self.insert_blocks([block]);
            id_local
        };
        self.add_bb_for_cast_assign_call_with_args(
            sym::assign_cast_transmute,
            vec![operand::move_for_local(id_local)],
        )
    }

    fn subtyped(&mut self, ty: Ty<'tcx>) {
        let id_local = {
            let (block, id_local) = self.make_type_id_of_bb(ty);
            self.insert_blocks([block]);
            id_local
        };
        self.add_bb_for_cast_assign_call_with_args(
            sym::assign_cast_subtype,
            vec![operand::move_for_local(id_local)],
        )
    }
}

impl<'tcx, C> RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: CastOperandProvider + ForAssignment<'tcx>,
{
    fn add_bb_for_cast_assign_call(&mut self, func_name: LeafSymbol) {
        self.add_bb_for_cast_assign_call_with_args(func_name, vec![])
    }

    fn add_bb_for_cast_assign_call_with_args(
        &mut self,
        func_name: LeafSymbol,
        args: Vec<Operand<'tcx>>,
    ) {
        self.add_bb_for_assign_call(
            func_name,
            [
                vec![operand::copy_for_local(self.context.operand_ref().into())],
                args,
            ]
            .concat(),
        )
    }

    fn add_bb_for_pointer_cast_assign_call(&mut self, ty: Ty<'tcx>, func_name: LeafSymbol) {
        let id_local: Local = {
            let (block, id_local) = self.make_type_id_of_bb(ty);
            self.insert_blocks([block]);
            id_local
        };
        self.add_bb_for_cast_assign_call_with_args(
            func_name,
            vec![operand::move_for_local(id_local)],
        );
    }
}

mod utils {
    pub(super) use super::super::utils::{
        convert_mir_binop_to_pri, convert_mir_unop_to_pri, prepare_operand_for_slice, ty,
    };

    pub(super) mod operand {
        use rustc_middle::{mir::Const, mir::ConstOperand, ty};
        use rustc_span::DUMMY_SP;

        pub use super::super::super::utils::operand::*;

        use super::super::super::prelude::mir::*;

        pub fn const_from_existing_ty_const<'tcx>(
            ty: Ty<'tcx>,
            constant: ty::Const<'tcx>,
        ) -> Operand<'tcx> {
            const_from_existing(&Box::new(ConstOperand {
                span: DUMMY_SP,
                user_ty: None,
                const_: Const::Ty(ty, constant),
            }))
        }
    }
}
use utils::*;
