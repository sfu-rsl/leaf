use super::{
    super::{ffi, utils},
    types::*,
};

/// The definition of the interface between the program and the runtime library.
///
/// This trait provides a compile-time guarantee that the list of functions
/// is kept consistent between the runtime library, its exported C ABI, and the
/// shim. User is required to implement this trait wherever the list of functions
/// is used.
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub trait ProgramRuntimeInterface {
    type U128;
    type Char;
    type ConstStr;
    type ConstByteStr;
    type Slice<'a, T: 'a>;
    type TypeId;
    type BinaryOp;
    type UnaryOp;
    type AtomicOrdering;
    type AtomicBinaryOp;
    type DebugInfo;
    type Tag;

    macros::list_func_decls! { modifier: utils::identity, (from Self) }
}

/// A marker trait to make sure that the FFI/ABI is the same between
/// the runtime library and the shim.
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub trait FfiPri:
    // This is currently not supported by the compiler.
    // for<'a, T>
    ProgramRuntimeInterface<
        U128 = ffi::U128Pack,
        Char = ffi::CharPack,
        ConstStr = ffi::ConstStrPack,
        ConstByteStr = ffi::ConstByteStrPack,
        // Slice<'a, T> = ffi::SlicePack<T>,
        TypeId = ffi::U128Pack<TypeId>,
        BinaryOp = BinaryOp,
        UnaryOp = UnaryOp,
        AtomicOrdering = AtomicOrdering,
        AtomicBinaryOp = AtomicBinaryOp,
        DebugInfo = ffi::DebugInfo,
        Tag = ffi::ConstStrPack,
    >
{
}

