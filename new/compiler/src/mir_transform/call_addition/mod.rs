use std::fmt::Debug;

use rustc_apfloat::{
    ieee::{self},
    Float,
};
use rustc_const_eval::interpret::{ConstValue, Scalar};
use rustc_middle::{
    mir::{
        BasicBlock, BasicBlockData, BinOp, Constant, ConstantKind, Local, Operand, Place,
        ProjectionElem, SourceInfo, Statement, Terminator, TerminatorKind, UnOp,
    },
    ty::{ScalarInt, Ty, TyCtxt},
};
use rustc_span::DUMMY_SP;

use self::{context::*, utils::*};
use super::modification::{self, BodyBlockManager, BodyLocalManager, BodyModificationUnit};

pub mod context;

/*
 * Contexts and RuntimeCallAdder.
 * Based on the location and the statement we are going to add runtime calls for,
 * there are some data that are required to be passed to the runtime or used in
 * MIR generation. We place these data in a `Context` and `RuntimeCallAdder`
 * capabilities is determined by this context. For example, if the information
 * for a destination place (left hand side of an assignment) is available in the
 * current context, then `RuntimeCallAdder` will be able to generate basic blocks
 * corresponding to calling the assignment functions in the runtime library.
 */

/*
 * The following traits are meant for definition of features that we expect
 * from `RuntimeCallAdder` for various call adding situations.
 */

pub trait MirCallAdder<'tcx> {
    fn make_bb_for_call(
        &mut self,
        func_name: &str,
        args: Vec<Operand<'tcx>>,
    ) -> BasicBlockData<'tcx>;

    fn make_bb_for_call_with_ret(
        &mut self,
        func_name: &str,
        args: Vec<Operand<'tcx>>,
    ) -> (BasicBlockData<'tcx>, Local);
}

pub trait BlockInserter<'tcx> {
    fn insert_blocks<I>(&mut self, blocks: I) -> Vec<BasicBlock>
    where
        I: IntoIterator<Item = BasicBlockData<'tcx>>;
}

/*
 * These wrappers just ensure the semantics for the runtime call adder and
 * prevent interchangeably using them.
 * Note that these types are different from what pri has declared. They are
 * direct aliases for interface clarification but these are separate structures
 * that provide stricter interface rules.
 */
macro_rules! make_local_wrapper {
    ($name:ident) => {
        #[derive(Clone, Copy)]
        pub struct $name(Local);
        impl From<Local> for $name {
            fn from(value: Local) -> Self {
                Self(value)
            }
        }
        impl Into<Local> for $name {
            fn into(self) -> Local {
                self.0
            }
        }
    };
}
make_local_wrapper!(PlaceRef);
make_local_wrapper!(OperandRef);

pub trait PlaceReferencer<'tcx>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
{
    fn reference_place(&mut self, place: &Place<'tcx>) -> PlaceRef;
}

pub trait OperandReferencer<'tcx> {
    fn reference_operand(&mut self, operand: &Operand<'tcx>) -> OperandRef;
}

pub trait Assigner {
    fn by_use(&mut self, operand: OperandRef);

    fn by_repeat(&mut self, operand: OperandRef, count: u64);

    fn by_ref(&mut self, place: PlaceRef, is_mutable: bool);

    fn by_thread_local_ref(&mut self);

    fn by_address_of(&mut self, place: PlaceRef, is_mutable: bool);

    fn by_len(&mut self, place: PlaceRef);

    fn by_cast_numeric(&mut self, operand: OperandRef, is_to_float: bool, size: u64);

    fn by_cast(&mut self, operand: OperandRef, is_to_float: bool, size: u64);

    fn by_binary_op(
        &mut self,
        operator: &BinOp,
        first: OperandRef,
        second: OperandRef,
        checked: bool,
    );

    fn by_unary_op(&mut self, operator: &UnOp, operand: OperandRef);

    fn by_discriminant(&mut self, place: PlaceRef);

