// Converting flat calls to a fluent chain of calls.

use common::pri::{
    AssertionInfo, AssignmentId, BasicBlockIndex, CalleeDef, FieldIndex, FuncDef, LocalIndex,
    OperandRef, PlaceRef, ProgramRuntimeInterface, RawAddress, Ref, SwitchInfo, TypeId, TypeSize,
    VariantIndex,
};
use common::{log_debug, log_info};
use leaf_macros::trait_log_fn;

use crate::abs::{
    self, AssertKind, CastKind, Constant, FloatType, IntType, Local, PlaceUsage, SymVariable,
    ValueType,
    backend::{
        AnnotationHandler, ArgsTupling, AssignmentHandler, CallHandler, ConstraintHandler,
        MemoryHandler, OperandHandler, PlaceBuilder, PlaceHandler, PlaceMetadataHandler,
        PlaceProjector, RawMemoryHandler, RuntimeBackend, Shutdown, SwitchHandler,
    },
};

use super::refs::RefManager;

/// Manages the instance(s) of the runtime backend and provide access to them.
pub(crate) trait InstanceManager {
    type Backend: RuntimeBackend;
    type PlaceBuilder: PlaceBuilder<
            Place = <Self::Backend as RuntimeBackend>::PlaceInfo,
            Index = <Self::Backend as RuntimeBackend>::Place,
        > + Default;
    type PlaceRefManager: RefManager<Ref = PlaceRef, Value = <Self::Backend as RuntimeBackend>::PlaceInfo>;
    type OperandRefManager: RefManager<Ref = OperandRef, Value = <Self::Backend as RuntimeBackend>::Operand>;

    fn init();

    fn perform_on_backend<T>(action: impl for<'a> FnOnce(&'a mut Self::Backend) -> T) -> T;

    fn perform_on_place_ref_manager<T>(action: impl FnOnce(&mut Self::PlaceRefManager) -> T) -> T;

    fn perform_on_operand_ref_manager<T>(
        action: impl FnOnce(&mut Self::OperandRefManager) -> T,
    ) -> T;
}

pub struct FluentPri<IM>(core::marker::PhantomData<IM>);

const TAG: &str = "pri";