pub mod macros {
    #[cfg_attr(not(core_build), macro_export)]
    macro_rules! self_slice_of { ($t:ty) => { Self::Slice<'_, $t> }; }

    #[cfg_attr(not(core_build), macro_export)]
    macro_rules! slice_pack_of { ($t:ty) => { common::ffi::SlicePack<$t> }; }

    /* NOTE: What are these macros for?
     * The list of PRI functions should get repeated in multiple places:
     * - in the runtime library, the actual PRI calling the backend.
     * - in the runtime library, the exported C ABI.
     * - in the runtime shim, the external functions.
     * - in the runtime shim, the exported Rust ABI.
     * - in delegations
     * But there are subtle differences between them concerning parameter types,
     * the modifiers of the functions, and their body.
     * We should be able to repeat the list easily and clean as much as possible.
     * Another motivation is to keep this list in one place to be easy to maintain.
     *
     * Why the macros are complicated and ugly?
     * - First, for the modifications, we let the user pass a `modifier` macro
     *   which receives the function and can change it to any form.
     * - Second, macro expansion works from outside to inside, so passing the
     *   function correctly to the modifier needs some tricks.
     * - Third, hygiene rules are not complete thus sometimes we need to add
     *   parentheses and braces to fix that.
     * - Fourth, to simplify some rules we use token trees which introduces
     *   braces in some places.
     */

    /* How to add/modify functions to the PRI?
     * - Add or modify the function's signature in the list below.
     * - Implement the actual functionality in `runtime::pri`.
     * - Ideally, all other use cases of these functions should be updated automatically.
     */

    #[cfg_attr(not(core_build), macro_export)]
    macro_rules! pass_func_decls_to {
    ($macro:ident) => {
        $macro! {
            // ----- Interaction -----
          { fn init_runtime_lib() }
          { fn shutdown_runtime_lib() }

          #[allow(unused_parens)]
          { fn debug_info(info: ($dbg_info_ty)) }

          #[allow(unused_parens)]
          { fn push_tag(tag: ($tag_ty)) }
          { fn pop_tag() }

          // ----- Place -----
          { fn ref_place_return_value() -> PlaceRef }
          { fn ref_place_argument(local_index: LocalIndex) -> PlaceRef }
          { fn ref_place_local(local_index: LocalIndex) -> PlaceRef }

          { fn ref_place_deref(place: PlaceRef) }
          { fn ref_place_field(place: PlaceRef, field: FieldIndex /*, type */) }
          { fn ref_place_index(place: PlaceRef, index_place: PlaceRef) }
          { fn ref_place_constant_index(place: PlaceRef, offset: u64, min_length: u64, from_end: bool) }
          { fn ref_place_subslice(place: PlaceRef, from: u64, to: u64, from_end: bool) }
          { fn ref_place_downcast(place: PlaceRef, variant_index: u32 /*, type */) }
          { fn ref_place_opaque_cast(place: PlaceRef /*, type */) }
          { fn ref_place_subtype(place: PlaceRef /*, type */) }

          { fn set_place_address(place: PlaceRef, raw_ptr: RawAddress) }
          #[allow(unused_parens)]
          { fn set_place_type_id(place: PlaceRef, type_id: ($type_id_ty)) }
          { fn set_place_type_bool(place: PlaceRef) }
          { fn set_place_type_char(place: PlaceRef) }
          { fn set_place_type_int(place: PlaceRef, bit_size: u64, is_signed: bool) }
          { fn set_place_type_float(place: PlaceRef, e_bits: u64, s_bits: u64) }
          { fn set_place_size(place: PlaceRef, byte_size: TypeSize) }

          // ----- Operand -----
          { fn ref_operand_copy(place: PlaceRef) -> OperandRef }
          { fn ref_operand_move(place: PlaceRef) -> OperandRef }

          { fn ref_operand_const_bool(value: bool) -> OperandRef }
          #[allow(unused_parens)]
          { fn ref_operand_const_int(bit_rep: ($u128_ty), bit_size: u64, is_signed: bool) -> OperandRef }
          #[allow(unused_parens)]
          { fn ref_operand_const_float(bit_rep: ($u128_ty), e_bits: u64, s_bits: u64) -> OperandRef }
          #[allow(unused_parens)]
          { fn ref_operand_const_char(value: ($char_ty)) -> OperandRef }
          #[allow(unused_parens)]
          { fn ref_operand_const_str(value: ($str_ty)) -> OperandRef }
          #[allow(unused_parens)]
          { fn ref_operand_const_byte_str(value: ($byte_str_ty)) -> OperandRef }
          { fn ref_operand_const_zst() -> OperandRef }
          { fn ref_operand_const_some() -> OperandRef }

          { fn new_sym_value_bool(conc_val: bool) -> OperandRef }
          { fn new_sym_value_char(conc_val: char) -> OperandRef }
          #[allow(unused_parens)]
          { fn new_sym_value_int(conc_val_bit_rep:($u128_ty), bit_size: u64, is_signed: bool) -> OperandRef }
          #[allow(unused_parens)]
          { fn new_sym_value_float(conc_val_bit_rep:($u128_ty), e_bits: u64, s_bits: u64) -> OperandRef }

          // ----- Assign -----
          { fn assign_use(id: AssignmentId, dest: PlaceRef, operand: OperandRef) }
          { fn assign_repeat(id: AssignmentId, dest: PlaceRef, operand: OperandRef, count: usize) }
          { fn assign_ref(id: AssignmentId, dest: PlaceRef, place: PlaceRef, is_mutable: bool) }
          { fn assign_thread_local_ref(id: AssignmentId, dest: PlaceRef) }
          { fn assign_raw_ptr_of(id: AssignmentId, dest: PlaceRef, place: PlaceRef, is_mutable: bool) }
          { fn assign_len(id: AssignmentId, dest: PlaceRef, place: PlaceRef) }

          // ------- Cast ---
          { fn assign_cast_char(id: AssignmentId, dest: PlaceRef, operand: OperandRef) }
          { fn assign_cast_integer(id: AssignmentId, dest: PlaceRef, operand: OperandRef, bit_size: u64, is_signed: bool) }
          { fn assign_cast_float(id: AssignmentId, dest: PlaceRef, operand: OperandRef, e_bits: u64, s_bits: u64) }
          { fn assign_cast_expose_prov(id: AssignmentId, dest: PlaceRef, operand: OperandRef) }
          #[allow(unused_parens)]
          { fn assign_cast_with_exposed_prov(id: AssignmentId, dest: PlaceRef, operand: OperandRef, dst_type_id: ($type_id_ty)) }
          #[allow(unused_parens)]
          { fn assign_cast_to_another_ptr(id: AssignmentId, dest: PlaceRef, operand: OperandRef, dst_type_id: ($type_id_ty)) }

          { fn assign_cast_unsize(id: AssignmentId, dest: PlaceRef, operand: OperandRef) }
          { fn assign_cast_sized_dyn(id: AssignmentId, dest: PlaceRef, operand: OperandRef) }
          #[allow(unused_parens)]
          { fn assign_cast_transmute(id: AssignmentId, dest: PlaceRef, operand: OperandRef, dst_type_id: ($type_id_ty)) }

          #[allow(unused_parens)]
          { fn assign_binary_op(id: AssignmentId,
              dest: PlaceRef,
              operator: ($binary_op_ty),
              first: OperandRef,
              second: OperandRef,
          ) }
          #[allow(unused_parens)]
          { fn assign_unary_op(id: AssignmentId, dest: PlaceRef, operator: ($unary_op_ty), operand: OperandRef) }

          { fn set_discriminant(id: AssignmentId, dest: PlaceRef, variant_index: u32) }
          { fn assign_discriminant(id: AssignmentId, dest: PlaceRef, place: PlaceRef) }

          #[allow(unused_parens)]
          { fn assign_aggregate_array(id: AssignmentId, dest: PlaceRef, items: ($slice_ty!(OperandRef))) }
          #[allow(unused_parens)]
          { fn assign_aggregate_tuple(id: AssignmentId, dest: PlaceRef, fields: ($slice_ty!(OperandRef))) }
          #[allow(unused_parens)]
          { fn assign_aggregate_struct(id: AssignmentId, dest: PlaceRef, fields: ($slice_ty!(OperandRef))) }
          #[allow(unused_parens)]
          { fn assign_aggregate_enum(id: AssignmentId,
              dest: PlaceRef,
              fields: ($slice_ty!(OperandRef)),
              variant: VariantIndex,
          ) }
          { fn assign_aggregate_union(id: AssignmentId, dest: PlaceRef, active_field: FieldIndex, value: OperandRef) }
          #[allow(unused_parens)]
          { fn assign_aggregate_closure(id: AssignmentId, dest: PlaceRef, upvars: ($slice_ty!(OperandRef))) }
          #[allow(unused_parens)]
          { fn assign_aggregate_coroutine(id: AssignmentId, dest: PlaceRef, upvars: ($slice_ty!(OperandRef))) }
          #[allow(unused_parens)]
          { fn assign_aggregate_coroutine_closure(id: AssignmentId, dest: PlaceRef, upvars: ($slice_ty!(OperandRef))) }
          { fn assign_aggregate_raw_ptr(id: AssignmentId, dest: PlaceRef, data_ptr: OperandRef, metadata: OperandRef, is_mutable: bool) }

          #[allow(unused_parens)]
          { fn assign_shallow_init_box(id: AssignmentId, dest: PlaceRef, operand: OperandRef, boxed_type_id: ($type_id_ty)) }

          // ----- Memory -----
          { fn mark_storage_dead(place: PlaceRef) }

          // ----- Switch -----
          #[allow(unused_parens)]
          { fn take_branch_false(info: SwitchInfo) }
          #[allow(unused_parens)]
          { fn take_branch_ow_bool(info: SwitchInfo) }
          #[allow(unused_parens)]
          { fn take_branch_int(
              info: SwitchInfo,
              value_bit_rep: ($u128_ty),
              bit_size: u64,
              is_signed: bool,
            ) }
          #[allow(unused_parens)]
          { fn take_branch_ow_int(
              info: SwitchInfo,
              non_values: ($slice_ty!($u128_ty)),
              bit_size: u64,
              is_signed: bool,
          ) }
          #[allow(unused_parens)]
          { fn take_branch_char(info: SwitchInfo, value: (($char_ty))) }
          #[allow(unused_parens)]
          { fn take_branch_ow_char(info: SwitchInfo, non_values: ($slice_ty!($char_ty))) }

          // ----- Assertion -----
          { fn assert_bounds_check(info: AssertionInfo, len: OperandRef, index: OperandRef) }
          #[allow(unused_parens)]
          { fn assert_overflow(
              info: AssertionInfo,
              operator: ($binary_op_ty),
              first: OperandRef,
              second: OperandRef,
          ) }
          { fn assert_overflow_neg(info: AssertionInfo, operand: OperandRef) }
          { fn assert_div_by_zero(info: AssertionInfo, operand: OperandRef) }
          { fn assert_rem_by_zero(info: AssertionInfo, operand: OperandRef) }
          { fn assert_misaligned_ptr_deref(
              info: AssertionInfo,
              required: OperandRef,
              found: OperandRef,
          ) }

          // ----- Calling -----
          #[allow(unused_parens)]
          { fn before_call_func(
              def: CalleeDef,
              call_site: BasicBlockIndex,
              func: OperandRef,
              args: ($slice_ty!(OperandRef)),
              are_args_tupled: bool
          ) }
          #[allow(unused_parens)]
          { fn enter_func(def: FuncDef, arg_places: &[PlaceRef], ret_val_place: PlaceRef) }
          #[allow(unused_parens)]
          { fn enter_func_tupled(
              def: FuncDef,
              arg_places: &[PlaceRef],
              ret_val_place: PlaceRef,
              tupled_arg_index: LocalIndex,
              tupled_arg_type_id: TypeId,
          ) }
          { fn return_from_func(ret_point: BasicBlockIndex) }
          { fn override_return_value(operand: OperandRef) }
          { fn after_call_func(id: AssignmentId, dest: PlaceRef) }

          // ----- Atomic -----
          { fn intrinsic_assign_rotate_left(id: AssignmentId, dest: PlaceRef, x: OperandRef, shift: OperandRef) }
          { fn intrinsic_assign_rotate_right(id: AssignmentId, dest: PlaceRef, x: OperandRef, shift: OperandRef) }
          { fn intrinsic_assign_saturating_add(id: AssignmentId, dest: PlaceRef, first: OperandRef, second: OperandRef) }
          { fn intrinsic_assign_saturating_sub(id: AssignmentId, dest: PlaceRef, first: OperandRef, second: OperandRef) }
          { fn intrinsic_assign_exact_div(id: AssignmentId, dest: PlaceRef, first: OperandRef, second: OperandRef) }
          { fn intrinsic_assign_bitreverse(id: AssignmentId, dest: PlaceRef, x: OperandRef) }
          { fn intrinsic_assign_cttz_nonzero(id: AssignmentId, dest: PlaceRef, x: OperandRef) }
          { fn intrinsic_assign_cttz(id: AssignmentId, dest: PlaceRef, x: OperandRef) }
          { fn intrinsic_assign_ctpop(id: AssignmentId, dest: PlaceRef, x: OperandRef) }
          { fn intrinsic_assign_ctlz_nonzero(id: AssignmentId, dest: PlaceRef, x: OperandRef) }
          { fn intrinsic_assign_ctlz(id: AssignmentId, dest: PlaceRef, x: OperandRef) }

          // All atomic operations have an ordering, majority get applied on a pointer.
          #[allow(unused_parens)]
          { fn intrinsic_atomic_load(
              ordering: ($atomic_ord_ty),
              id: AssignmentId,
              ptr: OperandRef,
              ptr_type_id: ($type_id_ty),
              dest: PlaceRef,
          ) }
          #[allow(unused_parens)]
          { fn intrinsic_atomic_store(
              ordering: ($atomic_ord_ty),
              id: AssignmentId,
              ptr: OperandRef,
              ptr_type_id: ($type_id_ty),
              val: OperandRef,
          ) }
          #[allow(unused_parens)]
          { fn intrinsic_atomic_xchg(
              ordering: ($atomic_ord_ty),
              id: AssignmentId,
              ptr: OperandRef,
              ptr_type_id: ($type_id_ty),
              val: OperandRef,
              prev_dest: PlaceRef,
          ) }
          #[allow(unused_parens)]
          { fn intrinsic_atomic_cxchg(
              ordering: ($atomic_ord_ty),
              id: AssignmentId,
              ptr: OperandRef,
              ptr_type_id: ($type_id_ty),
              failure_ordering: ($atomic_ord_ty),
              weak: bool,
              old: OperandRef,
              src: OperandRef,
              prev_dest: PlaceRef,
          ) }
          #[allow(unused_parens)]
          { fn intrinsic_atomic_binary_op(
              ordering: ($atomic_ord_ty),
              id: AssignmentId,
              ptr: OperandRef,
              ptr_type_id: ($type_id_ty),
              operator: ($atomic_bin_op_ty),
              src: OperandRef,
              prev_dest: PlaceRef,
          ) }
          #[allow(unused_parens)]
          { fn intrinsic_atomic_fence(
              ordering: ($atomic_ord_ty),
              single_thread: bool,
          ) }
        }
    };
}

    macro_rules! make_pass_func_names_to_macro {
    ($($(#[$($attr: meta)*])* {fn $name:ident ($($arg:tt)*) $(-> $($ret_ty:tt)*)?})*) => {
        #[cfg_attr(not(core_build), macro_export)]
        macro_rules! pass_func_names_to {
            ($$macro:ident, one_by_one) => {
                $(
                    $$macro!($name);
                )*
            };
            ($$macro:ident, all_comma_separated) => {
                $$macro! {
                    $($name),*
                }
            };
        }
    };
}
    pass_func_decls_to!(make_pass_func_names_to_macro);

    #[allow(unused_macros)]
    macro_rules! make_list_func_decls_macro {
    ($($(#[$($attr: meta)*])*{$($sig:tt)+})+) => {
        #[cfg_attr(not(core_build), macro_export)]
        macro_rules! list_func_decls {
            (
                modifier: $$modifier:path,
                (
                    u128: $$u128_ty:ty,
                    char: $$char_ty:ty,
                    &str: $$str_ty:ty,
                    &[u8]: $$byte_str_ty:ty,
                    // NOTE: Slice is received as a macro to enable having [T].
                    slice: $$slice_ty:path,
                    type_id: $$type_id_ty:ty,
                    binary_op: $$binary_op_ty:ty,
                    unary_op: $$unary_op_ty:ty,
                    atomic_ord: $$atomic_ord_ty:ty,
                    atomic_bin_op: $atomic_bin_op_ty:ty,
                    dbg_info: $$dbg_info_ty:ty,
                    tag: $$tag_ty:ty
                    $$(,)?
                )
            ) => {
                $(
                    $modifier! { $(#[$($attr)*])* $($sig)+ ; }
                )+
            };
            (modifier: $$modifier:path) => {
                $$crate::leaf::common::pri::macros::list_func_decls! {
                    modifier: $$modifier,
                    (
                        u128: (),
                        char: (),
                        &str: (),
                        &[u8]: (),
                        slice: $crate::leaf::common::utils::identity,
                        type_id: (),
                        binary_op: (),
                        unary_op: (),
                        atomic_ord: (),
                        atomic_bin_op: (),
                        dbg_info: (),
                        tag: (),
                    )
                }
            };
            (modifier: $$modifier:path, (from Self)) => {
                $$crate::leaf::common::pri::macros::list_func_decls! {
                    modifier: $$modifier,
                    (
                        u128: Self::U128,
                        char: Self::Char,
                        &str: Self::ConstStr,
                        &[u8]: Self::ConstByteStr,
                        slice: $$crate::leaf::common::pri::macros::self_slice_of,
                        type_id: Self::TypeId,
                        binary_op: Self::BinaryOp,
                        unary_op: Self::UnaryOp,
                        atomic_ord: Self::AtomicOrdering,
                        atomic_bin_op: Self::AtomicBinaryOp,
                        dbg_info: Self::DebugInfo,
                        tag: Self::Tag,
                    )
                }
            };
            (modifier: $$modifier:path, (from common::ffi)) => {
                $$crate::leaf::common::pri::macros::list_func_decls! {
                    modifier: $$modifier,
                    (
                        u128: U128Pack,
                        char: CharPack,
                        &str: ConstStrPack,
                        &[u8]: ConstByteStrPack,
                        slice: $$crate::leaf::common::pri::macros::slice_pack_of,
                        type_id: U128Pack<TypeId>,
                        binary_op: common::pri::BinaryOp,
                        unary_op: common::pri::UnaryOp,
                        atomic_ord: common::pri::AtomicOrdering,
                        atomic_bin_op: common::pri::AtomicBinaryOp,
                        dbg_info: common::ffi::DebugInfo,
                        tag: ConstStrPack,
                    )
                }
            };
        }
    }
}
    // NOTE: Because of a bug in the compiler, we need to perform the expansion manually.
    // pass_func_decls_to!(make_list_func_decls_macro);
    // Recursive expansion of pass_func_decls_to! macro
    // =================================================

    #[cfg_attr(not(core_build), macro_export)]
    macro_rules! list_func_decls {
        (modifier: $modifier:path,(u128: $u128_ty:ty,char: $char_ty:ty, &str: $str_ty:ty, &[u8]: $byte_str_ty:ty,slice: $slice_ty:path,type_id: $type_id_ty:ty,binary_op: $binary_op_ty:ty,unary_op: $unary_op_ty:ty,atomic_ord: $atomic_ord_ty:ty,atomic_bin_op: $atomic_bin_op_ty:ty,dbg_info: $dbg_info_ty:ty,tag: $tag_ty:ty$(,)?)) => {
            $modifier!{
                fn init_runtime_lib();
            }$modifier!{
                fn shutdown_runtime_lib();
            }$modifier!{
                #[allow(unused_parens)]fn debug_info(info:($dbg_info_ty));
            }$modifier!{
                #[allow(unused_parens)]fn push_tag(tag:($tag_ty));
            }$modifier!{
                fn pop_tag();
            }$modifier!{
                fn ref_place_return_value()->PlaceRef;
            }$modifier!{
                fn ref_place_argument(local_index:LocalIndex)->PlaceRef;
            }$modifier!{
                fn ref_place_local(local_index:LocalIndex)->PlaceRef;
            }$modifier!{
                fn ref_place_deref(place:PlaceRef);
            }$modifier!{
                fn ref_place_field(place:PlaceRef,field:FieldIndex);
            }$modifier!{
                fn ref_place_index(place:PlaceRef,index_place:PlaceRef);
            }$modifier!{
                fn ref_place_constant_index(place:PlaceRef,offset:u64,min_length:u64,from_end:bool);
            }$modifier!{
                fn ref_place_subslice(place:PlaceRef,from:u64,to:u64,from_end:bool);
            }$modifier!{
                fn ref_place_downcast(place:PlaceRef,variant_index:u32);
            }$modifier!{
                fn ref_place_opaque_cast(place:PlaceRef);
            }$modifier!{
                fn ref_place_subtype(place:PlaceRef);
            }$modifier!{
                fn set_place_address(place:PlaceRef,raw_ptr:RawAddress);
            }$modifier!{
                #[allow(unused_parens)]fn set_place_type_id(place:PlaceRef,type_id:($type_id_ty));
            }$modifier!{
                fn set_place_type_bool(place:PlaceRef);
            }$modifier!{
                fn set_place_type_char(place:PlaceRef);
            }$modifier!{
                fn set_place_type_int(place:PlaceRef,bit_size:u64,is_signed:bool);
            }$modifier!{
                fn set_place_type_float(place:PlaceRef,e_bits:u64,s_bits:u64);
            }$modifier!{
                fn set_place_size(place:PlaceRef,byte_size:TypeSize);
            }$modifier!{
                fn ref_operand_copy(place:PlaceRef)->OperandRef;
            }$modifier!{
                fn ref_operand_move(place:PlaceRef)->OperandRef;
            }$modifier!{
                fn ref_operand_const_bool(value:bool)->OperandRef;
            }$modifier!{
                #[allow(unused_parens)]fn ref_operand_const_int(bit_rep:($u128_ty),bit_size:u64,is_signed:bool)->OperandRef;
            }$modifier!{
                #[allow(unused_parens)]fn ref_operand_const_float(bit_rep:($u128_ty),e_bits:u64,s_bits:u64)->OperandRef;
            }$modifier!{
                #[allow(unused_parens)]fn ref_operand_const_char(value:($char_ty))->OperandRef;
            }$modifier!{
                #[allow(unused_parens)]fn ref_operand_const_str(value:($str_ty))->OperandRef;
            }$modifier!{
                #[allow(unused_parens)]fn ref_operand_const_byte_str(value:($byte_str_ty))->OperandRef;
            }$modifier!{
                fn ref_operand_const_zst()->OperandRef;
            }$modifier!{
                fn ref_operand_const_some()->OperandRef;
            }$modifier!{
                fn new_sym_value_bool(conc_val:bool)->OperandRef;
            }$modifier!{
                fn new_sym_value_char(conc_val:char)->OperandRef;
            }$modifier!{
                #[allow(unused_parens)]fn new_sym_value_int(conc_val_bit_rep:($u128_ty),bit_size:u64,is_signed:bool)->OperandRef;
            }$modifier!{
                #[allow(unused_parens)]fn new_sym_value_float(conc_val_bit_rep:($u128_ty),e_bits:u64,s_bits:u64)->OperandRef;
            }$modifier!{
                fn assign_use(id:AssignmentId,dest:PlaceRef,operand:OperandRef);
            }$modifier!{
                fn assign_repeat(id:AssignmentId,dest:PlaceRef,operand:OperandRef,count:usize);
            }$modifier!{
                fn assign_ref(id:AssignmentId,dest:PlaceRef,place:PlaceRef,is_mutable:bool);
            }$modifier!{
                fn assign_thread_local_ref(id:AssignmentId,dest:PlaceRef);
            }$modifier!{
                fn assign_raw_ptr_of(id:AssignmentId,dest:PlaceRef,place:PlaceRef,is_mutable:bool);
            }$modifier!{
                fn assign_len(id:AssignmentId,dest:PlaceRef,place:PlaceRef);
            }$modifier!{
                fn assign_cast_char(id:AssignmentId,dest:PlaceRef,operand:OperandRef);
            }$modifier!{
                fn assign_cast_integer(id:AssignmentId,dest:PlaceRef,operand:OperandRef,bit_size:u64,is_signed:bool);
            }$modifier!{
                fn assign_cast_float(id:AssignmentId,dest:PlaceRef,operand:OperandRef,e_bits:u64,s_bits:u64);
            }$modifier!{
                fn assign_cast_expose_prov(id:AssignmentId,dest:PlaceRef,operand:OperandRef);
            }$modifier!{
                #[allow(unused_parens)]fn assign_cast_with_exposed_prov(id:AssignmentId,dest:PlaceRef,operand:OperandRef,dst_type_id:($type_id_ty));
            }$modifier!{
                #[allow(unused_parens)]fn assign_cast_to_another_ptr(id:AssignmentId,dest:PlaceRef,operand:OperandRef,dst_type_id:($type_id_ty));
            }$modifier!{
                fn assign_cast_unsize(id:AssignmentId,dest:PlaceRef,operand:OperandRef);
            }$modifier!{
                fn assign_cast_sized_dyn(id:AssignmentId,dest:PlaceRef,operand:OperandRef);
            }$modifier!{
                #[allow(unused_parens)]fn assign_cast_transmute(id:AssignmentId,dest:PlaceRef,operand:OperandRef,dst_type_id:($type_id_ty));
            }$modifier!{
                #[allow(unused_parens)]fn assign_binary_op(id:AssignmentId,dest:PlaceRef,operator:($binary_op_ty),first:OperandRef,second:OperandRef,);
            }$modifier!{
                #[allow(unused_parens)]fn assign_unary_op(id:AssignmentId,dest:PlaceRef,operator:($unary_op_ty),operand:OperandRef);
            }$modifier!{
                fn set_discriminant(id:AssignmentId,dest:PlaceRef,variant_index:u32);
            }$modifier!{
                fn assign_discriminant(id:AssignmentId,dest:PlaceRef,place:PlaceRef);
            }$modifier!{
                #[allow(unused_parens)]fn assign_aggregate_array(id:AssignmentId,dest:PlaceRef,items:($slice_ty!(OperandRef)));
            }$modifier!{
                #[allow(unused_parens)]fn assign_aggregate_tuple(id:AssignmentId,dest:PlaceRef,fields:($slice_ty!(OperandRef)));
            }$modifier!{
                #[allow(unused_parens)]fn assign_aggregate_struct(id:AssignmentId,dest:PlaceRef,fields:($slice_ty!(OperandRef)));
            }$modifier!{
                #[allow(unused_parens)]fn assign_aggregate_enum(id:AssignmentId,dest:PlaceRef,fields:($slice_ty!(OperandRef)),variant:VariantIndex,);
            }$modifier!{
                fn assign_aggregate_union(id:AssignmentId,dest:PlaceRef,active_field:FieldIndex,value:OperandRef);
            }$modifier!{
                #[allow(unused_parens)]fn assign_aggregate_closure(id:AssignmentId,dest:PlaceRef,upvars:($slice_ty!(OperandRef)));
            }$modifier!{
                #[allow(unused_parens)]fn assign_aggregate_coroutine(id:AssignmentId,dest:PlaceRef,upvars:($slice_ty!(OperandRef)));
            }$modifier!{
                #[allow(unused_parens)]fn assign_aggregate_coroutine_closure(id:AssignmentId,dest:PlaceRef,upvars:($slice_ty!(OperandRef)));
            }$modifier!{
                fn assign_aggregate_raw_ptr(id:AssignmentId,dest:PlaceRef,data_ptr:OperandRef,metadata:OperandRef,is_mutable:bool);
            }$modifier!{
                #[allow(unused_parens)]fn assign_shallow_init_box(id:AssignmentId,dest:PlaceRef,operand:OperandRef,boxed_type_id:($type_id_ty));
            }$modifier!{
                fn mark_storage_dead(place:PlaceRef);
            }$modifier!{
                #[allow(unused_parens)]fn take_branch_false(info:SwitchInfo);
            }$modifier!{
                #[allow(unused_parens)]fn take_branch_ow_bool(info:SwitchInfo);
            }$modifier!{
                #[allow(unused_parens)]fn take_branch_int(info:SwitchInfo,value_bit_rep:($u128_ty),bit_size:u64,is_signed:bool,);
            }$modifier!{
                #[allow(unused_parens)]fn take_branch_ow_int(info:SwitchInfo,non_values:($slice_ty!($u128_ty)),bit_size:u64,is_signed:bool,);
            }$modifier!{
                #[allow(unused_parens)]fn take_branch_char(info:SwitchInfo,value:(($char_ty)));
            }$modifier!{
                #[allow(unused_parens)]fn take_branch_ow_char(info:SwitchInfo,non_values:($slice_ty!($char_ty)));
            }$modifier!{
                fn assert_bounds_check(info:AssertionInfo,len:OperandRef,index:OperandRef);
            }$modifier!{
                #[allow(unused_parens)]fn assert_overflow(info:AssertionInfo,operator:($binary_op_ty),first:OperandRef,second:OperandRef,);
            }$modifier!{
                fn assert_overflow_neg(info:AssertionInfo,operand:OperandRef);
            }$modifier!{
                fn assert_div_by_zero(info:AssertionInfo,operand:OperandRef);
            }$modifier!{
                fn assert_rem_by_zero(info:AssertionInfo,operand:OperandRef);
            }$modifier!{
                fn assert_misaligned_ptr_deref(info:AssertionInfo,required:OperandRef,found:OperandRef,);
            }$modifier!{
                #[allow(unused_parens)]fn before_call_func(def:CalleeDef,call_site:BasicBlockIndex,func:OperandRef,args:($slice_ty!(OperandRef)),are_args_tupled:bool);
            }$modifier!{
                #[allow(unused_parens)]fn enter_func(def:FuncDef,arg_places: &[PlaceRef],ret_val_place:PlaceRef);
            }$modifier!{
                #[allow(unused_parens)]fn enter_func_tupled(def:FuncDef,arg_places: &[PlaceRef],ret_val_place:PlaceRef,tupled_arg_index:LocalIndex,tupled_arg_type_id:TypeId,);
            }$modifier!{
                fn return_from_func(ret_point:BasicBlockIndex);
            }$modifier!{
                fn override_return_value(operand:OperandRef);
            }$modifier!{
                fn after_call_func(id: AssignmentId, dest: PlaceRef);
            }$modifier!{
                fn intrinsic_assign_rotate_left(id:AssignmentId,dest:PlaceRef,x:OperandRef,shift:OperandRef);
            }$modifier!{
                fn intrinsic_assign_rotate_right(id:AssignmentId,dest:PlaceRef,x:OperandRef,shift:OperandRef);
            }$modifier!{
                fn intrinsic_assign_saturating_add(id:AssignmentId,dest:PlaceRef,first:OperandRef,second:OperandRef);
            }$modifier!{
                fn intrinsic_assign_saturating_sub(id:AssignmentId,dest:PlaceRef,first:OperandRef,second:OperandRef);
            }$modifier!{
                fn intrinsic_assign_exact_div(id:AssignmentId,dest:PlaceRef,first:OperandRef,second:OperandRef);
            }$modifier!{
                fn intrinsic_assign_bitreverse(id:AssignmentId,dest:PlaceRef,x:OperandRef);
            }$modifier!{
                fn intrinsic_assign_cttz_nonzero(id:AssignmentId,dest:PlaceRef,x:OperandRef);
            }$modifier!{
                fn intrinsic_assign_cttz(id:AssignmentId,dest:PlaceRef,x:OperandRef);
            }$modifier!{
                fn intrinsic_assign_ctpop(id:AssignmentId,dest:PlaceRef,x:OperandRef);
            }$modifier!{
                fn intrinsic_assign_ctlz_nonzero(id:AssignmentId,dest:PlaceRef,x:OperandRef);
            }$modifier!{
                fn intrinsic_assign_ctlz(id:AssignmentId,dest:PlaceRef,x:OperandRef);
            }$modifier!{
                #[allow(unused_parens)]fn intrinsic_atomic_load(ordering:($atomic_ord_ty),id:AssignmentId,ptr:OperandRef,ptr_type_id:($type_id_ty),dest:PlaceRef,);
            }$modifier!{
                #[allow(unused_parens)]fn intrinsic_atomic_store(ordering:($atomic_ord_ty),id:AssignmentId,ptr:OperandRef,ptr_type_id:($type_id_ty),val:OperandRef,);
            }$modifier!{
                #[allow(unused_parens)]fn intrinsic_atomic_xchg(ordering:($atomic_ord_ty),id:AssignmentId,ptr:OperandRef,ptr_type_id:($type_id_ty),val:OperandRef,prev_dest:PlaceRef,);
            }$modifier!{
                #[allow(unused_parens)]fn intrinsic_atomic_cxchg(ordering:($atomic_ord_ty),id:AssignmentId,ptr:OperandRef,ptr_type_id:($type_id_ty),failure_ordering:($atomic_ord_ty),weak:bool,old:OperandRef,src:OperandRef,prev_dest:PlaceRef,);
            }$modifier!{
                #[allow(unused_parens)]fn intrinsic_atomic_binary_op(ordering:($atomic_ord_ty),id:AssignmentId,ptr:OperandRef,ptr_type_id:($type_id_ty),operator:($atomic_bin_op_ty),src:OperandRef,prev_dest:PlaceRef,);
            }$modifier!{
                #[allow(unused_parens)]fn intrinsic_atomic_fence(ordering:($atomic_ord_ty),single_thread:bool,);
            }
        };
        (modifier: $modifier:path) => {
            $crate::leaf::common::pri::macros::list_func_decls!{
                modifier: $modifier,(u128:(),char:(), &str:(), &[u8]:(),slice:crate::leaf::common::utils::identity,type_id:(),binary_op:(),unary_op:(),atomic_ord:(),atomic_bin_op:(),dbg_info:(),tag:(),)
            }
        };
        (modifier: $modifier:path,(from Self)) => {
            $crate::leaf::common::pri::macros::list_func_decls!{
                modifier: $modifier,(u128:Self::U128,char:Self::Char, &str:Self::ConstStr, &[u8]:Self::ConstByteStr,slice: $crate::leaf::common::pri::macros::self_slice_of,type_id:Self::TypeId,binary_op:Self::BinaryOp,unary_op:Self::UnaryOp,atomic_ord:Self::AtomicOrdering,atomic_bin_op:Self::AtomicBinaryOp,dbg_info:Self::DebugInfo,tag:Self::Tag,)
            }
        };
        (modifier: $modifier:path,(from common::ffi)) => {
            $crate::leaf::common::pri::macros::list_func_decls!{
                modifier: $modifier,(u128:U128Pack,char:CharPack, &str:ConstStrPack, &[u8]:ConstByteStrPack,slice: $crate::leaf::common::pri::macros::slice_pack_of,type_id:U128Pack<TypeId>,binary_op:common::pri::BinaryOp,unary_op:common::pri::UnaryOp,atomic_ord:common::pri::AtomicOrdering,atomic_bin_op:common::pri::AtomicBinaryOp,dbg_info:common::ffi::DebugInfo,tag:ConstStrPack,)
            }
        };
    }
    #[cfg(not(core_build))]
    pub use {list_func_decls, pass_func_names_to, self_slice_of, slice_pack_of};
    #[cfg(core_build)]
    #[stable(feature = "rust1", since = "1.0.0")]
    pub(crate) use {list_func_decls, pass_func_names_to, self_slice_of, slice_pack_of};
}
