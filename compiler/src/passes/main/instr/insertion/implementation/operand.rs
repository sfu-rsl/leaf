use rustc_middle::mir::{ConstOperand, RuntimeChecks};

use common::{log_debug, log_warn};

use super::{
    OperandReferencer,
    config::{ConstantTypeRules, DetailDecision},
    ctxt_reqs::{Basic, ForOperandRef, ForPlaceRef},
    prelude::{mir::*, *},
    utils::ty::TyExt,
};

impl<'tcx, C> OperandReferencer<'tcx> for ProbeInserter<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: ForOperandRef<'tcx>,
{
    fn reference_operand(&mut self, operand: &Operand<'tcx>) -> OperandRef {
        let BlocksAndResult(new_blocks, reference) = self.internal_reference_operand(operand);
        self.insert_blocks(new_blocks);
        reference.into()
    }
}
#[allow(clippy::borrowed_box)]
impl<'tcx, C> ProbeInserter<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: Basic<'tcx> + SourceInfoProvider,
{
    pub(super) fn internal_reference_operand(
        &mut self,
        operand: &Operand<'tcx>,
    ) -> BlocksAndResult<'tcx>
    where
        C: ForOperandRef<'tcx>,
    {
        let config = &self.context.config().operand_info;

        use Operand::*;
        match operand {
            Copy(place) if config.copy.is_enabled() => {
                self.internal_reference_place_operand(place, true)
            }
            Move(place) if config.mov.is_enabled() => {
                self.internal_reference_place_operand(place, false)
            }
            Constant(constant) if config.constant.is_some() => {
                self.internal_reference_const_operand(constant)
            }
            RuntimeChecks(checks) if config.constant.is_some() => {
                self.internal_reference_runtime_checks(checks)
            }
            Copy(..) | Move(..) | Constant(..) | RuntimeChecks(..) => {
                self.internal_reference_operand_some()
            }
        }
    }

