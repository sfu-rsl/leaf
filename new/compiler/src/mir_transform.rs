use std::{collections::HashMap, marker::PhantomData};

use rustc_apfloat::{
    ieee::{self},
    Float,
};
use rustc_ast::Mutability;
use rustc_const_eval::interpret::{ConstValue, Scalar};
use rustc_index::vec::IndexVec;
use rustc_middle::{
    mir::{
        BasicBlock, BasicBlockData, Body, Constant, ConstantKind, HasLocalDecls, Local, LocalDecl,
        LocalDecls, Operand, Place, ProjectionElem, SourceInfo, Terminator, TerminatorKind,
    },
    ty::{ScalarInt, Ty, TyCtxt},
};
use rustc_span::{Span, DUMMY_SP};

use crate::visit::{self, TerminatorKindMutVisitor};

use self::utils::*;

const NEXT_BLOCK: BasicBlock = BasicBlock::MAX;

pub struct BodyModificationUnit<'tcx> {
    nex_local_index: Local,
    new_locals: Vec<NewLocalDecl<'tcx>>,
    new_blocks: HashMap<BasicBlock, Vec<(BasicBlock, BasicBlockData<'tcx>)>>,
    new_block_count: u32,
}

impl<'tcx> BodyModificationUnit<'tcx> {
    pub fn new(nex_local_index: Local) -> Self {
        Self {
            nex_local_index: nex_local_index,
            new_locals: Vec::new(),
            new_blocks: HashMap::new(),
            new_block_count: 0,
        }
    }
}

struct NewLocalDecl<'tcx>(LocalDecl<'tcx>);

impl<'tcx> From<LocalDecl<'tcx>> for NewLocalDecl<'tcx> {
    fn from(value: LocalDecl<'tcx>) -> Self {
        NewLocalDecl(value)
    }
}

impl<'tcx> From<(Mutability, Ty<'tcx>, SourceInfo)> for NewLocalDecl<'tcx> {
    fn from(value: (Mutability, Ty<'tcx>, SourceInfo)) -> Self {
        LocalDecl {
            mutability: value.0,
            local_info: None,
            internal: true,
            is_block_tail: None,
            ty: value.1,
            user_ty: None,
            source_info: value.2,
        }
        .into()
    }
}

impl<'tcx> From<Ty<'tcx>> for NewLocalDecl<'tcx> {
    fn from(value: Ty<'tcx>) -> Self {
        (
            Mutability::Not,
            value,
            SourceInfo::outermost(rustc_span::DUMMY_SP),
        )
            .into()
    }
}

impl<'tcx> BodyModificationUnit<'tcx> {
    pub fn add_local<T>(&mut self, decl_info: T) -> Local
    where
        T: Into<NewLocalDecl<'tcx>>,
    {
        self.new_locals.push(decl_info.into());
        Local::from(self.nex_local_index + (self.new_locals.len() - 1))
    }
}

impl<'tcx> BodyModificationUnit<'tcx> {
    pub fn insert_blocks_before<I>(&mut self, index: BasicBlock, blocks: I) -> Vec<BasicBlock>
    where
        I: IntoIterator<Item = BasicBlockData<'tcx>>,
    {
        if !self.new_blocks.contains_key(&index) {
            self.new_blocks.insert(index, Vec::new());
        }

        let chunk = self.new_blocks.get_mut(&index).unwrap();
        let block_count: u32 = {
            let count_before = chunk.len();
            // Associating temporary indices to the new blocks, so they can be referenced if needed.
            chunk.extend(blocks.into_iter().enumerate().map(|(i, b)| {
                (
                    BasicBlock::from(u32::MAX - 1 - self.new_block_count - i as u32),
                    b,
                )
            }));
            (chunk.len() - count_before).try_into().unwrap()
        };
        self.new_block_count += block_count;
        Vec::from_iter(
            chunk[(chunk.len() - block_count as usize)..]
                .iter()
                .map(|x| x.0),
        )
    }
}

impl<'tcx> BodyModificationUnit<'tcx> {
    pub fn commit(self, body: &mut Body<'tcx>) {
        Self::add_new_locals(&mut body.local_decls, self.new_locals);

        if !self.new_blocks.is_empty() {
            let index_mapping = Self::insert_new_blocks(body.basic_blocks_mut(), self.new_blocks);
            Self::update_jumps(body.basic_blocks_mut(), index_mapping);
        }
    }