    fn by_aggregate_array(&mut self, items: &[OperandRef]);
}

pub struct RuntimeCallAdder<C> {
    context: C,
}

impl<'tcx, 'm> RuntimeCallAdder<DefaultContext<'tcx, 'm>> {
    pub fn new(tcx: TyCtxt<'tcx>, modification_unit: &'m mut BodyModificationUnit<'tcx>) -> Self {
        Self {
            context: DefaultContext::new(tcx, modification_unit),
        }
    }
}

impl<C> RuntimeCallAdder<C> {
    pub fn at(&mut self, location: BasicBlock) -> RuntimeCallAdder<AtLocationContext<C>> {
        RuntimeCallAdder {
            context: AtLocationContext {
                base: &mut self.context,
                location: location,
            },
        }
    }

    pub fn assign(&mut self, dest_ref: PlaceRef) -> RuntimeCallAdder<AssignmentContext<C>> {
        RuntimeCallAdder {
            context: AssignmentContext {
                base: &mut self.context,
                dest_ref,
            },
        }
    }

    pub fn borrow_from(other: &mut RuntimeCallAdder<C>) -> RuntimeCallAdder<TransparentContext<C>> {
        RuntimeCallAdder {
            context: TransparentContext {
                base: &mut other.context,
            },
        }
    }
}

impl<'tcx, C> MirCallAdder<'tcx> for RuntimeCallAdder<C>
where
    C: BodyLocalManager<'tcx> + TyContextProvider<'tcx> + FunctionInfoProvider<'tcx>,
{
    fn make_bb_for_call(
        &mut self,
        func_name: &str,
        args: Vec<Operand<'tcx>>,
    ) -> BasicBlockData<'tcx> {
        self.make_bb_for_call_with_ret(func_name, args).0
    }

    fn make_bb_for_call_with_ret(
        &mut self,
        func_name: &str,
        args: Vec<Operand<'tcx>>,
    ) -> (BasicBlockData<'tcx>, Local) {
        let result_local = self
            .context
            .add_local(self.context.get_pri_func_info(func_name).ret_ty);

        (
            self.make_call_bb(func_name, args, Place::from(result_local)),
            result_local,
        )
    }
}

impl<'tcx, C> RuntimeCallAdder<C>
where
    C: TyContextProvider<'tcx> + FunctionInfoProvider<'tcx>,
{
    fn make_call_bb(
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
                /*
                 * NOTE: Check if it is supposed to be the same operand for each function definition,
                 * i.e. caching/lazy singleton.
                 */
                func: Operand::function_handle(
                    self.context.tcx(),
                    self.context.get_pri_func_info(func_name).def_id,
                    std::iter::empty(),
                    DUMMY_SP,
                ),
                args: args,
                destination: destination,
                target: Some(modification::NEXT_BLOCK),
                cleanup: None,
                from_hir_call: true,
                fn_span: DUMMY_SP,
            },
        }
    }
}

impl<'tcx, C> BlockInserter<'tcx> for RuntimeCallAdder<C>
where
    C: BodyBlockManager<'tcx> + LocationProvider,
{
    fn insert_blocks<I>(&mut self, blocks: I) -> Vec<BasicBlock>
    where
        I: IntoIterator<Item = BasicBlockData<'tcx>>,
    {
        self.context
            .insert_blocks_before(self.context.location(), blocks)
    }
}

impl<'tcx, C> PlaceReferencer<'tcx> for RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: TyContextProvider<'tcx>,
{
    fn reference_place(&mut self, place: &Place<'tcx>) -> PlaceRef {
        let BlocksAndResult(new_blocks, reference) = self.internal_reference_place(place);
        self.insert_blocks(new_blocks);
        reference.into()
    }
}