    fn internal_reference_operand_some(&mut self) -> BlocksAndResult<'tcx> {
        self.make_bb_for_helper_call_with_ret(
            self.pri_helper_funcs().ref_operand_some_encoded,
            Vec::default(),
        )
        .into()
    }

    fn internal_reference_place_operand(
        &mut self,
        place: &Place<'tcx>,
        is_copy: bool,
    ) -> BlocksAndResult<'tcx>
    where
        C: ForPlaceRef<'tcx>,
    {
        let BlocksAndResult(mut additional_blocks, mut place_ref) =
            self.internal_reference_place(place);

        let tcx = self.tcx();
        let ty = place.ty(&self.context, tcx).ty;
        if ty.is_adt() || ty.is_trivially_tuple() || ty.is_array() || !ty.is_known_rigid() {
            if let Some(BlocksAndResult(new_blocks, new_ref)) = self.add_place_size(place_ref, ty) {
                additional_blocks.extend(new_blocks);
                place_ref = new_ref;
            }
        }

        let func = if is_copy {
            self.pri_helper_funcs().ref_operand_copy_encoded
        } else {
            self.pri_helper_funcs().ref_operand_move_encoded
        };

        BlocksAndResult::from(
            self.make_bb_for_helper_call_with_ret(func, vec![operand::copy_for_local(place_ref)]),
        )
        .prepend(additional_blocks)
    }

    fn internal_reference_const_operand(
        &mut self,
        constant: &Box<ConstOperand<'tcx>>,
    ) -> BlocksAndResult<'tcx>
    where
        C: ForOperandRef<'tcx>,
    {
        let config = self.const_config();

        /* NOTE: Although there might be some ways to obtain/evaluate the values, we prefer to use
         * the constant as it is, and rely on high-level conversions to support all types of
         * constants in an abstract fashion. */
        let ty = constant.ty();
        let tcx = self.tcx();
        if ty.is_primitive() {
            self.internal_reference_const_primitive(constant)
        } else if ty.is_raw_ptr() {
            config
                .ptr
                .is_enabled()
                .then(|| self.internal_reference_const_ptr(constant))
        }
        // &str
        else if ty.is_imm_ref_str() {
            config.str.is_enabled().then(|| {
                self.internal_reference_const_operand_directly(sym::ref_operand_const_str, constant)
            })
        }
        // &[u8]
        else if ty::is_u8_slice_ref(tcx, ty) {
            config.byte_str.is_enabled().then(|| {
                self.internal_reference_const_operand_directly(
                    sym::ref_operand_const_byte_str,
                    constant,
                )
            })
        }
        // &[u8; N]
        else if ty::is_u8_array_ref(tcx, ty) {
            config
                .byte_str
                .is_enabled()
                .then(|| self.internal_reference_byte_str_const_operand(constant))
        }
        // NOTE: Check this after all other ZSTs that you want to distinguish.
        else if ty.size(tcx, self.current_typing_env()) == rustc_abi::Size::ZERO {
            config
                .zst
                .is_enabled()
                .then(|| self.internal_reference_zst_const_operand())
        } else if let TyKind::FnDef(..) = ty.kind() {
            unreachable!(
                "FnDef is expected to be a ZST and handled above, found {:?}",
                ty
            )
        } else if let Some(def_id) = Self::try_as_immut_static(tcx, constant) {
            log_debug!("Static reference constant is not transferred: {:?}", def_id);
            None
        } else {
            log_debug!(
                concat!(
                    "Encountered constant {:?} with a complex type {:?}. ",
                    "Please report if it is from a primitive type.",
                ),
                constant.const_,
                ty
            );
            None
        }
        .unwrap_or_else(|| self.internal_reference_const_some())
    }

    fn internal_reference_runtime_checks(&mut self, checks: &RuntimeChecks) -> BlocksAndResult<'tcx>
    where
        C: ForOperandRef<'tcx>,
    {
        let value = checks.value(self.tcx().sess);
        self.internal_reference_operand(&operand::const_from_bool(self.tcx(), value))
    }

    fn internal_reference_const_primitive(
        &mut self,
        constant: &Box<ConstOperand<'tcx>>,
    ) -> Option<BlocksAndResult<'tcx>> {
        let ty = constant.ty();
        debug_assert!(ty.is_primitive(), "Expected primitive type, found {:?}", ty);

        let config = self.const_config();
        if ty.is_bool() {
            config.bool.is_enabled().then(|| {
                self.make_bb_for_helper_call_with_ret(
                    self.pri_helper_funcs().ref_operand_const_bool_encoded,
                    vec![operand::const_from_existing(constant)],
                )
                .into()
            })
        } else if ty.is_char() {
            config.char.is_enabled().then(|| {
                self.internal_reference_const_operand_directly(
                    sym::ref_operand_const_char,
                    constant,
                )
            })
        } else if ty.is_integral() {
            config
                .int
                .is_enabled()
                .then(|| self.internal_reference_int_const_operand(constant))
        } else if ty.is_floating_point() {
            config
                .float
                .is_enabled()
                .then(|| self.internal_reference_float_const_operand(constant))
        } else {
            unreachable!()
        }
    }

    fn internal_reference_const_ptr(
        &mut self,
        constant: &Box<ConstOperand<'tcx>>,
    ) -> BlocksAndResult<'tcx> {
        let ty = constant.ty();
        debug_assert!(ty.is_raw_ptr(), "Expected raw pointer type, found {:?}", ty);

        let TyKind::RawPtr(pointee_ty, _) = ty.kind() else {
            unreachable!()
        };

        let tcx = self.tcx();

        if !pointee_ty.is_sized(tcx, self.current_typing_env()) {
            log_warn!("Unexpected constant fat pointer: {:?}", constant);
            return self.internal_reference_const_some().into();
        }

        let raw_ptr_ty = Ty::new_imm_ptr(tcx, tcx.types.unit);
        let local: Local = self.add_local(raw_ptr_ty);
        let assignment = assignment::create(
            Place::from(local),
            rvalue::cast_ptr_to_ptr(operand::const_from_existing(constant), raw_ptr_ty),
        );
        let (mut block, result) = self.make_bb_for_operand_ref_call(
            sym::ref_operand_const_addr,
            vec![operand::move_for_local(local)],
        );
        block.statements.push(assignment);
        (block, result).into()
    }

    pub(super) fn internal_reference_const_some(&mut self) -> BlocksAndResult<'tcx> {
        self.make_bb_for_helper_call_with_ret(
            self.pri_helper_funcs().ref_operand_const_some_encoded,
            Vec::default(),
        )
        .into()
    }

    fn internal_reference_const_operand_directly(
        &mut self,
        func_name: LeafSymbol,
        constant: &Box<ConstOperand<'tcx>>,
    ) -> BlocksAndResult<'tcx> {
        self.make_bb_for_operand_ref_call(func_name, vec![operand::const_from_existing(constant)])
            .into()
    }

    fn internal_reference_int_const_operand(
        &mut self,
        constant: &Box<ConstOperand<'tcx>>,
    ) -> BlocksAndResult<'tcx> {
        let ty = constant.ty();
        debug_assert!(ty.is_integral());

        let tcx = self.tcx();
        if ty.primitive_size(tcx) <= tcx.types.u64.primitive_size(tcx) {
            let mut blocks = Vec::new();

            let primitive_ty_local = {
                let (block, local) = self.make_primitive_type_of_bb(ty);
                blocks.push(block);
                local
            };

            let (bit_rep_local, additional_stmts) =
                utils::cast_int_to_bit_rep(&mut self.context, constant, tcx.types.u64);
            let (mut block, result) = self.make_bb_for_operand_ref_call(
                sym::ref_operand_const_int,
                vec![
                    operand::move_for_local(bit_rep_local),
                    operand::move_for_local(primitive_ty_local),
                ],
            );
            block.statements.extend(additional_stmts);
            blocks.push(block);

            BlocksAndResult(blocks, result)
        } else {
            let (bit_rep_local, additional_stmts) =
                utils::cast_int_to_bit_rep(&mut self.context, constant, tcx.types.u128);
            let (mut block, result) = self.make_bb_for_operand_ref_call(
                sym::ref_operand_const_int_arb,
                vec![
                    operand::move_for_local(bit_rep_local),
                    operand::const_from_uint(tcx, ty.primitive_size(tcx).bits()),
                    operand::const_from_bool(tcx, ty.is_signed()),
                ],
            );
            block.statements.extend(additional_stmts);
            (block, result).into()
        }
    }

    fn internal_reference_float_const_operand(
        &mut self,
        constant: &Box<ConstOperand<'tcx>>,
    ) -> BlocksAndResult<'tcx> {
        let ty = constant.ty();
        debug_assert!(ty.is_floating_point());

        let tcx = self.tcx();
        let (bit_rep_local, conversion_block) =
            utils::cast_float_to_bit_rep(tcx, &mut self.context, constant);
        let (e_bits, s_bits) = ty::ebit_sbit_size(ty);
        let block_pair = self.make_bb_for_operand_ref_call(
            sym::ref_operand_const_float,
            vec![
                operand::move_for_local(bit_rep_local),
                operand::const_from_uint(tcx, e_bits),
                operand::const_from_uint(tcx, s_bits),
            ],
        );
        BlocksAndResult::from(block_pair).prepend([conversion_block])
    }

    fn internal_reference_zst_const_operand(&mut self) -> BlocksAndResult<'tcx> {
        self.make_bb_for_helper_call_with_ret(
            self.pri_helper_funcs().ref_operand_const_zst_encoded,
            Default::default(),
        )
        .into()
    }

    fn internal_reference_byte_str_const_operand(
        &mut self,
        constant: &Box<ConstOperand<'tcx>>,
    ) -> BlocksAndResult<'tcx> {
        let tcx = self.tcx();
        let ty = constant.ty();
        debug_assert!(ty.is_ref() && ty.peel_refs().sequence_element_type(tcx) == tcx.types.u8);

        let (slice_local, slice_assignment) = cast_array_ref_to_slice(
            tcx,
            &mut self.context,
            operand::const_from_existing(constant),
            None,
        );
        let mut block_pair = self.make_bb_for_operand_ref_call(
            sym::ref_operand_const_byte_str,
            vec![operand::move_for_local(slice_local)],
        );
        block_pair.0.statements.insert(0, slice_assignment);
        BlocksAndResult::from(block_pair)
    }

    fn make_bb_for_operand_ref_call(
        &mut self,
        func_name: LeafSymbol,
        args: Vec<Operand<'tcx>>,
    ) -> (BasicBlockData<'tcx>, Local) {
        self.make_bb_for_call_with_ret(func_name, args)
    }

    #[inline]
    fn try_as_immut_static(tcx: TyCtxt<'tcx>, constant: &Box<ConstOperand<'tcx>>) -> Option<DefId> {
        /* Immutable statics are accessed by a constant reference which points to a statically
         * allocated program memory block. If the static item's type is T then the constant is
         * of type &T. */
        constant
            .ty()
            .ref_mutability()
            .is_some_and(|m| m.is_not())
            .then(|| constant.check_static_ptr(tcx))
            .flatten()
    }

    fn const_config(&self) -> &ConstantTypeRules<DetailDecision> {
        self.context
            .config()
            .operand_info
            .constant
            .as_ref()
            .unwrap()
    }
}