    fn add_new_locals(locals: &mut LocalDecls<'tcx>, new_locals: Vec<NewLocalDecl<'tcx>>) {
        let first_index = locals.len();
        for (i, local) in new_locals.into_iter().enumerate() {
            let index = locals.push(local.0);
            // Asserting that the indices that we have given are correct.
            assert_eq!(index, (i + first_index).into());
        }
    }

    fn insert_new_blocks<I>(
        blocks: &mut IndexVec<BasicBlock, BasicBlockData<'tcx>>,
        new_blocks: I,
    ) -> HashMap<BasicBlock, BasicBlock>
    where
        I: IntoIterator<Item = (BasicBlock, Vec<(BasicBlock, BasicBlockData<'tcx>)>)>,
    {
        let mut index_mapping = HashMap::<BasicBlock, BasicBlock>::new();
        let mut push = |blocks: &mut IndexVec<BasicBlock, BasicBlockData<'tcx>>,
                        block: BasicBlockData<'tcx>,
                        current_index: BasicBlock| {
            let new_index = blocks.push(block);
            if new_index != current_index {
                index_mapping.insert(current_index, new_index);
            }
        };

        let current_blocks = Vec::from_iter(blocks.drain(..));

        let mut new_blocks = Vec::from_iter(new_blocks);
        new_blocks.sort_by_key(|p| p.0);
        let mut new_blocks = new_blocks.into_iter().peekable();

        for (i, block) in current_blocks.into_iter().enumerate() {
            let i = BasicBlock::from(i);
            if new_blocks.peek().is_some_and(|(index, _)| *index == i) {
                let (_, chunk) = new_blocks.next().unwrap();
                blocks.extend_reserve(chunk.len());
                for (pseudo_index, block) in chunk {
                    push(blocks, block, pseudo_index);
                }
            }

            push(blocks, block, i);
        }

        // Currently, we only consider insertion of blocks before original blocks.
        assert!(new_blocks.peek().is_none());

        index_mapping
    }

    fn update_jumps(
        blocks: &mut IndexVec<BasicBlock, BasicBlockData<'tcx>>,
        index_mapping: HashMap<BasicBlock, BasicBlock>,
    ) {
        let mut updater = JumpUpdater {
            index_mapping: index_mapping,
            next_index: NEXT_BLOCK,
            phantom: PhantomData,
        };
        for (index, block) in blocks
            .iter_enumerated_mut()
            .filter(|(_, b)| b.terminator.is_some())
        {
            updater.next_index = index + 1;
            updater.update_terminator(block.terminator_mut());
        }
    }
}

struct JumpUpdater<'tcx> {
    index_mapping: HashMap<BasicBlock, BasicBlock>,
    next_index: BasicBlock,
    phantom: PhantomData<&'tcx ()>,
}

impl<'tcx> visit::TerminatorKindMutVisitor<'tcx, ()> for JumpUpdater<'tcx> {
    fn visit_goto(&mut self, target: &mut BasicBlock) {
        self.update(target);
    }

    fn visit_switch_int(
        &mut self,
        _discr: &mut rustc_middle::mir::Operand<'tcx>,
        targets: &mut rustc_middle::mir::SwitchTargets,
    ) {
        for target in targets.all_targets_mut() {
            self.update(&mut *target);
        }
    }

    fn visit_drop(
        &mut self,
        _place: &mut rustc_middle::mir::Place<'tcx>,
        target: &mut BasicBlock,
        unwind: &mut Option<BasicBlock>,
    ) {
        self.update(target);
        self.update_maybe(unwind);
    }

    fn visit_drop_and_replace(
        &mut self,
        _place: &mut rustc_middle::mir::Place<'tcx>,
        _value: &mut rustc_middle::mir::Operand<'tcx>,
        target: &mut BasicBlock,
        unwind: &mut Option<BasicBlock>,
    ) {
        self.update(target);
        self.update_maybe(unwind);
    }

    fn visit_assert(
        &mut self,
        _cond: &mut rustc_middle::mir::Operand<'tcx>,
        _expected: &mut bool,
        _msg: &mut rustc_middle::mir::AssertMessage<'tcx>,
        target: &mut BasicBlock,
        cleanup: &mut Option<BasicBlock>,
    ) {
        self.update(target);
        self.update_maybe(cleanup);
    }

    fn visit_yield(
        &mut self,
        _value: &mut rustc_middle::mir::Operand<'tcx>,
        resume: &mut BasicBlock,
        _resume_arg: &mut rustc_middle::mir::Place<'tcx>,
        drop: &mut Option<BasicBlock>,
    ) {
        self.update(resume);
        self.update_maybe(drop);
    }