#[trait_log_fn(target = "pri", level = "debug")]
impl<IM: InstanceManager> ProgramRuntimeInterface for FluentPri<IM>
where
    <IM::Backend as RuntimeBackend>::Operand: Clone,
{
    type U128 = u128;
    type Char = char;
    type ConstStr = &'static str;
    type ConstByteStr = &'static [u8];
    type Slice<'a, T: 'a> = &'a [T];
    type TypeId = abs::TypeId;
    type BinaryOp = abs::BinaryOp;
    type UnaryOp = abs::UnaryOp;
    type AtomicOrdering = abs::AtomicOrdering;
    type AtomicBinaryOp = abs::AtomicBinaryOp;
    type DebugInfo = common::pri::DebugInfo;
    type Tag = common::pri::Tag;

    fn init_runtime_lib() {
        IM::init();
    }

    fn shutdown_runtime_lib() {
        IM::perform_on_backend(|b| b.shutdown());
    }

    #[tracing::instrument(target = "pri", skip_all, level = "trace")]
    fn debug_info(info: Self::DebugInfo) {
        let str_rep = String::from_utf8_lossy(info);
        let str_rep = str_rep.trim_matches('"');
        const MAX_LEN: usize = 120;
        const DB_TAG: &str = const_format::concatcp!(TAG, "::debug");
        if str_rep.len() <= MAX_LEN {
            log_info!(target: DB_TAG, "{}", str_rep);
        } else {
            log_info!(target: DB_TAG, "{}…", &str_rep[..MAX_LEN]);
            log_debug!(target: DB_TAG, "Full debug info: {}", str_rep);
        }
    }

    fn push_tag(tag: Self::Tag) {
        Self::annotate(|h| h.push_tag(tag))
    }

    fn pop_tag() {
        Self::annotate(|h| h.pop_tag())
    }

    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_return_value() -> PlaceRef {
        Self::push_place_info(|p| p.of_local(Local::ReturnValue))
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_argument(local_index: LocalIndex) -> PlaceRef {
        Self::push_place_info(|p| p.of_local(Local::Argument(local_index)))
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_local(local_index: LocalIndex) -> PlaceRef {
        Self::push_place_info(|p| p.of_local(Local::Normal(local_index)))
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_deref(place: PlaceRef) {
        Self::mut_place_info(place, |p, place| p.project_on(place).deref());
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_field(place: PlaceRef, field: FieldIndex /*, type */) {
        Self::mut_place_info(place, |p, place| p.project_on(place).for_field(field));
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_index(place: PlaceRef, index_place: PlaceRef) {
        let index = Self::take_place_info_to_read(index_place);
        Self::mut_place_info(place, |p, place| p.project_on(place).at_index(index));
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_constant_index(place: PlaceRef, offset: u64, min_length: u64, from_end: bool) {
        Self::mut_place_info(place, |p, place| {
            p.project_on(place)
                .at_constant_index(offset, min_length, from_end)
        });
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_subslice(place: PlaceRef, from: u64, to: u64, from_end: bool) {
        Self::mut_place_info(place, |p, place| {
            p.project_on(place).subslice(from, to, from_end)
        });
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_downcast(place: PlaceRef, variant_index: u32 /*, type */) {
        Self::mut_place_info(place, |p, place| {
            p.project_on(place).downcast(variant_index)
        });
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_opaque_cast(place: PlaceRef /*, type */) {
        Self::mut_place_info(place, |p, place| p.project_on(place).opaque_cast());
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_unwrap_unsafe_binder(place: PlaceRef /*, type */) {
        Self::mut_place_info(place, |p, place| p.project_on(place).unwrap_unsafe_binder());
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn set_place_address(place: PlaceRef, raw_ptr: RawAddress) {
        Self::mut_place_info(place, |p, place| p.metadata(place).set_address(raw_ptr));
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn set_place_type_id(place: PlaceRef, type_id: Self::TypeId) {
        Self::mut_place_info(place, |h, p| h.metadata(p).set_type_id(type_id))
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn set_place_type_bool(place: PlaceRef) {
        Self::set_place_type(place, ValueType::Bool)
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn set_place_type_char(place: PlaceRef) {
        Self::set_place_type(place, ValueType::Char)
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn set_place_type_int(place: PlaceRef, bit_size: u64, is_signed: bool) {
        Self::set_place_type(place, ValueType::new_int(bit_size, is_signed))
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn set_place_type_float(place: PlaceRef, e_bits: u64, s_bits: u64) {
        Self::set_place_type(place, ValueType::new_float(e_bits, s_bits))
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn set_place_size(place: PlaceRef, byte_size: TypeSize) {
        Self::mut_place_info(place, |h, p| h.metadata(p).set_size(byte_size))
    }

    #[tracing::instrument(target = "pri::operand", level = "debug", ret)]
    fn ref_operand_copy(place: PlaceRef) -> OperandRef {
        let place = Self::take_place_info_to_read(place);
        Self::push_operand(|o| o.copy_of(place))
    }
    #[tracing::instrument(target = "pri::operand", level = "debug", ret)]
    fn ref_operand_move(place: PlaceRef) -> OperandRef {
        let place = Self::take_place_info_to_read(place);
        Self::push_operand(|o| o.move_of(place))
    }

    #[tracing::instrument(target = "pri::operand", level = "debug", ret)]
    fn ref_operand_const_bool(value: bool) -> OperandRef {
        Self::push_const_operand(value)
    }
    #[tracing::instrument(target = "pri::operand", level = "debug", ret)]
    fn ref_operand_const_int(bit_rep: u128, bit_size: u64, is_signed: bool) -> OperandRef {
        Self::push_const_operand(Constant::Int {
            bit_rep,
            ty: IntType {
                bit_size,
                is_signed,
            },
        })
    }
    #[tracing::instrument(target = "pri::operand", level = "debug", ret)]
    fn ref_operand_const_float(bit_rep: u128, e_bits: u64, s_bits: u64) -> OperandRef {
        Self::push_const_operand(Constant::Float {
            bit_rep,
            ty: FloatType { e_bits, s_bits },
        })
    }
    #[tracing::instrument(target = "pri::operand", level = "debug", ret)]
    fn ref_operand_const_char(value: char) -> OperandRef {
        Self::push_const_operand(value)
    }
    #[tracing::instrument(target = "pri::operand", level = "debug", ret)]
    fn ref_operand_const_str(value: &'static str) -> OperandRef {
        Self::push_const_operand(value)
    }
    #[tracing::instrument(target = "pri::operand", level = "debug", ret)]
    fn ref_operand_const_byte_str(value: &'static [u8]) -> OperandRef {
        Self::push_const_operand(value)
    }
    #[tracing::instrument(target = "pri::operand", level = "debug", ret)]
    fn ref_operand_const_addr(value: RawAddress) -> OperandRef {
        Self::push_const_operand(value)
    }
    #[tracing::instrument(target = "pri::operand", level = "debug", ret)]
    fn ref_operand_const_zst() -> OperandRef {
        Self::push_const_operand(Constant::Zst)
    }
    #[tracing::instrument(target = "pri::operand", level = "debug", ret)]
    fn ref_operand_const_some() -> OperandRef {
        Self::push_const_operand(Constant::Some)
    }

    fn new_sym_value_bool(conc_val: bool) -> OperandRef {
        // FIXME: Redundant referencing.
        let conc_val = Self::take_back_operand(Self::ref_operand_const_bool(conc_val));
        Self::push_operand(|o| {
            o.new_symbolic(SymVariable {
                ty: ValueType::Bool,
                conc_value: Some(conc_val),
            })
        })
    }
    fn new_sym_value_char(conc_val: char) -> OperandRef {
        // FIXME: Redundant referencing.
        let conc_val = Self::take_back_operand(Self::ref_operand_const_char(conc_val));
        Self::push_operand(|o| {
            o.new_symbolic(SymVariable {
                ty: ValueType::Char,
                conc_value: Some(conc_val),
            })
        })
    }
    fn new_sym_value_int(conc_val_bit_rep: u128, bit_size: u64, is_signed: bool) -> OperandRef {
        // FIXME: Redundant referencing.
        let conc_val = Self::take_back_operand(Self::ref_operand_const_int(
            conc_val_bit_rep,
            bit_size,
            is_signed,
        ));
        Self::push_operand(|o| {
            o.new_symbolic(SymVariable {
                ty: ValueType::new_int(bit_size, is_signed),
                conc_value: Some(conc_val),
            })
        })
    }
    fn new_sym_value_float(conc_val_bit_rep: u128, e_bits: u64, s_bits: u64) -> OperandRef {
        // FIXME: Redundant referencing.
        let conc_val = Self::take_back_operand(Self::ref_operand_const_float(
            conc_val_bit_rep,
            e_bits,
            s_bits,
        ));
        Self::push_operand(|o| {
            o.new_symbolic(SymVariable {
                ty: ValueType::new_float(e_bits, s_bits),
                conc_value: Some(conc_val),
            })
        })
    }

    fn assign_use(id: AssignmentId, dest: PlaceRef, operand: OperandRef) {
        Self::assign_to(id, dest, |h| h.use_of(Self::take_back_operand(operand)))
    }
    fn assign_repeat(id: AssignmentId, dest: PlaceRef, operand: OperandRef, count: usize) {
        Self::assign_to(id, dest, |h| {
            h.repeat_of(Self::take_back_operand(operand), count)
        })
    }
    fn assign_ref(id: AssignmentId, dest: PlaceRef, place: PlaceRef, is_mutable: bool) {
        // FIXME: Mutability does not necessarily mean writing.
        let place = Self::take_place_info_to_ref(place);
        Self::assign_to(id, dest, |h| h.ref_to(place, is_mutable))
    }
    fn assign_thread_local_ref(id: AssignmentId, dest: PlaceRef /* TODO: #365 */) {
        Self::assign_to(id, dest, |h| h.thread_local_ref_to())
    }
    fn assign_raw_ptr_of(id: AssignmentId, dest: PlaceRef, place: PlaceRef, is_mutable: bool) {
        let place = Self::take_place_info_to_ref(place);
        Self::assign_to(id, dest, |h| h.address_of(place, is_mutable))
    }

    fn assign_cast_char(id: AssignmentId, dest: PlaceRef, operand: OperandRef) {
        Self::assign_to(id, dest, |h| {
            h.cast_of(Self::take_back_operand(operand), CastKind::ToChar)
        })
    }
    fn assign_cast_integer(
        id: AssignmentId,
        dest: PlaceRef,
        operand: OperandRef,
        bit_size: u64,
        is_signed: bool,
    ) {
        Self::assign_to(id, dest, |h| {
            h.cast_of(
                Self::take_back_operand(operand),
                CastKind::ToInt(IntType {
                    bit_size,
                    is_signed,
                }),
            )
        })
    }
    fn assign_cast_float(
        id: AssignmentId,
        dest: PlaceRef,
        operand: OperandRef,
        e_bits: u64,
        s_bits: u64,
    ) {
        Self::assign_to(id, dest, |h| {
            h.cast_of(
                Self::take_back_operand(operand),
                CastKind::ToFloat(FloatType { e_bits, s_bits }),
            )
        })
    }
    fn assign_cast_expose_prov(id: AssignmentId, dest: PlaceRef, operand: OperandRef) {
        Self::assign_to(id, dest, |h| {
            h.cast_of(Self::take_back_operand(operand), CastKind::ExposeProvenance)
        })
    }
    fn assign_cast_with_exposed_prov(
        id: AssignmentId,
        dest: PlaceRef,
        operand: OperandRef,
        dst_type_id: TypeId,
    ) {
        Self::assign_cast_pointer(id, dest, operand, dst_type_id);
    }
    fn assign_cast_to_another_ptr(
        id: AssignmentId,
        dest: PlaceRef,
        operand: OperandRef,
        dst_type_id: TypeId,
    ) {
        Self::assign_cast_pointer(id, dest, operand, dst_type_id);
    }

    fn assign_cast_unsize(id: AssignmentId, dest: PlaceRef, operand: OperandRef) {
        Self::assign_to(id, dest, |h| {
            h.cast_of(Self::take_back_operand(operand), CastKind::PointerUnsize)
        })
    }
    fn assign_cast_transmute(
        id: AssignmentId,
        dest: PlaceRef,
        operand: OperandRef,
        dst_type_id: Self::TypeId,
    ) {
        Self::assign_to(id, dest, |h| {
            h.cast_of(
                Self::take_back_operand(operand),
                CastKind::Transmute(dst_type_id),
            )
        })
    }
    fn assign_cast_subtype(
        id: AssignmentId,
        dest: PlaceRef,
        operand: OperandRef,
        dst_type_id: Self::TypeId,
    ) {
        Self::assign_to(id, dest, |h| {
            h.cast_of(
                Self::take_back_operand(operand),
                CastKind::Subtype(dst_type_id),
            )
        })
    }

    fn assign_binary_op(
        id: AssignmentId,
        dest: PlaceRef,
        operator: Self::BinaryOp,
        first: OperandRef,
        second: OperandRef,
    ) {
        Self::assign_to(id, dest, |h| {
            h.binary_op_between(
                operator,
                Self::take_back_operand(first),
                Self::take_back_operand(second),
            )
        })
    }
    fn assign_unary_op(
        id: AssignmentId,
        dest: PlaceRef,
        operator: Self::UnaryOp,
        operand: OperandRef,
    ) {
        Self::assign_to(id, dest, |h| {
            h.unary_op_on(operator, Self::take_back_operand(operand))
        })
    }

    fn set_discriminant(id: AssignmentId, dest: PlaceRef, variant_index: u32) {
        Self::assign_to(id, dest, |h| h.variant_index(variant_index))
    }
    fn assign_discriminant(id: AssignmentId, dest: PlaceRef, place: PlaceRef) {
        let place_info = Self::take_back_place_info(place);
        let place = Self::get_backend_place(abs::PlaceUsage::Read, |h| h.tag_of(place_info));
        Self::assign_to(id, dest, |h| h.discriminant_from(place))
    }

    // We use slice to simplify working with the interface.
    fn assign_aggregate_array(id: AssignmentId, dest: PlaceRef, items: &[OperandRef]) {
        Self::assign_to(id, dest, |h| {
            h.array_from(items.iter().map(|o| Self::take_back_operand(*o)))
        })
    }
    fn assign_aggregate_tuple(id: AssignmentId, dest: PlaceRef, fields: &[OperandRef]) {
        Self::assign_to(id, dest, |h| {
            let fields = Self::take_fields(fields);
            h.tuple_from(fields.into_iter())
        })
    }
    fn assign_aggregate_struct(id: AssignmentId, dest: PlaceRef, fields: &[OperandRef]) {
        Self::assign_to(id, dest, |h| {
            let fields = Self::take_fields(fields);
            h.adt_from(fields.into_iter(), None)
        })
    }
    fn assign_aggregate_enum(
        id: AssignmentId,
        dest: PlaceRef,
        fields: &[OperandRef],
        variant: VariantIndex,
    ) {
        Self::assign_to(id, dest, |h| {
            let fields = Self::take_fields(fields);
            h.adt_from(fields.into_iter(), Some(variant))
        })
    }
    fn assign_aggregate_union(
        id: AssignmentId,
        dest: PlaceRef,
        active_field: FieldIndex,
        value: OperandRef,
    ) {
        Self::assign_to(id, dest, |h| {
            let field = Self::take_fields(&[value]).pop().unwrap();
            h.union_from(active_field, field)
        })
    }
    fn assign_aggregate_closure(id: AssignmentId, dest: PlaceRef, upvars: &[OperandRef]) {
        let upvars = Self::take_fields(upvars);
        Self::assign_to(id, dest, |h| h.closure_from(upvars.into_iter()))
    }
    fn assign_aggregate_coroutine(id: AssignmentId, dest: PlaceRef, upvars: &[OperandRef]) {
        Self::assign_to(id, dest, |h| {
            let upvars = Self::take_fields(upvars);
            h.coroutine_from(upvars.into_iter())
        })
    }
    fn assign_aggregate_coroutine_closure(id: AssignmentId, dest: PlaceRef, upvars: &[OperandRef]) {
        Self::assign_to(id, dest, |h| {
            let upvars = Self::take_fields(upvars);
            h.coroutine_closure_from(upvars.into_iter())
        })
    }
    fn assign_aggregate_raw_ptr(
        id: AssignmentId,
        dest: PlaceRef,
        data_ptr: OperandRef,
        metadata: OperandRef,
        is_mutable: bool,
    ) {
        Self::assign_to(id, dest, |h| {
            h.raw_ptr_from(
                Self::take_back_operand(data_ptr),
                Self::take_back_operand(metadata),
                is_mutable,
            )
        })
    }

    fn assign_shallow_init_box(
        id: AssignmentId,
        dest: PlaceRef,
        operand: OperandRef,
        _boxed_type_id: Self::TypeId,
    ) {
        Self::assign_to(id, dest, |h| {
            h.shallow_init_box_from(Self::take_back_operand(operand))
        })
    }

    fn assign_wrap_unsafe_binder(
        id: AssignmentId,
        dest: PlaceRef,
        operand: OperandRef,
        _binder_type_id: Self::TypeId,
    ) {
        Self::assign_to(id, dest, |h| {
            h.shallow_init_box_from(Self::take_back_operand(operand))
        })
    }

    fn mark_storage_dead(place: PlaceRef) {
        let place_info = Self::take_back_place_info(place);
        let place = Self::get_backend_place(abs::PlaceUsage::Write, |h| h.from_info(place_info));
        Self::memory(|h| h.mark_dead(place));
    }

    fn take_branch_false(info: SwitchInfo) {
        // Self::switch(info, |h| h.take(false.into()))
        Self::constraint_at(info.node_location, |c| {
            let handler = c.switch(Self::take_back_operand(info.discriminant));
            handler.take(false.into())
        })
    }
    fn take_branch_ow_bool(info: SwitchInfo) {
        // Self::switch(info, |h| h.take_otherwise(vec![false.into()]))
        Self::constraint_at(info.node_location, |c| {
            let handler = c.switch(Self::take_back_operand(info.discriminant));
            handler.take_otherwise(vec![false.into()])
        })
    }

    fn take_branch_int(info: SwitchInfo, value_bit_rep: u128, bit_size: u64, is_signed: bool) {
        // Self::switch(info, |h| {
        //     h.take(Constant::Int {
        //         bit_rep: value_bit_rep,
        //         ty: IntType {
        //             bit_size,
        //             is_signed,
        //         },
        //     })
        // })
        Self::constraint_at(info.node_location, |c| {
            let handler = c.switch(Self::take_back_operand(info.discriminant));
            handler.take(Constant::Int {
                bit_rep: value_bit_rep,
                ty: IntType {
                    bit_size,
                    is_signed,
                },
            })
        })
    }
    fn take_branch_ow_int(info: SwitchInfo, non_values: &[u128], bit_size: u64, is_signed: bool) {
        // Self::switch(info, |h| {
        //     h.take_otherwise(
        //         non_values
        //             .iter()
        //             .map(|nv| Constant::Int {
        //                 bit_rep: *nv,
        //                 ty: IntType {
        //                     bit_size,
        //                     is_signed,
        //                 },
        //             })
        //             .collect(),
        //     )
        // })
        Self::constraint_at(info.node_location, |c| {
            let handler = c.switch(Self::take_back_operand(info.discriminant));
            handler.take_otherwise(
                non_values
                    .iter()
                    .map(|nv| Constant::Int {
                        bit_rep: *nv,
                        ty: IntType {
                            bit_size,
                            is_signed,
                        },
                    })
                    .collect(),
            )
        })
    }

    fn take_branch_char(info: SwitchInfo, value: char) {
        // Self::switch(info, |h| h.take(value.into()))
        Self::constraint_at(info.node_location, |c| {
            let handler = c.switch(Self::take_back_operand(info.discriminant));
            handler.take(value.into())
        })
    }
    fn take_branch_ow_char(info: SwitchInfo, non_values: &[char]) {
        // Self::switch(info, |h| {
        //     h.take_otherwise(non_values.iter().map(|c| (*c).into()).collect())
        // })
        Self::constraint_at(info.node_location, |c| {
            let handler = c.switch(Self::take_back_operand(info.discriminant));
            handler.take_otherwise(non_values.iter().map(|c| (*c).into()).collect())
        })
    }

    fn assert_bounds_check(info: AssertionInfo, len: OperandRef, index: OperandRef) {
        let assert_kind = AssertKind::BoundsCheck {
            len: Self::take_back_operand(len),
            index: Self::take_back_operand(index),
        };
        Self::assert(info, assert_kind)
    }
    fn assert_overflow(
        info: AssertionInfo,
        operator: Self::BinaryOp,
        first: OperandRef,
        second: OperandRef,
    ) {
        let assert_kind = AssertKind::Overflow(
            operator,
            Self::take_back_operand(first),
            Self::take_back_operand(second),
        );
        Self::assert(info, assert_kind)
    }
    fn assert_overflow_neg(info: AssertionInfo, operand: OperandRef) {
        let assert_kind = AssertKind::OverflowNeg(Self::take_back_operand(operand));
        Self::assert(info, assert_kind)
    }
    fn assert_div_by_zero(info: AssertionInfo, operand: OperandRef) {
        let assert_kind = AssertKind::DivisionByZero(Self::take_back_operand(operand));
        Self::assert(info, assert_kind)
    }
    fn assert_rem_by_zero(info: AssertionInfo, operand: OperandRef) {
        let assert_kind = AssertKind::RemainderByZero(Self::take_back_operand(operand));
        Self::assert(info, assert_kind)
    }
    fn assert_misaligned_ptr_deref(info: AssertionInfo, required: OperandRef, found: OperandRef) {
        let assert_kind = AssertKind::MisalignedPointerDereference {
            required: Self::take_back_operand(required),
            found: Self::take_back_operand(found),
        };
        Self::assert(info, assert_kind)
    }
    fn assert_null_ptr_deref(info: AssertionInfo) {
        let assert_kind = AssertKind::NullPointerDereference;
        Self::assert(info, assert_kind)
    }
    fn assert_invalid_enum_ctn(info: AssertionInfo, discr: OperandRef) {
        let assert_kind = AssertKind::InvalidEnumConstruction(Self::take_back_operand(discr));
        Self::assert(info, assert_kind)
    }

    #[tracing::instrument(target = "pri::call", level = "debug")]
    fn before_call_func(
        def: CalleeDef,
        call_site: BasicBlockIndex,
        func: OperandRef,
        args: &[OperandRef],
        are_args_tupled: bool,
    ) {
        Self::func_control(|h| {
            h.before_call(
                def.into(),
                call_site,
                Self::take_back_operand(func),
                args.iter().map(|o| Self::take_back_operand(*o).into()),
                are_args_tupled,
            )
        });
    }
    #[tracing::instrument(target = "pri::call", level = "debug")]
    fn enter_func(def: FuncDef, arg_places: &[PlaceRef], ret_val_place: PlaceRef) {
        Self::enter_func_with_tupling(def, arg_places, ret_val_place, ArgsTupling::Normal);
    }
    #[tracing::instrument(target = "pri::call", level = "debug")]
    fn enter_func_untupled_args(
        def: FuncDef,
        arg_places: &[PlaceRef],
        ret_val_place: PlaceRef,
        tupled_arg_index: LocalIndex,
        tupled_arg_type_id: TypeId,
    ) {
        Self::enter_func_with_tupling(
            def,
            arg_places,
            ret_val_place,
            ArgsTupling::Untupled {
                tupled_arg_index: Local::Argument(tupled_arg_index),
                tuple_type: tupled_arg_type_id,
            },
        )
    }
    fn enter_func_tupled_args(def: FuncDef, arg_places: &[PlaceRef], ret_val_place: PlaceRef) {
        Self::enter_func_with_tupling(def, arg_places, ret_val_place, ArgsTupling::Tupled)
    }
    #[tracing::instrument(target = "pri::call", level = "debug")]
    fn return_from_func(ret_point: BasicBlockIndex) {
        Self::func_control(|h| h.ret(ret_point))
    }
    /// Overrides (forces) the return value of a function.
    /// In an external call chain, the value will be kept as the return value
    /// until it is consumed at the point of return to an internal caller.
    #[tracing::instrument(target = "pri::call", level = "debug")]
    fn override_return_value(operand: OperandRef) {
        Self::func_control(|h| h.override_return_value(Self::take_back_operand(operand)))
    }
    #[tracing::instrument(target = "pri::call", level = "debug")]
    fn after_call_func(id: AssignmentId, dest: PlaceRef) {
        let dest_place = Self::take_place_info_to_write(dest);
        Self::func_control(|h| h.after_call(id, dest_place))
    }

    fn intrinsic_assign_identity(id: AssignmentId, dest: PlaceRef, x: OperandRef) {
        Self::assign_use(id, dest, x)
    }

    fn intrinsic_assign_rotate_left(
        id: AssignmentId,
        dest: PlaceRef,
        x: OperandRef,
        shift: OperandRef,
    ) {
        Self::assign_binary_op(id, dest, Self::BinaryOp::RotateL, x, shift)
    }

    fn intrinsic_assign_rotate_right(
        id: AssignmentId,
        dest: PlaceRef,
        x: OperandRef,
        shift: OperandRef,
    ) {
        Self::assign_binary_op(id, dest, Self::BinaryOp::RotateR, x, shift)
    }

    fn intrinsic_assign_saturating_add(
        id: AssignmentId,
        dest: PlaceRef,
        first: OperandRef,
        second: OperandRef,
    ) {
        Self::assign_binary_op(id, dest, Self::BinaryOp::AddSaturating, first, second)
    }

    fn intrinsic_assign_saturating_sub(
        id: AssignmentId,
        dest: PlaceRef,
        first: OperandRef,
        second: OperandRef,
    ) {
        Self::assign_binary_op(id, dest, Self::BinaryOp::SubSaturating, first, second)
    }

    fn intrinsic_assign_disjoint_bitor(
        id: AssignmentId,
        dest: PlaceRef,
        first: OperandRef,
        second: OperandRef,
    ) {
        // Currently no distinction, but may be an interesting case for unsafe checks.
        Self::assign_binary_op(id, dest, Self::BinaryOp::BitOr, first, second);
    }

    fn intrinsic_assign_exact_div(
        id: AssignmentId,
        dest: PlaceRef,
        first: OperandRef,
        second: OperandRef,
    ) {
        Self::assign_binary_op(id, dest, Self::BinaryOp::DivExact, first, second);
    }

    fn intrinsic_assign_bitreverse(id: AssignmentId, dest: PlaceRef, x: OperandRef) {
        Self::assign_unary_op(id, dest, Self::UnaryOp::BitReverse, x);
    }

    fn intrinsic_assign_cttz_nonzero(id: AssignmentId, dest: PlaceRef, x: OperandRef) {
        Self::assign_unary_op(id, dest, Self::UnaryOp::NonZeroTrailingZeros, x);
    }

    fn intrinsic_assign_cttz(id: AssignmentId, dest: PlaceRef, x: OperandRef) {
        Self::assign_unary_op(id, dest, Self::UnaryOp::TrailingZeros, x);
    }

    fn intrinsic_assign_ctlz_nonzero(id: AssignmentId, dest: PlaceRef, x: OperandRef) {
        Self::assign_unary_op(id, dest, Self::UnaryOp::NonZeroLeadingZeros, x);
    }

    fn intrinsic_assign_ctlz(id: AssignmentId, dest: PlaceRef, x: OperandRef) {
        Self::assign_unary_op(id, dest, Self::UnaryOp::LeadingZeros, x);
    }

    fn intrinsic_assign_ctpop(id: AssignmentId, dest: PlaceRef, x: OperandRef) {
        Self::assign_unary_op(id, dest, Self::UnaryOp::CountOnes, x);
    }

    fn intrinsic_assign_bswap(id: AssignmentId, dest: PlaceRef, x: OperandRef) {
        Self::assign_unary_op(id, dest, Self::UnaryOp::ByteSwap, x);
    }

    fn intrinsic_atomic_binary_op(
        _ordering: Self::AtomicOrdering,
        id: AssignmentId,
        ptr: OperandRef,
        conc_ptr: RawAddress,
        ptr_type_id: Self::TypeId,
        operator: Self::AtomicBinaryOp,
        src: OperandRef,
        prev_dest: PlaceRef,
    ) {
        // Perform sequentially.
        let binary_op = match operator {
            abs::AtomicBinaryOp::Add => Self::BinaryOp::Add,
            abs::AtomicBinaryOp::Sub => Self::BinaryOp::Sub,
            abs::AtomicBinaryOp::Xor => Self::BinaryOp::BitXor,
            abs::AtomicBinaryOp::And => Self::BinaryOp::BitAnd,
            abs::AtomicBinaryOp::Nand => todo!(),
            abs::AtomicBinaryOp::Or => Self::BinaryOp::BitOr,
            abs::AtomicBinaryOp::Min => todo!(),
            abs::AtomicBinaryOp::Max => todo!(),
        };

        Self::update_by_ptr_return_old(
            id,
            ptr,
            conc_ptr,
            ptr_type_id,
            src,
            prev_dest,
            |h, current, src| h.binary_op_between(binary_op, current, src),
        );
    }

    fn intrinsic_atomic_fence(_ordering: Self::AtomicOrdering, _single_thread: bool) {
        // No-op.
    }

    fn intrinsic_memory_load(
        id: AssignmentId,
        ptr: OperandRef,
        conc_ptr: RawAddress,
        ptr_type_id: Self::TypeId,
        dest: PlaceRef,
        _is_volatile: bool,
        _is_aligned: bool,
    ) {
        Self::load(id, ptr, conc_ptr, ptr_type_id, dest)
    }

    fn intrinsic_memory_store(
        id: AssignmentId,
        ptr: OperandRef,
        conc_ptr: RawAddress,
        ptr_type_id: Self::TypeId,
        value: OperandRef,
        _is_volatile: bool,
        _is_aligned: bool,
    ) {
        Self::store(id, ptr, conc_ptr, ptr_type_id, value)
    }

    fn intrinsic_memory_copy(
        id: AssignmentId,
        src_ptr: OperandRef,
        conc_src_ptr: RawAddress,
        ptr_type_id: Self::TypeId,
        dst_ptr: OperandRef,
        conc_dst_ptr: RawAddress,
        count: OperandRef,
        conc_count: usize,
        _is_volatile: bool,
        _is_overlapping: bool,
    ) {
        let src_ptr = Self::take_back_operand(src_ptr);
        let dst_ptr = Self::take_back_operand(dst_ptr);
        let count = Self::take_back_operand(count);
        Self::raw_memory(|h| {
            h.copy(
                id,
                src_ptr,
                conc_src_ptr,
                dst_ptr,
                conc_dst_ptr,
                count,
                conc_count,
                ptr_type_id,
            )
        })
    }

    #[allow(unused_parens)]
    fn intrinsic_memory_set(
        id: AssignmentId,
        ptr: OperandRef,
        conc_ptr: RawAddress,
        ptr_type_id: Self::TypeId,
        val: OperandRef,
        count: OperandRef,
        conc_count: usize,
        _is_volatile: bool,
    ) {
        let ptr = Self::take_back_operand(ptr);
        let val = Self::take_back_operand(val);
        let count = Self::take_back_operand(count);
        Self::raw_memory(|h| h.set(id, ptr, conc_ptr, val, count, conc_count, ptr_type_id));
    }

    fn intrinsic_memory_swap(
        id: AssignmentId,
        first_ptr: OperandRef,
        conc_first_ptr: RawAddress,
        ptr_type_id: Self::TypeId,
        second_ptr: OperandRef,
        conc_second_ptr: RawAddress,
    ) {
        let first_ptr = Self::take_back_operand(first_ptr);
        let second_ptr = Self::take_back_operand(second_ptr);
        Self::raw_memory(|h| {
            h.swap(
                id,
                first_ptr,
                conc_first_ptr,
                second_ptr,
                conc_second_ptr,
                ptr_type_id,
            )
        });
    }

    fn intrinsic_atomic_load(
        _ordering: Self::AtomicOrdering,
        id: AssignmentId,
        ptr: OperandRef,
        conc_ptr: RawAddress,
        ptr_type_id: Self::TypeId,
        dest: PlaceRef,
    ) {
        Self::load(id, ptr, conc_ptr, ptr_type_id, dest)
    }

    fn intrinsic_atomic_store(
        _ordering: Self::AtomicOrdering,
        id: AssignmentId,
        ptr: OperandRef,
        conc_ptr: RawAddress,
        ptr_type_id: Self::TypeId,
        src: OperandRef,
    ) {
        Self::store(id, ptr, conc_ptr, ptr_type_id, src)
    }

    fn intrinsic_atomic_xchg(
        _ordering: Self::AtomicOrdering,
        id: AssignmentId,
        ptr: OperandRef,
        conc_ptr: RawAddress,
        ptr_type_id: Self::TypeId,
        val: OperandRef,
        prev_dest: PlaceRef,
    ) {
        Self::update_by_ptr_return_old(
            id,
            ptr,
            conc_ptr,
            ptr_type_id,
            val,
            prev_dest,
            |h, _current, val| h.use_of(val),
        )
    }

    fn intrinsic_atomic_cxchg(
        _ordering: Self::AtomicOrdering,
        id: AssignmentId,
        ptr: OperandRef,
        conc_ptr: RawAddress,
        ptr_type_id: Self::TypeId,
        failure_ordering: Self::AtomicOrdering,
        _weak: bool,
        old: OperandRef,
        src: OperandRef,
        prev_dest: PlaceRef,
    ) {
        let old = Self::take_back_operand(old);

        Self::update_by_ptr(
            id,
            ptr,
            conc_ptr,
            ptr_type_id,
            src,
            prev_dest,
            |h, current, src| h.use_if_eq(src, current, old.clone()),
            |h, current| h.use_and_check_eq(current, old.clone()),
        )
    }
}

impl<IM: InstanceManager> FluentPri<IM>
where
    <IM::Backend as RuntimeBackend>::Operand: Clone,
{
    #[inline]
    fn push_const_operand<T: Into<Constant>>(constant: T) -> OperandRef {
        Self::push_operand(|o| o.const_from(constant.into()))
    }

    fn set_place_type(place: PlaceRef, ty: ValueType) {
        Self::mut_place_info(place, |p, place| p.metadata(place).set_primitive_type(ty));
    }

    fn take_fields(fields: &[OperandRef]) -> Vec<<IM::Backend as RuntimeBackend>::Operand> {
        fields.iter().map(|o| Self::take_back_operand(*o)).collect()
    }

    fn assign_cast_pointer(
        id: AssignmentId,
        dest: PlaceRef,
        operand: OperandRef,
        dst_type_id: TypeId,
    ) {
        Self::assign_to(id, dest, |h| {
            h.cast_of(
                Self::take_back_operand(operand),
                CastKind::ToPointer(dst_type_id),
            )
        })
    }

    fn assert(
        info: AssertionInfo,
        assert_kind: AssertKind<<IM::Backend as RuntimeBackend>::Operand>,
    ) {
        Self::constraint_at(info.location, |h| {
            h.assert(
                Self::take_back_operand(info.condition),
                info.expected,
                assert_kind,
            )
        })
    }

    fn load(
        id: AssignmentId,
        ptr: OperandRef,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        dest: PlaceRef,
    ) {
        let src_ptr = Self::take_back_operand(ptr);
        let src_place = Self::raw_memory(|h| {
            h.place_from_ptr(src_ptr.clone(), conc_ptr, ptr_type_id, PlaceUsage::Read)
        });
        let src_pointee_value =
            Self::take_back_operand(Self::push_operand(|h| h.copy_of(src_place)));
        Self::assign_to(id, dest, |h| h.use_of(src_pointee_value))
    }

    fn store(
        id: AssignmentId,
        ptr: OperandRef,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        value: OperandRef,
    ) {
        let dst_ptr = Self::take_back_operand(ptr);
        let dst_place = Self::raw_memory(|h| {
            h.place_from_ptr(dst_ptr, conc_ptr, ptr_type_id, PlaceUsage::Write)
        });
        let value = Self::take_back_operand(value);
        Self::assign_to_place(id, dst_place, |h| h.use_of(value))
    }

    fn update_by_ptr_return_old(
        id: AssignmentId,
        ptr: OperandRef,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        src: OperandRef,
        prev_dest: PlaceRef,
        ptr_update_action: impl FnOnce(
            <IM::Backend as RuntimeBackend>::AssignmentHandler<'_>,
            <IM::Backend as RuntimeBackend>::Operand,
            <IM::Backend as RuntimeBackend>::Operand,
        ),
    ) {
        Self::update_by_ptr(
            id,
            ptr,
            conc_ptr,
            ptr_type_id,
            src,
            prev_dest,
            ptr_update_action,
            |h, current| h.use_of(current),
        )
    }

    fn update_by_ptr(
        id: AssignmentId,
        ptr: OperandRef,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        src: OperandRef,
        prev_dest: PlaceRef,
        ptr_update_action: impl FnOnce(
            <IM::Backend as RuntimeBackend>::AssignmentHandler<'_>,
            <IM::Backend as RuntimeBackend>::Operand,
            <IM::Backend as RuntimeBackend>::Operand,
        ),
        dest_assign_action: impl FnOnce(
            <IM::Backend as RuntimeBackend>::AssignmentHandler<'_>,
            <IM::Backend as RuntimeBackend>::Operand,
        ),
    ) {
        let ptr = Self::take_back_operand(ptr);
        let ptr_place = Self::raw_memory(|h| {
            h.place_from_ptr(ptr.clone(), conc_ptr, ptr_type_id, PlaceUsage::Read)
        });
        let current = Self::take_back_operand(Self::push_operand(|h| h.copy_of(ptr_place)));

        let ptr_place =
            Self::raw_memory(|h| h.place_from_ptr(ptr, conc_ptr, ptr_type_id, PlaceUsage::Write));
        let src = Self::take_back_operand(src);
        Self::assign_to_place(id, ptr_place, |h| {
            ptr_update_action(h, current.clone(), src)
        });

        Self::assign_to(id, prev_dest, |h| dest_assign_action(h, current.clone()));
    }

    fn enter_func_with_tupling(
        def: FuncDef,
        arg_places: &[PlaceRef],
        ret_val_place: PlaceRef,
        tupling: ArgsTupling,
    ) {
        let arg_places = arg_places
            .iter()
            .map(|p| Self::take_place_info_to_read(*p))
            .collect::<Vec<_>>();
        let ret_val_place = Self::take_place_info_to_write(ret_val_place);
        Self::func_control(|h| h.enter(def.into(), arg_places, ret_val_place, tupling));
    }

    #[inline]
    fn push_place_info(
        build: impl FnOnce(IM::PlaceBuilder) -> <IM::Backend as RuntimeBackend>::PlaceInfo,
    ) -> PlaceRef {
        let builder = IM::PlaceBuilder::default();
        let place = build(builder);
        IM::perform_on_place_ref_manager(|rm| rm.push(place))
    }
    #[inline]
    fn mut_place_info<T>(
        place_ref: PlaceRef,
        mut_place: impl FnOnce(IM::PlaceBuilder, &mut <IM::Backend as RuntimeBackend>::PlaceInfo) -> T,
    ) -> T {
        IM::perform_on_place_ref_manager(|rm| {
            let place = rm.get_mut(place_ref);
            let builder = IM::PlaceBuilder::default();
            mut_place(builder, place)
        })
    }
    #[inline]
    fn take_back_place_info(reference: PlaceRef) -> <IM::Backend as RuntimeBackend>::PlaceInfo {
        IM::perform_on_place_ref_manager(|rm| rm.take(reference))
    }
    #[inline]
    fn take_place_info_to_read(reference: PlaceRef) -> <IM::Backend as RuntimeBackend>::Place {
        let place_info = Self::take_back_place_info(reference);
        Self::get_backend_place(PlaceUsage::Read, |h| h.from_info(place_info))
    }
    #[inline]
    fn take_place_info_to_write(reference: PlaceRef) -> <IM::Backend as RuntimeBackend>::Place {
        let place_info = Self::take_back_place_info(reference);
        Self::get_backend_place(PlaceUsage::Write, |h| h.from_info(place_info))
    }
    #[inline]
    fn take_place_info_to_ref(reference: PlaceRef) -> <IM::Backend as RuntimeBackend>::Place {
        let place_info = Self::take_back_place_info(reference);
        Self::get_backend_place(PlaceUsage::Ref, |h| h.from_info(place_info))
    }
    #[inline]
    fn get_backend_place<T>(
        usage: PlaceUsage,
        get_place: impl FnOnce(<IM::Backend as RuntimeBackend>::PlaceHandler<'_>) -> T,
    ) -> T {
        IM::perform_on_backend(|r| get_place(r.place(usage)))
    }

    #[inline]
    fn assign_to<T>(
        id: AssignmentId,
        dest_ref: PlaceRef,
        assign_action: impl FnOnce(<IM::Backend as RuntimeBackend>::AssignmentHandler<'_>) -> T,
    ) -> T {
        let dest = Self::take_place_info_to_write(dest_ref);
        Self::assign_to_place(id, dest, assign_action)
    }

    #[inline]
    fn assign_to_place<T>(
        id: AssignmentId,
        dest: <IM::Backend as RuntimeBackend>::Place,
        assign_action: impl FnOnce(<IM::Backend as RuntimeBackend>::AssignmentHandler<'_>) -> T,
    ) -> T {
        IM::perform_on_backend(|r| assign_action(r.assign_to(id, dest)))
    }

    #[inline]
    fn push_operand(
        get_operand: impl FnOnce(
            <IM::Backend as RuntimeBackend>::OperandHandler<'_>,
        ) -> <IM::Backend as RuntimeBackend>::Operand,
    ) -> OperandRef {
        let operand = IM::perform_on_backend(|r| get_operand(r.operand()));
        IM::perform_on_operand_ref_manager(|rm| rm.push(operand))
    }
    #[inline]
    fn take_back_operand(reference: OperandRef) -> <IM::Backend as RuntimeBackend>::Operand {
        IM::perform_on_operand_ref_manager(|rm| rm.take(reference))
    }

    #[inline]
    fn memory<T>(
        memory_action: impl FnOnce(<IM::Backend as RuntimeBackend>::MemoryHandler<'_>) -> T,
    ) -> T {
        IM::perform_on_backend(|r| {
            let handler = r.memory();
            memory_action(handler)
        })
    }

    #[inline]
    fn raw_memory<T>(
        memory_action: impl FnOnce(<IM::Backend as RuntimeBackend>::RawMemoryHandler<'_>) -> T,
    ) -> T {
        IM::perform_on_backend(|r| {
            let handler = r.raw_memory();
            memory_action(handler)
        })
    }

    #[inline]
    fn constraint_at<T>(
        location: BasicBlockIndex,
        constraint_action: impl for<'a> FnOnce(
            <IM::Backend as RuntimeBackend>::ConstraintHandler<'a>,
        ) -> T,
    ) -> T {
        IM::perform_on_backend(|r| {
            let handler = r.constraint_at(location);
            constraint_action(handler)
        })
    }

    #[inline]
    fn switch<T>(
        info: SwitchInfo,
        switch_action: impl for<'a> FnOnce(
        <<IM::Backend as RuntimeBackend>::ConstraintHandler<'a> as ConstraintHandler>::SwitchHandler,
    ) -> T,
    ) -> T {
        Self::constraint_at(info.node_location, |c| {
            let handler = c.switch(Self::take_back_operand(info.discriminant));
            switch_action(handler)
        })
    }

    #[inline]
    fn func_control<T>(
        call_action: impl FnOnce(<IM::Backend as RuntimeBackend>::CallHandler<'_>) -> T,
    ) -> T {
        IM::perform_on_backend(|r| {
            let call_control = r.call_control();
            call_action(call_control)
        })
    }

    #[inline]
    fn annotate<T>(
        annotate_action: impl FnOnce(<IM::Backend as RuntimeBackend>::AnnotationHandler<'_>) -> T,
    ) -> T {
        IM::perform_on_backend(|r| {
            let annotate = r.annotate();
            annotate_action(annotate)
        })
    }
}
