mod ffi;
mod instance;
mod utils;

use common::{log_debug, log_info, pri::*};

use self::instance::*;
use crate::abs::{
    self, AssertKind, CastKind, Constant, FloatType, IntType, Local, PlaceUsage, SymVariable,
    ValueType, backend::*,
};

use leaf_macros::trait_log_fn;

pub struct BasicPri;

const TAG: &str = "pri";

#[trait_log_fn(target = "pri", level = "debug")]
impl ProgramRuntimeInterface for BasicPri {
    type U128 = u128;
    type Char = char;
    type ConstStr = &'static str;
    type ConstByteStr = &'static [u8];
    type Slice<'a, T: 'a> = &'a [T];
    type TypeId = TypeId;
    type BinaryOp = abs::BinaryOp;
    type UnaryOp = abs::UnaryOp;
    type AtomicOrdering = abs::AtomicOrdering;
    type AtomicBinaryOp = abs::AtomicBinaryOp;
    type DebugInfo = DebugInfo;
    type Tag = Tag;

    fn init_runtime_lib() {
        init_backend();
    }

    fn shutdown_runtime_lib() {
        shutdown_backend();
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
        annotate(|h| h.push_tag(tag))
    }

    fn pop_tag() {
        annotate(|h| h.pop_tag())
    }

    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_return_value() -> PlaceRef {
        push_place_info(|p| p.of_local(Local::ReturnValue))
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_argument(local_index: LocalIndex) -> PlaceRef {
        push_place_info(|p| p.of_local(Local::Argument(local_index)))
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_local(local_index: LocalIndex) -> PlaceRef {
        push_place_info(|p| p.of_local(Local::Normal(local_index)))
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_deref(place: PlaceRef) {
        mut_place_info(place, |p, place| p.project_on(place).deref())
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_field(place: PlaceRef, field: FieldIndex /*, type */) {
        mut_place_info(place, |p, place| p.project_on(place).for_field(field))
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_index(place: PlaceRef, index_place: PlaceRef) {
        let index = take_place_info_to_read(index_place);
        mut_place_info(place, |p, place| p.project_on(place).at_index(index))
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_constant_index(place: PlaceRef, offset: u64, min_length: u64, from_end: bool) {
        mut_place_info(place, |p, place| {
            p.project_on(place)
                .at_constant_index(offset, min_length, from_end)
        })
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_subslice(place: PlaceRef, from: u64, to: u64, from_end: bool) {
        mut_place_info(place, |p, place| {
            p.project_on(place).subslice(from, to, from_end)
        })
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_downcast(place: PlaceRef, variant_index: u32 /*, type */) {
        mut_place_info(place, |p, place| {
            p.project_on(place).downcast(variant_index)
        })
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_opaque_cast(place: PlaceRef /*, type */) {
        mut_place_info(place, |p, place| p.project_on(place).opaque_cast())
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn ref_place_unwrap_unsafe_binder(place: PlaceRef /*, type */) {
        mut_place_info(place, |p, place| p.project_on(place).unwrap_unsafe_binder())
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn set_place_address(place: PlaceRef, raw_ptr: RawAddress) {
        mut_place_info(place, |p, place| p.metadata(place).set_address(raw_ptr));
    }
    #[tracing::instrument(target = "pri::place", level = "debug", ret)]
    fn set_place_type_id(place: PlaceRef, type_id: Self::TypeId) {
        mut_place_info(place, |h, p| h.metadata(p).set_type_id(type_id))
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
        mut_place_info(place, |h, p| h.metadata(p).set_size(byte_size))
    }

    #[tracing::instrument(target = "pri::operand", level = "debug", ret)]
    fn ref_operand_copy(place: PlaceRef) -> OperandRef {
        let place = take_place_info_to_read(place);
        push_operand(|o| o.copy_of(place))
    }
    #[tracing::instrument(target = "pri::operand", level = "debug", ret)]
    fn ref_operand_move(place: PlaceRef) -> OperandRef {
        let place = take_place_info_to_read(place);
        push_operand(|o| o.move_of(place))
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
        let conc_val = take_back_operand(Self::ref_operand_const_bool(conc_val));
        push_operand(|o| {
            o.new_symbolic(SymVariable {
                ty: ValueType::Bool,
                conc_value: Some(conc_val),
            })
        })
    }
    fn new_sym_value_char(conc_val: char) -> OperandRef {
        // FIXME: Redundant referencing.
        let conc_val = take_back_operand(Self::ref_operand_const_char(conc_val));
        push_operand(|o| {
            o.new_symbolic(SymVariable {
                ty: ValueType::Char,
                conc_value: Some(conc_val),
            })
        })
    }
    fn new_sym_value_int(conc_val_bit_rep: u128, bit_size: u64, is_signed: bool) -> OperandRef {
        // FIXME: Redundant referencing.
        let conc_val = take_back_operand(Self::ref_operand_const_int(
            conc_val_bit_rep,
            bit_size,
            is_signed,
        ));
        push_operand(|o| {
            o.new_symbolic(SymVariable {
                ty: ValueType::new_int(bit_size, is_signed),
                conc_value: Some(conc_val),
            })
        })
    }
    fn new_sym_value_float(conc_val_bit_rep: u128, e_bits: u64, s_bits: u64) -> OperandRef {
        // FIXME: Redundant referencing.
        let conc_val = take_back_operand(Self::ref_operand_const_float(
            conc_val_bit_rep,
            e_bits,
            s_bits,
        ));
        push_operand(|o| {
            o.new_symbolic(SymVariable {
                ty: ValueType::new_float(e_bits, s_bits),
                conc_value: Some(conc_val),
            })
        })
    }

    fn assign_use(id: AssignmentId, dest: PlaceRef, operand: OperandRef) {
        assign_to(id, dest, |h| h.use_of(take_back_operand(operand)))
    }
    fn assign_repeat(id: AssignmentId, dest: PlaceRef, operand: OperandRef, count: usize) {
        assign_to(id, dest, |h| h.repeat_of(take_back_operand(operand), count))
    }
    fn assign_ref(id: AssignmentId, dest: PlaceRef, place: PlaceRef, is_mutable: bool) {
        // FIXME: Mutability does not necessarily mean writing.
        let place = take_place_info_to_ref(place);
        assign_to(id, dest, |h| h.ref_to(place, is_mutable))
    }
    fn assign_thread_local_ref(id: AssignmentId, dest: PlaceRef /* TODO: #365 */) {
        assign_to(id, dest, |h| h.thread_local_ref_to())
    }
    fn assign_raw_ptr_of(id: AssignmentId, dest: PlaceRef, place: PlaceRef, is_mutable: bool) {
        let place = take_place_info_to_ref(place);
        assign_to(id, dest, |h| h.address_of(place, is_mutable))
    }
    fn assign_len(id: AssignmentId, dest: PlaceRef, place: PlaceRef) {
        // To be investigated. Not obvious whether it appears at all in the later stages.
        let place = take_place_info_to_ref(place);
        assign_to(id, dest, |h| h.len_of(place))
    }

    fn assign_cast_char(id: AssignmentId, dest: PlaceRef, operand: OperandRef) {
        assign_to(id, dest, |h| {
            h.cast_of(take_back_operand(operand), CastKind::ToChar)
        })
    }
    fn assign_cast_integer(
        id: AssignmentId,
        dest: PlaceRef,
        operand: OperandRef,
        bit_size: u64,
        is_signed: bool,
    ) {
        assign_to(id, dest, |h| {
            h.cast_of(
                take_back_operand(operand),
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
        assign_to(id, dest, |h| {
            h.cast_of(
                take_back_operand(operand),
                CastKind::ToFloat(FloatType { e_bits, s_bits }),
            )
        })
    }
    fn assign_cast_expose_prov(id: AssignmentId, dest: PlaceRef, operand: OperandRef) {
        assign_to(id, dest, |h| {
            h.cast_of(take_back_operand(operand), CastKind::ExposeProvenance)
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
        assign_to(id, dest, |h| {
            h.cast_of(take_back_operand(operand), CastKind::PointerUnsize)
        })
    }
    fn assign_cast_transmute(
        id: AssignmentId,
        dest: PlaceRef,
        operand: OperandRef,
        dst_type_id: Self::TypeId,
    ) {
        assign_to(id, dest, |h| {
            h.cast_of(take_back_operand(operand), CastKind::Transmute(dst_type_id))
        })
    }
    fn assign_cast_subtype(
        id: AssignmentId,
        dest: PlaceRef,
        operand: OperandRef,
        dst_type_id: Self::TypeId,
    ) {
        assign_to(id, dest, |h| {
            h.cast_of(take_back_operand(operand), CastKind::Subtype(dst_type_id))
        })
    }

    fn assign_binary_op(
        id: AssignmentId,
        dest: PlaceRef,
        operator: Self::BinaryOp,
        first: OperandRef,
        second: OperandRef,
    ) {
        assign_to(id, dest, |h| {
            h.binary_op_between(
                operator,
                take_back_operand(first),
                take_back_operand(second),
            )
        })
    }
    fn assign_unary_op(
        id: AssignmentId,
        dest: PlaceRef,
        operator: Self::UnaryOp,
        operand: OperandRef,
    ) {
        assign_to(id, dest, |h| {
            h.unary_op_on(operator, take_back_operand(operand))
        })
    }

    fn set_discriminant(id: AssignmentId, dest: PlaceRef, variant_index: u32) {
        assign_to(id, dest, |h| h.variant_index(variant_index))
    }
    fn assign_discriminant(id: AssignmentId, dest: PlaceRef, place: PlaceRef) {
        let place_info = take_back_place_info(place);
        let place = get_backend_place(abs::PlaceUsage::Read, |h| h.tag_of(place_info));
        assign_to(id, dest, |h| h.discriminant_from(place))
    }

    // We use slice to simplify working with the interface.
    fn assign_aggregate_array(id: AssignmentId, dest: PlaceRef, items: &[OperandRef]) {
        assign_to(id, dest, |h| {
            h.array_from(items.iter().map(|o| take_back_operand(*o)))
        })
    }
    fn assign_aggregate_tuple(id: AssignmentId, dest: PlaceRef, fields: &[OperandRef]) {
        assign_to(id, dest, |h| {
            let fields = Self::take_fields(fields);
            h.tuple_from(fields.into_iter())
        })
    }
    fn assign_aggregate_struct(id: AssignmentId, dest: PlaceRef, fields: &[OperandRef]) {
        assign_to(id, dest, |h| {
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
        assign_to(id, dest, |h| {
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
        assign_to(id, dest, |h| {
            let field = Self::take_fields(&[value]).pop().unwrap();
            h.union_from(active_field, field)
        })
    }
    fn assign_aggregate_closure(id: AssignmentId, dest: PlaceRef, upvars: &[OperandRef]) {
        assign_to(id, dest, |h| {
            let upvars = Self::take_fields(upvars);
            h.closure_from(upvars.into_iter())
        })
    }
    fn assign_aggregate_coroutine(id: AssignmentId, dest: PlaceRef, upvars: &[OperandRef]) {
        assign_to(id, dest, |h| {
            let upvars = Self::take_fields(upvars);
            h.coroutine_from(upvars.into_iter())
        })
    }
    fn assign_aggregate_coroutine_closure(id: AssignmentId, dest: PlaceRef, upvars: &[OperandRef]) {
        assign_to(id, dest, |h| {
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
        assign_to(id, dest, |h| {
            h.raw_ptr_from(
                take_back_operand(data_ptr),
                take_back_operand(metadata),
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
        assign_to(id, dest, |h| {
            h.shallow_init_box_from(take_back_operand(operand))
        })
    }

    fn assign_wrap_unsafe_binder(
        id: AssignmentId,
        dest: PlaceRef,
        operand: OperandRef,
        _binder_type_id: Self::TypeId,
    ) {
        assign_to(id, dest, |h| {
            h.shallow_init_box_from(take_back_operand(operand))
        })
    }

    fn mark_storage_dead(place: PlaceRef) {
        let place_info = take_back_place_info(place);
        let place = get_backend_place(abs::PlaceUsage::Write, |h| h.from_info(place_info));
        memory(|h| h.mark_dead(place));
    }

    fn take_branch_false(info: SwitchInfo) {
        switch(info, |h| h.take(false.into()))
    }
    fn take_branch_ow_bool(info: SwitchInfo) {
        switch(info, |h| h.take_otherwise(vec![false.into()]))
    }

    fn take_branch_int(info: SwitchInfo, value_bit_rep: u128, bit_size: u64, is_signed: bool) {
        switch(info, |h| {
            h.take(Constant::Int {
                bit_rep: value_bit_rep,
                ty: IntType {
                    bit_size,
                    is_signed,
                },
            })
        })
    }
    fn take_branch_ow_int(info: SwitchInfo, non_values: &[u128], bit_size: u64, is_signed: bool) {
        switch(info, |h| {
            h.take_otherwise(
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
        switch(info, |h| h.take(value.into()))
    }
    fn take_branch_ow_char(info: SwitchInfo, non_values: &[char]) {
        switch(info, |h| {
            h.take_otherwise(non_values.iter().map(|c| (*c).into()).collect())
        })
    }

    fn assert_bounds_check(info: AssertionInfo, len: OperandRef, index: OperandRef) {
        let assert_kind = AssertKind::BoundsCheck {
            len: take_back_operand(len),
            index: take_back_operand(index),
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
            take_back_operand(first),
            take_back_operand(second),
        );
        Self::assert(info, assert_kind)
    }
    fn assert_overflow_neg(info: AssertionInfo, operand: OperandRef) {
        let assert_kind = AssertKind::OverflowNeg(take_back_operand(operand));
        Self::assert(info, assert_kind)
    }
    fn assert_div_by_zero(info: AssertionInfo, operand: OperandRef) {
        let assert_kind = AssertKind::DivisionByZero(take_back_operand(operand));
        Self::assert(info, assert_kind)
    }
    fn assert_rem_by_zero(info: AssertionInfo, operand: OperandRef) {
        let assert_kind = AssertKind::RemainderByZero(take_back_operand(operand));
        Self::assert(info, assert_kind)
    }
    fn assert_misaligned_ptr_deref(info: AssertionInfo, required: OperandRef, found: OperandRef) {
        let assert_kind = AssertKind::MisalignedPointerDereference {
            required: take_back_operand(required),
            found: take_back_operand(found),
        };
        Self::assert(info, assert_kind)
    }
    fn assert_null_ptr_deref(info: AssertionInfo) {
        let assert_kind = AssertKind::NullPointerDereference;
        Self::assert(info, assert_kind)
    }
    fn assert_invalid_enum_ctn(info: AssertionInfo, discr: OperandRef) {
        let assert_kind = AssertKind::InvalidEnumConstruction(take_back_operand(discr));
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
        func_control(|h| {
            h.before_call(
                def.into(),
                call_site,
                take_back_operand(func),
                args.iter().map(|o| take_back_operand(*o)),
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
        func_control(|h| h.ret(ret_point))
    }
    /// Overrides (forces) the return value of a function.
    /// In an external call chain, the value will be kept as the return value
    /// until it is consumed at the point of return to an internal caller.
    #[tracing::instrument(target = "pri::call", level = "debug")]
    fn override_return_value(operand: OperandRef) {
        func_control(|h| h.override_return_value(take_back_operand(operand)))
    }
    #[tracing::instrument(target = "pri::call", level = "debug")]
    fn after_call_func(id: AssignmentId, dest: PlaceRef) {
        let dest_place = take_place_info_to_write(dest);
        func_control(|h| h.after_call(id, dest_place))
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
        let src_ptr = take_back_operand(src_ptr);
        let dst_ptr = take_back_operand(dst_ptr);
        let count = take_back_operand(count);
        raw_memory(|h| {
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
        let ptr = take_back_operand(ptr);
        let val = take_back_operand(val);
        let count = take_back_operand(count);
        raw_memory(|h| h.set(id, ptr, conc_ptr, val, count, conc_count, ptr_type_id));
    }

    fn intrinsic_memory_swap(
        id: AssignmentId,
        first_ptr: OperandRef,
        conc_first_ptr: RawAddress,
        ptr_type_id: Self::TypeId,
        second_ptr: OperandRef,
        conc_second_ptr: RawAddress,
    ) {
        let first_ptr = take_back_operand(first_ptr);
        let second_ptr = take_back_operand(second_ptr);
        raw_memory(|h| {
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
        let old = take_back_operand(old);

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

impl BasicPri {
    fn push_const_operand<T: Into<Constant>>(constant: T) -> OperandRef {
        push_operand(|o| o.const_from(constant.into()))
    }

    fn set_place_type(place: PlaceRef, ty: ValueType) {
        mut_place_info(place, |p, place| p.metadata(place).set_primitive_type(ty));
    }

    fn take_fields(fields: &[OperandRef]) -> Vec<FieldImpl> {
        let fields = fields.iter().map(|o| take_back_operand(*o));
        fields.map(Into::<FieldImpl>::into).collect()
    }

    fn assign_cast_pointer(
        id: AssignmentId,
        dest: PlaceRef,
        operand: OperandRef,
        dst_type_id: TypeId,
    ) {
        assign_to(id, dest, |h| {
            h.cast_of(take_back_operand(operand), CastKind::ToPointer(dst_type_id))
        })
    }

    fn assert(info: AssertionInfo, assert_kind: AssertKind<OperandImpl>) {
        constraint_at(info.location, |h| {
            h.assert(
                take_back_operand(info.condition),
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
        let src_ptr = take_back_operand(ptr);
        let src_place = raw_memory(|h| {
            h.place_from_ptr(src_ptr.clone(), conc_ptr, ptr_type_id, PlaceUsage::Read)
        });
        let src_pointee_value = take_back_operand(push_operand(|h| h.copy_of(src_place.clone())));
        assign_to(id, dest, |h| h.use_of(src_pointee_value))
    }

    fn store(
        id: AssignmentId,
        ptr: OperandRef,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        value: OperandRef,
    ) {
        let dst_ptr = take_back_operand(ptr);
        let dst_place =
            raw_memory(|h| h.place_from_ptr(dst_ptr, conc_ptr, ptr_type_id, PlaceUsage::Write));
        let value = take_back_operand(value);
        assign_to_place(id, dst_place, |h| h.use_of(value))
    }

    fn update_by_ptr_return_old(
        id: AssignmentId,
        ptr: OperandRef,
        conc_ptr: RawAddress,
        ptr_type_id: TypeId,
        src: OperandRef,
        prev_dest: PlaceRef,
        ptr_update_action: impl FnOnce(
            <BackendImpl as RuntimeBackend>::AssignmentHandler<'_>,
            OperandImpl,
            OperandImpl,
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
            <BackendImpl as RuntimeBackend>::AssignmentHandler<'_>,
            OperandImpl,
            OperandImpl,
        ),
        dest_assign_action: impl FnOnce(
            <BackendImpl as RuntimeBackend>::AssignmentHandler<'_>,
            OperandImpl,
        ),
    ) {
        let ptr = take_back_operand(ptr);
        let ptr_place =
            raw_memory(|h| h.place_from_ptr(ptr.clone(), conc_ptr, ptr_type_id, PlaceUsage::Read));
        let current = take_back_operand(push_operand(|h| h.copy_of(ptr_place.clone())));

        let ptr_place =
            raw_memory(|h| h.place_from_ptr(ptr, conc_ptr, ptr_type_id, PlaceUsage::Write));
        let src = take_back_operand(src);
        assign_to_place(id, ptr_place, |h| {
            ptr_update_action(h, current.clone(), src)
        });

        assign_to(id, prev_dest, |h| dest_assign_action(h, current.clone()));
    }

    fn enter_func_with_tupling(
        def: FuncDef,
        arg_places: &[PlaceRef],
        ret_val_place: PlaceRef,
        tupling: ArgsTupling,
    ) {
        let arg_places = arg_places
            .iter()
            .map(|p| take_place_info_to_read(*p))
            .collect::<Vec<_>>();
        let ret_val_place = take_place_info_to_write(ret_val_place);
        func_control(|h| h.enter(def.into(), arg_places, ret_val_place, tupling));
    }
}