    fn visit_false_edge(
        &mut self,
        real_target: &mut BasicBlock,
        imaginary_target: &mut BasicBlock,
    ) {
        self.update(real_target);
        self.update(imaginary_target);
    }

    fn visit_false_unwind(
        &mut self,
        real_target: &mut BasicBlock,
        unwind: &mut Option<BasicBlock>,
    ) {
        self.update(real_target);
        self.update_maybe(unwind);
    }

    fn visit_inline_asm(
        &mut self,
        _template: &mut &[rustc_ast::InlineAsmTemplatePiece],
        _operands: &mut Vec<rustc_middle::mir::InlineAsmOperand<'tcx>>,
        _options: &mut rustc_ast::InlineAsmOptions,
        _line_spans: &'tcx [Span],
        destination: &mut Option<BasicBlock>,
        cleanup: &mut Option<BasicBlock>,
    ) {
        self.update_maybe(destination);
        self.update_maybe(cleanup);
    }
}

impl<'tcx> JumpUpdater<'tcx> {
    pub fn update_terminator(&mut self, terminator: &mut Terminator<'tcx>) {
        Self::visit_terminator_kind(self, &mut terminator.kind)
    }

    fn update(&self, target: &mut BasicBlock) {
        if *target == NEXT_BLOCK {
            *target = self.next_index;
        } else if let Some(new_index) = self.index_mapping.get(target) {
            *target = *new_index;
        }
    }

    fn update_maybe(&self, target: &mut Option<BasicBlock>) {
        if let Some(t) = target.as_mut() {
            self.update(t);
        }
    }
}

pub struct RuntimeCallAdder<'tcx, 'm> {
    tcx: TyCtxt<'tcx>,
    modification_unit: &'m mut BodyModificationUnit<'tcx>,
}

impl<'tcx, 'm> RuntimeCallAdder<'tcx, 'm> {
    pub fn new(tcx: TyCtxt<'tcx>, modification_unit: &'m mut BodyModificationUnit<'tcx>) -> Self {
        Self {
            tcx: tcx,
            modification_unit: modification_unit,
        }
    }
}

struct BlocksAndResult<'tcx>(Vec<BasicBlockData<'tcx>>, Local);

impl<'tcx> BlocksAndResult<'tcx> {
    fn prepend(self, blocks: Vec<BasicBlockData<'tcx>>) -> Self {
        Self([blocks, self.0].concat(), self.1)
    }
}

impl<'tcx> From<(BasicBlockData<'tcx>, Local)> for BlocksAndResult<'tcx> {
    fn from(value: (BasicBlockData<'tcx>, Local)) -> Self {
        BlocksAndResult(vec![value.0], value.1)
    }
}

impl<'tcx, 'm> RuntimeCallAdder<'tcx, 'm> {
    pub fn reference_place(&mut self, place: &Place<'tcx>, location: BasicBlock) -> Local {
        let BlocksAndResult(new_blocks, reference) = self.internal_reference_place(place);
        self.modification_unit
            .insert_blocks_before(location, new_blocks);
        reference
    }

    fn internal_reference_place(&mut self, place: &Place<'tcx>) -> BlocksAndResult<'tcx> {
        let mut new_blocks = vec![];
        let (call_block, mut current_ref) = self.make_bb_for_call_with_ret(
            stringify!(pri::ref_place_local),
            vec![operand::const_from_uint(self.tcx, u32::from(place.local))],
        );
        new_blocks.push(call_block);

        for (_, proj) in place.iter_projections() {
            let BlocksAndResult(added_blocks, wrapped_ref) =
                self.reference_place_projection(current_ref, proj);
            current_ref = wrapped_ref;
            new_blocks.extend(added_blocks);
        }

        BlocksAndResult(new_blocks, current_ref)
    }

    fn reference_place_projection<T>(
        &mut self,
        current_ref: Local,
        proj: ProjectionElem<Local, T>,
    ) -> BlocksAndResult<'tcx> {
        let mut new_blocks = Vec::new();

        let (func_name, additional_args) = match proj {
            ProjectionElem::Deref => (stringify!(pri::ref_place_deref), vec![]),
            ProjectionElem::Field(index, _) => (
                stringify!(pri::ref_place_field),
                vec![operand::const_from_uint(self.tcx, u32::from(index))],
            ),
            ProjectionElem::Index(index) => {
                let BlocksAndResult(additional_blocks, index_ref) =
                    self.internal_reference_place(&Place::from(index));
                new_blocks.extend(additional_blocks);
                (
                    stringify!(pri::ref_place_index),
                    vec![operand::copy_for_local(index_ref)],
                )
            }
            ProjectionElem::ConstantIndex {
                offset,
                min_length,
                from_end,
            } => (
                stringify!(pri::ref_place_constant_index),
                vec![
                    operand::const_from_uint(self.tcx, offset),
                    operand::const_from_uint(self.tcx, min_length),
                    operand::const_from_bool(self.tcx, from_end),
                ],
            ),
            ProjectionElem::Subslice { from, to, from_end } => (
                stringify!(pri::ref_place_subslice),
                vec![
                    operand::const_from_uint(self.tcx, from),
                    operand::const_from_uint(self.tcx, to),
                    operand::const_from_bool(self.tcx, from_end),
                ],
            ),
            ProjectionElem::Downcast(_, index) => (
                stringify!(pri::ref_place_downcast),
                vec![operand::const_from_uint(self.tcx, u32::from(index))],
            ),
            ProjectionElem::OpaqueCast(_) => (stringify!(pri::ref_place_opaque_cast), vec![]),
        };

        BlocksAndResult::from(self.make_bb_for_call_with_ret(
            func_name,
            [vec![operand::copy_for_local(current_ref)], additional_args].concat(),
        ))
        .prepend(new_blocks)
    }