impl<'tcx, C> ProbeInserter<C>
where
    C: ForOperandRef<'tcx>,
{
    // Utility extension method to reference an operand with a spanned location.
    pub(super) fn reference_operand_spanned(
        &mut self,
        operand: &rustc_span::Spanned<Operand<'tcx>>,
    ) -> OperandRef {
        let source_scope = self.source_info().scope;
        let mut inserter = self.before();
        inserter
            .with_source_info(rustc_middle::mir::SourceInfo {
                span: operand.span,
                scope: source_scope,
            })
            .reference_operand(&operand.node)
    }
}

mod utils {
    use rustc_middle::mir::{CastKind, ConstOperand};

    pub(super) use super::super::prelude::{mir::*, *};

    pub(super) use super::super::utils::{assignment, cast_array_ref_to_slice, rvalue, terminator};

    pub(super) mod operand {
        pub use super::super::super::utils::operand::*;
    }

    pub(super) mod ty {
        pub use super::super::super::utils::ty::*;

        use super::*;

        pub fn is_u8_array_ref<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> bool {
            if let TyKind::Ref(_, ty, _) = ty.kind() {
                return ty.is_array() && ty.sequence_element_type(tcx) == tcx.types.u8;
            }

            false
        }

        pub fn is_u8_slice_ref<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> bool {
            // This is just for mitigating the bug in rustfmt. Track: rustfmt#5863
            if let TyKind::Ref(_, ty, _) = ty.kind() {
                if let TyKind::Slice(ty) = ty.kind() {
                    return *ty == tcx.types.u8;
                }
            }

            false
        }
    }