impl<'tcx, C> RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: TyContextProvider<'tcx>,
{
    fn internal_reference_place(&mut self, place: &Place<'tcx>) -> BlocksAndResult<'tcx> {
        let mut new_blocks = vec![];
        let (call_block, mut current_ref) = self.make_bb_for_place_ref_call(
            stringify!(pri::ref_place_local),
            vec![operand::const_from_uint(
                self.context.tcx(),
                u32::from(place.local),
            )],
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
                vec![operand::const_from_uint(
                    self.context.tcx(),
                    u32::from(index),
                )],
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
                    operand::const_from_uint(self.context.tcx(), offset),
                    operand::const_from_uint(self.context.tcx(), min_length),
                    operand::const_from_bool(self.context.tcx(), from_end),
                ],
            ),
            ProjectionElem::Subslice { from, to, from_end } => (
                stringify!(pri::ref_place_subslice),
                vec![
                    operand::const_from_uint(self.context.tcx(), from),
                    operand::const_from_uint(self.context.tcx(), to),
                    operand::const_from_bool(self.context.tcx(), from_end),
                ],
            ),
            ProjectionElem::Downcast(_, index) => (
                stringify!(pri::ref_place_downcast),
                vec![operand::const_from_uint(
                    self.context.tcx(),
                    u32::from(index),
                )],
            ),
            ProjectionElem::OpaqueCast(_) => (stringify!(pri::ref_place_opaque_cast), vec![]),
        };

        BlocksAndResult::from(self.make_bb_for_place_ref_call(
            func_name,
            [vec![operand::copy_for_local(current_ref)], additional_args].concat(),
        ))
        .prepend(new_blocks)
    }

    fn make_bb_for_place_ref_call(
        &mut self,
        func_name: &str,
        args: Vec<Operand<'tcx>>,
    ) -> (BasicBlockData<'tcx>, Local) {
        self.make_bb_for_call_with_ret(func_name, args)
    }
}

impl<'tcx, C> OperandReferencer<'tcx> for RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: TyContextProvider<'tcx>,
{
    fn reference_operand(&mut self, operand: &Operand<'tcx>) -> OperandRef {
        let BlocksAndResult(new_blocks, reference) = self.internal_reference_operand(operand);
        self.insert_blocks(new_blocks);
        reference.into()
    }
}

impl<'tcx, C> RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: TyContextProvider<'tcx>,
{
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

                BlocksAndResult::from(self.make_bb_for_operand_ref_call(
                    func_name,
                    vec![operand::copy_for_local(place_ref)],
                ))
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
            self.make_bb_for_operand_ref_call(
                stringify!(pri::ref_operand_const_bool),
                vec![operand::const_from_scalar_int(
                    self.context.tcx(),
                    scalar.clone(),
                    ty,
                )],
            )
        } else if ty.is_integral() {
            self.make_bb_for_operand_ref_call(
                stringify!(pri::ref_operand_const_int),
                vec![
                    operand::const_from_uint(
                        self.context.tcx(),
                        /* Currently no direct way to read the data field. */
                        scalar.assert_bits(scalar.size()),
                    ),
                    operand::const_from_uint(self.context.tcx(), scalar.size().bits()),
                    operand::const_from_bool(self.context.tcx(), ty.is_signed()),
                ],
            )
        } else if ty.is_floating_point() {
            let bit_size = scalar.size().bits();
            let ebit_size = if bit_size == ieee::Single::BITS as u64 {
                ieee::Single::PRECISION
            } else {
                ieee::Double::PRECISION
            } as u64;
            let sbits = bit_size - ebit_size;
            self.make_bb_for_operand_ref_call(
                stringify!(pri::ref_operand_const_float),
                vec![
                    operand::const_from_uint(
                        self.context.tcx(),
                        /* Currently no direct way to read the data field. */
                        scalar.assert_bits(scalar.size()),
                    ),
                    operand::const_from_uint(self.context.tcx(), ebit_size),
                    operand::const_from_uint(self.context.tcx(), sbits),
                ],
            )
        } else if ty.is_char() {
            self.make_bb_for_operand_ref_call(
                stringify!(pri::ref_operand_const_char),
                vec![operand::const_from_scalar_int(
                    self.context.tcx(),
                    scalar.clone(),
                    ty,
                )],
            )
        } else {
            unreachable!("ScalarInt is supposed to be either bool, int, float, or char.")
        }
        .into()
    }

    fn make_bb_for_operand_ref_call(
        &mut self,
        func_name: &str,
        args: Vec<Operand<'tcx>>,
    ) -> (BasicBlockData<'tcx>, Local) {
        self.make_bb_for_call_with_ret(func_name, args)
    }
}