    fn get_pri_place_ref_ty(&self) -> Ty<'tcx> {
        todo!()
    }

    fn get_pri_operand_ref_ty(&self) -> Ty<'tcx> {
        todo!()
    }
}

impl<'tcx, 'm> RuntimeCallAdder<'tcx, 'm> {
    pub fn reference_operand(&mut self, operand: &Operand<'tcx>, location: BasicBlock) -> Local {
        let BlocksAndResult(new_blocks, reference) = self.internal_reference_operand(operand);
        self.modification_unit
            .insert_blocks_before(location, new_blocks);
        reference
    }

    fn internal_reference_operand(&mut self, operand: &Operand<'tcx>) -> BlocksAndResult<'tcx> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                let BlocksAndResult(additional_blocks, place_ref) =
                    self.internal_reference_place(place);

                let func_name = if let Operand::Copy(_) = operand {
                    stringify!(pri::ref_operand_copy)
                } else {
                    stringify!(pri::ref_operand_move)
                };

                BlocksAndResult::from(
                    self.make_bb_for_call_with_ret(
                        func_name,
                        vec![operand::copy_for_local(place_ref)],
                    ),
                )
                .prepend(additional_blocks)
            }
            Operand::Constant(constant) => self.internal_reference_const_operand(&constant),
        }
    }

    fn internal_reference_const_operand(
        &mut self,
        constant: &Constant<'tcx>,
    ) -> BlocksAndResult<'tcx> {
        let kind = constant.literal;
        match kind {
            ConstantKind::Ty(_) => todo!(),
            ConstantKind::Unevaluated(_, _) => todo!(),
            ConstantKind::Val(value, ty) => self.internal_reference_val_const_operand(value, ty),
        }
    }

    fn internal_reference_val_const_operand(
        &mut self,
        value: ConstValue<'tcx>,
        ty: Ty<'tcx>,
    ) -> BlocksAndResult<'tcx> {
        match value {
            ConstValue::Scalar(scalar) => self.internal_reference_scalar_const_operand(scalar, ty),
            ConstValue::ZeroSized => todo!(),
            ConstValue::Slice { data, start, end } => todo!(),
            ConstValue::ByRef { alloc, offset } => todo!(),
        }
    }

    fn internal_reference_scalar_const_operand(
        &mut self,
        scalar: Scalar,
        ty: Ty<'tcx>,
    ) -> BlocksAndResult<'tcx> {
        match scalar {
            Scalar::Int(int) => self.internal_reference_scalar_int_const_operand(int, ty),
            Scalar::Ptr(_, _) => todo!(),
        }
    }

    fn internal_reference_scalar_int_const_operand(
        &mut self,
        scalar: ScalarInt,
        ty: Ty<'tcx>,
    ) -> BlocksAndResult<'tcx> {
        if ty.is_bool() {
            self.make_bb_for_call_with_ret(
                stringify!(pri::ref_operand_const_bool),
                vec![operand::const_from_scalar_int(self.tcx, scalar.clone(), ty)],
            )
        } else if ty.is_integral() {
            self.make_bb_for_call_with_ret(
                stringify!(pri::ref_operand_const_int),
                vec![
                    operand::const_from_scalar_int(self.tcx, scalar.clone(), ty),
                    operand::const_from_uint(self.tcx, scalar.size().bits()),
                    operand::const_from_bool(self.tcx, ty.is_signed()),
                ],
            )
        } else if ty.is_floating_point() {
            let bits = scalar.size().bits();
            let ebits = if bits == ieee::Single::BITS as u64 {
                ieee::Single::PRECISION
            } else {
                ieee::Double::PRECISION
            } as u64;
            let sbits = bits - ebits;
            self.make_bb_for_call_with_ret(
                stringify!(pri::ref_operand_const_float),
                vec![
                    operand::const_from_scalar_int(self.tcx, scalar.clone(), ty),
                    operand::const_from_uint(self.tcx, ebits),
                    operand::const_from_uint(self.tcx, sbits),
                ],
            )
        } else {
            unreachable!("ScalarInt is supposed to be either bool, int, or float.")
        }
        .into()
    }
}