    #[allow(clippy::borrowed_box)]
    pub(super) fn cast_int_to_bit_rep<'tcx>(
        context: &mut impl BodyLocalManager<'tcx>,
        constant: &Box<ConstOperand<'tcx>>,
        dest_ty: Ty<'tcx>,
    ) -> (Local, [Statement<'tcx>; 1]) {
        debug_assert!(constant.ty().is_integral());
        let bit_rep_local = context.add_local(dest_ty);
        let bit_rep_assign = assignment::create(
            Place::from(bit_rep_local),
            Rvalue::Cast(
                CastKind::IntToInt,
                operand::const_from_existing(constant),
                dest_ty,
            ),
        );
        (bit_rep_local, [bit_rep_assign])
    }

    #[allow(clippy::borrowed_box)]
    pub(super) fn cast_float_to_bit_rep<'tcx>(
        tcx: TyCtxt<'tcx>,
        context: &mut (impl BodyLocalManager<'tcx> + PriItemsProvider<'tcx> + SourceInfoProvider),
        constant: &Box<ConstOperand<'tcx>>,
    ) -> (Local, BasicBlockData<'tcx>) {
        use rustc_middle::ty::FloatTy;
        debug_assert!(constant.ty().is_floating_point());
        let bit_rep_local = context.add_local(tcx.types.u128);
        let conversion_func = if matches!(constant.ty().kind(), TyKind::Float(FloatTy::F32)) {
            context.pri_helper_funcs().f32_to_bits
        } else {
            context.pri_helper_funcs().f64_to_bits
        };
        let block = BasicBlockData::new(
            Some(terminator::call(
                tcx,
                conversion_func.def_id,
                core::iter::empty(),
                vec![operand::const_from_existing(constant)],
                bit_rep_local.into(),
                None,
                context.source_info(),
            )),
            false,
        );
        (bit_rep_local, block)
    }
}
use utils::*;