impl<'tcx, C> Assigner for RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: DestinationReferenceProvider + LocationProvider + BaseContext<'tcx>,
{
    fn by_use(&mut self, operand: OperandRef) {
        self.add_bb_for_assign_call(
            stringify!(pri::assign_use),
            vec![operand::copy_for_local(operand.into())],
        )
    }

    fn by_repeat(&mut self, operand: OperandRef, count: u64) {
        self.add_bb_for_assign_call(
            stringify!(pri::assign_repeat),
            vec![
                operand::copy_for_local(operand.into()),
                operand::const_from_uint(self.context.tcx(), count),
            ],
        )
    }

    fn by_ref(&mut self, place: PlaceRef, is_mutable: bool) {
        self.add_bb_for_assign_call(
            stringify!(pri::assign_ref),
            vec![
                operand::copy_for_local(place.into()),
                operand::const_from_bool(self.context.tcx(), is_mutable),
            ],
        )
    }

    fn by_thread_local_ref(&mut self) {
        todo!()
    }

    fn by_address_of(&mut self, place: PlaceRef, is_mutable: bool) {
        self.add_bb_for_assign_call(
            stringify!(pri::assign_address_of),
            vec![
                operand::copy_for_local(place.into()),
                operand::const_from_bool(self.context.tcx(), is_mutable),
            ],
        )
    }

    fn by_len(&mut self, place: PlaceRef) {
        self.add_bb_for_assign_call(
            stringify!(pri::assign_len),
            vec![operand::copy_for_local(place.into())],
        )
    }

    fn by_cast_numeric(&mut self, operand: OperandRef, is_to_float: bool, size: u64) {
        self.add_bb_for_assign_call(
            stringify!(pri::assign_cast_numeric),
            vec![
                operand::copy_for_local(operand.into()),
                operand::const_from_bool(self.context.tcx(), is_to_float),
                operand::const_from_uint(self.context.tcx(), size),
            ],
        )
    }

    fn by_cast(&mut self, operand: OperandRef, is_to_float: bool, size: u64) {
        todo!()
    }

    fn by_binary_op(
        &mut self,
        operator: &BinOp,
        first: OperandRef,
        second: OperandRef,
        checked: bool,
    ) {
        let operator = convert_mir_binop_to_pri(operator);
        let (operator_local, additional_statements) =
            self.add_and_set_local_for_enum(self.context.pri_special_types().binary_op, operator);

        self.add_bb_for_assign_call_with_statements(
            stringify!(pri::assign_binary_op),
            vec![
                operand::move_for_local(operator_local),
                operand::copy_for_local(first.into()),
                operand::copy_for_local(second.into()),
                operand::const_from_bool(self.context.tcx(), checked),
            ],
            additional_statements,
        )
    }

    fn by_unary_op(&mut self, operator: &UnOp, operand: OperandRef) {
        let operator = convert_mir_unop_to_pri(operator);
        let (operator_local, additional_statements) =
            self.add_and_set_local_for_enum(self.context.pri_special_types().unary_op, operator);

        self.add_bb_for_assign_call_with_statements(
            stringify!(pri::assign_unary_op),
            vec![
                operand::move_for_local(operator_local),
                operand::copy_for_local(operand.into()),
            ],
            additional_statements,
        )
    }

    fn by_discriminant(&mut self, place: PlaceRef) {
        self.add_bb_for_assign_call(
            stringify!(pri::assign_discriminant),
            vec![operand::copy_for_local(place.into())],
        )
    }

    fn by_aggregate_array(&mut self, items: &[OperandRef]) {
        let tcx = self.context.tcx();
        let operand_ref_ty = self.context.pri_special_types().operand_ref;
        let items_local = self
            .context
            .add_local(tcx.mk_array(operand_ref_ty, items.len() as u64));
        assignment::array_of_locals_by_move(
            Place::from(items_local),
            operand_ref_ty,
            &items
                .iter()
                .map(|i| (*i).into())
                .collect::<Vec<Local>>()
                .as_slice(),
        );
        let items_ref_local = self.context.add_local(
            self.context
                .tcx()
                .mk_imm_ref(tcx.lifetimes.re_erased, operand_ref_ty),
        );
        assignment::ref_of(Place::from(items_ref_local), Place::from(items_local), tcx);

        self.add_bb_for_assign_call(
            stringify!(pri::assign_binary_op),
            vec![operand::move_for_local(items_ref_local)],
        )
    }
}