impl<'tcx, 'm> RuntimeCallAdder<'tcx, 'm> {
    fn make_bb_for_call_with_ret(
        &mut self,
        func_name: &str,
        args: Vec<Operand<'tcx>>,
    ) -> (BasicBlockData<'tcx>, Local) {
        let result_local = self
            .modification_unit
            .add_local(self.get_pri_place_ref_ty());

        (
            self.make_bb_for_call(func_name, args, Place::from(result_local)),
            result_local,
        )
    }

    fn make_bb_for_call(
        &self,
        func_name: &str,
        args: Vec<Operand<'tcx>>,
        destination: Place<'tcx>,
    ) -> BasicBlockData<'tcx> {
        BasicBlockData::new(Some(self.make_call_terminator(
            func_name,
            args,
            destination,
        )))
    }

    fn make_call_terminator(
        &self,
        func_name: &str,
        args: Vec<Operand<'tcx>>,
        destination: Place<'tcx>,
    ) -> Terminator<'tcx> {
        Terminator {
            source_info: SourceInfo::outermost(DUMMY_SP),
            kind: TerminatorKind::Call {
                func: Operand::function_handle(
                    self.tcx,
                    self.get_pri_func_def_id(func_name),
                    std::iter::empty(),
                    DUMMY_SP,
                ),
                args: args,
                destination: destination,
                target: Some(NEXT_BLOCK),
                cleanup: None,
                from_hir_call: true,
                fn_span: DUMMY_SP,
            },
        }
    }

    fn get_pri_func_def_id(&self, func_name: &str) -> rustc_span::def_id::DefId {
        todo!()
    }

    fn get_pri_return_ty(&self, func_name: &str) -> Ty<'tcx> {
        todo!()
    }
}

mod utils {

    pub mod operand {

        use std::mem::size_of;

        use rustc_const_eval::interpret::Scalar;
        use rustc_middle::{
            mir::{Local, Operand, Place},
            ty::{ScalarInt, Ty, TyCtxt},
        };
        use rustc_span::DUMMY_SP;
        use rustc_type_ir::UintTy;

        pub fn const_from_uint<'tcx, T>(tcx: TyCtxt<'tcx>, value: T) -> Operand<'tcx>
        where
            T: Into<u128>,
        {
            const_from_scalar_int(
                tcx,
                ScalarInt::try_from_uint(value, rustc_abi::Size::from_bytes(size_of::<T>()))
                    .unwrap(),
                tcx.mk_mach_uint(
                    [
                        UintTy::U8,
                        UintTy::U16,
                        UintTy::U32,
                        UintTy::U64,
                        UintTy::U128,
                    ]
                    .into_iter()
                    .find(|t| (t.bit_width().unwrap() / 8) as usize == size_of::<T>())
                    .unwrap(),
                ),
            )
        }

        pub fn const_from_bool<'tcx>(tcx: TyCtxt<'tcx>, value: bool) -> Operand<'tcx> {
            const_from_scalar_int(tcx, ScalarInt::from(value), tcx.types.bool)
        }

        pub fn const_from_scalar_int<'tcx>(
            tcx: TyCtxt<'tcx>,
            value: ScalarInt,
            ty: Ty<'tcx>,
        ) -> Operand<'tcx> {
            Operand::const_from_scalar(tcx, ty, Scalar::Int(value), DUMMY_SP)
        }

        pub fn copy_for_local<'tcx>(value: Local) -> Operand<'tcx> {
            for_local(value, true)
        }

        pub fn for_local<'tcx>(value: Local, copy: bool) -> Operand<'tcx> {
            let place = Place::from(value);
            if copy {
                Operand::Copy(place)
            } else {
                Operand::Move(Place::from(place))
            }
        }
    }
}