impl<'tcx, C> RuntimeCallAdder<C>
where
    Self: MirCallAdder<'tcx> + BlockInserter<'tcx>,
    C: DestinationReferenceProvider + BodyLocalManager<'tcx>,
{
    fn add_bb_for_assign_call(&mut self, func_name: &str, args: Vec<Operand<'tcx>>) {
        self.add_bb_for_assign_call_with_statements(func_name, args, vec![])
    }

    fn add_bb_for_assign_call_with_statements(
        &mut self,
        func_name: &str,
        args: Vec<Operand<'tcx>>,
        statements: Vec<Statement<'tcx>>,
    ) {
        let mut block = self.make_bb_for_assign_call(func_name, args);
        block.statements.extend(statements);
        self.insert_blocks([block]);
    }

    fn make_bb_for_assign_call(
        &mut self,
        func_name: &str,
        args: Vec<Operand<'tcx>>,
    ) -> BasicBlockData<'tcx> {
        self.make_bb_for_call(
            func_name,
            [
                vec![operand::copy_for_local(self.context.dest_ref().into())],
                args,
            ]
            .concat(),
        )
    }

    fn add_and_set_local_for_enum<T>(
        &mut self,
        enum_ty: Ty<'tcx>,
        value: T,
    ) -> (Local, Vec<Statement<'tcx>>)
    where
        T: Debug,
    {
        let local = self.context.add_local(enum_ty);
        let statements =
            enums::set_variant_to_local(enum_ty, format!("{:?}", value).as_str(), local);
        (local, statements)
    }
}

/*
 * Context requirements work as aliases for context traits to guarantee that a
 * certain feature will be available in `RuntimeCallAdder` when its context
 * implement that set of traits.
 */
pub mod context_requirements {
    use super::{context::*, *};

    pub trait Basic<'tcx>:
        TyContextProvider<'tcx>
        + BodyLocalManager<'tcx>
        + BodyBlockManager<'tcx>
        + FunctionInfoProvider<'tcx>
        + SpecialTypesProvider<'tcx>
    {
    }
    impl<'tcx, C> Basic<'tcx> for C where
        C: TyContextProvider<'tcx>
            + BodyLocalManager<'tcx>
            + BodyBlockManager<'tcx>
            + FunctionInfoProvider<'tcx>
            + SpecialTypesProvider<'tcx>
    {
    }

    pub trait ForPlaceRef<'tcx>: LocationProvider + BaseContext<'tcx> {}
    impl<'tcx, C> ForPlaceRef<'tcx> for C where C: LocationProvider + BaseContext<'tcx> {}

    pub trait ForOperandRef<'tcx>: LocationProvider + BaseContext<'tcx> {}
    impl<'tcx, C> ForOperandRef<'tcx> for C where C: LocationProvider + BaseContext<'tcx> {}

    pub trait ForAssignment<'tcx>:
        DestinationReferenceProvider + LocationProvider + BaseContext<'tcx>
    {
    }
    impl<'tcx, C> ForAssignment<'tcx> for C where
        C: DestinationReferenceProvider + LocationProvider + BaseContext<'tcx>
    {
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

mod utils {
    use rustc_middle::mir;

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

        pub fn move_for_local<'tcx>(value: Local) -> Operand<'tcx> {
            for_local(value, false)
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

    pub mod enums {
        use rustc_middle::{
            mir::{Local, Place, SourceInfo, Statement},
            ty::{Ty, TyKind},
        };
        use rustc_span::DUMMY_SP;
        use rustc_target::abi::VariantIdx;

        pub fn set_variant_to_local<'tcx>(
            enum_ty: Ty<'tcx>,
            variant_name: &str,
            local: Local,
        ) -> Vec<Statement<'tcx>> {
            let place = Place::from(local);

            let deinit = Statement {
                source_info: SourceInfo::outermost(DUMMY_SP),
                kind: rustc_middle::mir::StatementKind::Deinit(Box::new(place)),
            };

            let disc = Statement {
                source_info: SourceInfo::outermost(DUMMY_SP),
                kind: rustc_middle::mir::StatementKind::SetDiscriminant {
                    place: Box::new(place),
                    variant_index: get_variant_index_by_name(enum_ty, variant_name),
                },
            };

            vec![deinit, disc]
        }

        pub fn get_variant_index_by_name<'tcx>(ty: Ty<'tcx>, variant_name: &str) -> VariantIdx {
            let adt_def = match ty.kind() {
                TyKind::Adt(def, _) => def,
                _ => unreachable!(),
            };
            let y = adt_def
                .variants()
                .iter()
                .find(|v| v.name.as_str() == variant_name)
                .expect(
                    format!("Variant could not be found with name `{}`.", variant_name).as_str(),
                );
            adt_def.variant_index_with_ctor_id(y.def_id)
        }
    }

    pub mod assignment {
        use rustc_middle::{
            mir::{
                AggregateKind, BorrowKind, Local, Operand, Place, Rvalue, SourceInfo, Statement,
                StatementKind,
            },
            ty::{Ty, TyCtxt},
        };
        use rustc_span::DUMMY_SP;

        use super::operand;

        pub fn ref_of<'tcx>(
            destination: Place<'tcx>,
            target: Place<'tcx>,
            tcx: TyCtxt<'tcx>,
        ) -> Statement<'tcx> {
            rvalue(
                destination,
                Rvalue::Ref(tcx.lifetimes.re_erased, BorrowKind::Shared, target),
            )
        }

        pub fn array_of_locals_by_move<'tcx>(
            destination: Place<'tcx>,
            ty: Ty<'tcx>,
            items: &[Local],
        ) -> Statement<'tcx> {
            array(
                destination,
                ty,
                Vec::from_iter(items.iter().map(|l| operand::move_for_local(l.clone()))),
            )
        }

        pub fn array<'tcx>(
            destination: Place<'tcx>,
            ty: Ty<'tcx>,
            items: Vec<Operand<'tcx>>,
        ) -> Statement<'tcx> {
            rvalue(
                destination,
                Rvalue::Aggregate(Box::new(AggregateKind::Array(ty)), items),
            )
        }

        pub fn rvalue<'tcx>(destination: Place<'tcx>, value: Rvalue<'tcx>) -> Statement<'tcx> {
            Statement {
                source_info: SourceInfo::outermost(DUMMY_SP),
                kind: StatementKind::Assign(Box::new((destination, value))),
            }
        }
    }

    pub fn convert_mir_binop_to_pri(op: &mir::BinOp) -> pri::BinaryOp {
        match op {
            mir::BinOp::Add => pri::BinaryOp::Add,
            mir::BinOp::Sub => pri::BinaryOp::Sub,
            mir::BinOp::Mul => pri::BinaryOp::Mul,
            mir::BinOp::Div => pri::BinaryOp::Div,
            mir::BinOp::Rem => pri::BinaryOp::Rem,
            mir::BinOp::BitXor => pri::BinaryOp::BitXor,
            mir::BinOp::BitAnd => pri::BinaryOp::BitAnd,
            mir::BinOp::BitOr => pri::BinaryOp::BitOr,
            mir::BinOp::Shl => pri::BinaryOp::Shl,
            mir::BinOp::Shr => pri::BinaryOp::Shr,
            mir::BinOp::Eq => pri::BinaryOp::Eq,
            mir::BinOp::Lt => pri::BinaryOp::Lt,
            mir::BinOp::Le => pri::BinaryOp::Le,
            mir::BinOp::Ne => pri::BinaryOp::Ne,
            mir::BinOp::Ge => pri::BinaryOp::Ge,
            mir::BinOp::Gt => pri::BinaryOp::Gt,
            mir::BinOp::Offset => pri::BinaryOp::Offset,
        }
    }

    pub fn convert_mir_unop_to_pri(op: &mir::UnOp) -> pri::UnaryOp {
        match op {
            mir::UnOp::Not => pri::UnaryOp::Not,
            mir::UnOp::Neg => pri::UnaryOp::Neg,
        }
    }
}