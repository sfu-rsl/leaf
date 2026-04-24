use core::{
    intrinsics::{self, transmute, transmute_unchecked},
    marker::FnPtr,
    ops::{CoerceUnsized, Receiver},
};

use super::common::{
    self,
    pri::{refs::encoding as r_enc, *},
    types::{BasicBlockLocation, DefId, InstanceKindDiscr, InstanceKindId},
};

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub static CH_MODULE_MARKER: u8 = 0;

/* These fields serve as exported symbols to get the types of the desired
 * arguments easier in the compiler. */
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub static PLACE_REF_TYPE_HOLDER: PlaceRef = 0;
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub static OPERAND_REF_TYPE_HOLDER: OperandRef = 0;
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub static BINARY_OP_TYPE_HOLDER: BinaryOp = BinaryOp::ADD;
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub static UNARY_OP_TYPE_HOLDER: UnaryOp = UnaryOp::NEG;

/* NOTE: The const version of this conversion is unstable
 * and causes errors during the compilation of the core library.
 */
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub fn f32_to_bits(value: f32) -> u128 {
    unsafe { transmute::<f32, u32>(value) as u128 }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub fn f64_to_bits(value: f64) -> u128 {
    unsafe { transmute::<f64, u64>(value) as u128 }
}

/* NOTE:
 * This is a workaround to prevent the compiler from removing the generic
 * functions from the exported symbols as they are unused.
 */

#[used]
static _PLACE_WITH_ADDRESS_TYPED_REFERENCER: fn(PlaceRef, *const u32) -> PlaceRef =
    place_with_address_typed::<u32>;

#[used]
static _TYPE_ID_OF_REFERENCER: fn() -> TypeId = type_id_of::<u32>;

#[used]
static _SIZE_OF_REFERENCER: fn() -> TypeSize = size_of::<u32>;

#[used]
static _SWITCH_INFO_REFERENCER: fn(BasicBlockIndex, OperandRef) -> SwitchInfo = switch_info;

#[used]
static _CONST_BINARY_OP_OF_REFERENCER: fn(u8) -> BinaryOp = const_binary_op_of;

#[used]
static _CONST_UNARY_OP_OF_REFERENCER: fn(u8) -> UnaryOp = const_unary_op_of;

#[used]
static _CONST_ATOMIC_ORD_OF_REFERENCER: fn(u8) -> AtomicOrdering = const_atomic_ord_of;

#[used]
static _CONST_ATOMIC_BINARY_OP_OF_REFERENCER: fn(u8) -> AtomicBinaryOp = const_atomic_binary_op_of;

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub fn place_with_address_typed<T>(place: PlaceRef, address: *const T) -> PlaceRef {
    super::place_with_address(place, address as RawAddress)
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(
    core_build,
    rustc_const_unstable(feature = "const_eval_select", issue = "124625")
)]
#[inline(always)]
pub const fn type_id_of<T: ?Sized + 'static>() -> TypeId {
    common::utils::type_id_of::<T>()
}
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub const fn size_of<T>() -> TypeSize {
    const { intrinsics::size_of::<T>() as TypeSize }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub const fn switch_info(node_location: BasicBlockIndex, discriminant: OperandRef) -> SwitchInfo {
    SwitchInfo {
        node_location,
        discriminant,
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub const fn assertion_info(
    location: BasicBlockIndex,
    condition: OperandRef,
    expected: bool,
) -> AssertionInfo {
    AssertionInfo {
        location,
        condition,
        expected,
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub fn callee_def_static<F: FnPtr>(func_addr: F) -> CalleeDef {
    CalleeDef {
        static_addr: func_addr.addr(),
        as_virtual: None,
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub fn callee_def_maybe_virtual<F: FnPtr, Pointee: ?Sized, R: Receiver<Target = Pointee>>(
    func_addr: F,
    receiver: &R,
    identifier: u64,
) -> CalleeDef
where
    <Pointee as core::ptr::Pointee>::Metadata: 'static,
{
    CalleeDef {
        static_addr: func_addr.addr(),
        as_virtual: {
            if const { is_dyn::<Pointee>() } {
                Some((
                    unsafe {
                        // NOTE: UB happens if it is not really a dyn type.
                        let metadata = metadata_from_receiver::<Pointee, R>(receiver);
                        transmute_unchecked(metadata)
                    },
                    identifier,
                ))
            } else {
                None
            }
        },
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub fn func_def_static<F: FnPtr>(
    addr: F,
    instance_kind_discr: InstanceKindDiscr,
    crate_id: u32,
    body_id: u32,
) -> FuncDef {
    FuncDef {
        static_addr: addr.addr(),
        as_dyn_method: None,
        body_id: InstanceKindId(instance_kind_discr, DefId(crate_id, body_id)),
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub fn func_def_dyn_method<F: FnPtr, TSelf: core::ptr::Thin, Dyn: ?Sized>(
    static_addr: F,
    identifier: u64,
    instance_kind_discr: InstanceKindDiscr,
    crate_id: u32,
    body_id: u32,
) -> FuncDef
where
    *const TSelf: CoerceUnsized<*const Dyn>,
    <Dyn as core::ptr::Pointee>::Metadata: 'static,
{
    FuncDef {
        static_addr: static_addr.addr(),
        as_dyn_method: Some((
            {
                // NOTE: UB happens if the following assumption does not hold.
                if const { !is_dyn::<Dyn>() } {
                    loop {}
                }

                unsafe {
                    let ptr = transmute::<usize, *const TSelf>(0) as *const Dyn;
                    let metadata = intrinsics::ptr_metadata(ptr);
                    transmute_unchecked(metadata)
                }
            },
            identifier,
        )),
        body_id: InstanceKindId(instance_kind_discr, DefId(crate_id, body_id)),
    }
}

/* Possible receiver types based on: https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility
 * - &Self (i.e. &self)
 * - &mut Self (i.e &mut self)
 * - Box<Self>
 * - Rc<Self>
 * - Arc<Self>
 * - Pin<P> where P is one of the types above
 * As all of these types are a wrapper around a pointer to a possibly DST,
 * a very unsafe transmute should work to obtain the metadata from the receiver type.
 * (all DSTs have the same metadata layout)
 * This might break if the generalization over receiver types happen in the compiler.
 * (You can follow features like `dispatch_from_dyn` and `arbitrary_self_types` in the compiler.)
 */
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[inline]
pub unsafe fn metadata_from_receiver<Pointee: ?Sized, R: Receiver<Target = Pointee>>(
    receiver: &R,
) -> <Pointee as core::ptr::Pointee>::Metadata {
    // The following can cause UB if the receiver does not have the same alignment and size as assumed.
    let ptr = intrinsics::read_via_copy(receiver as *const R as *const *const Pointee);
    intrinsics::ptr_metadata(ptr)
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
const fn is_dyn<T: ?Sized>() -> bool
where
    // This is not necessarily true, but works for instrumentation as lifetimes are erased.
    <T as core::ptr::Pointee>::Metadata: 'static,
{
    /* Based on the documentation, three cases are possible for the metadata of a pointer:
     *- () for sized types
     *- usize for slice types
     *- DynMetadata for dyn types
     */
    const {
        !intrinsics::type_id_eq(
            intrinsics::type_id::<<T as core::ptr::Pointee>::Metadata>(),
            intrinsics::type_id::<()>(),
        ) && !intrinsics::type_id_eq(
            intrinsics::type_id::<<T as core::ptr::Pointee>::Metadata>(),
            intrinsics::type_id::<usize>(),
        )
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub const fn const_binary_op_of(raw: u8) -> BinaryOp {
    BinaryOp::from_raw(raw)
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub const fn const_unary_op_of(raw: u8) -> UnaryOp {
    UnaryOp::from_raw(raw)
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub const fn const_atomic_ord_of(raw: u8) -> AtomicOrdering {
    AtomicOrdering::from_raw(raw)
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub const fn const_atomic_binary_op_of(raw: u8) -> AtomicBinaryOp {
    AtomicBinaryOp::from_raw(raw)
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub const fn const_primitive_type_of(raw: i8) -> PrimitiveType {
    PrimitiveType::from_raw(raw)
}

/* This function is used as a replacement for special functions like intrinsics
 * that cannot be treated as ordinary ones. */
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub fn special_func_placeholder() {}

// FIXME: Clean up
#[cfg(refs_inlining)]
#[inline(always)]
pub const fn ref_place_return_value_encoded() -> PlaceRef {
    r_enc::place::encode_return_value()
}

#[cfg(refs_inlining)]
#[inline(always)]
pub const fn ref_place_argument_encoded(arg_index: u32) -> PlaceRef {
    r_enc::place::encode_argument(arg_index)
}

#[cfg(refs_inlining)]
#[inline(always)]
pub const fn ref_place_local_encoded(arg_index: u32) -> PlaceRef {
    r_enc::place::encode_local(arg_index)
}

#[cfg(refs_inlining)]
#[inline(always)]
pub const fn ref_place_some_encoded() -> PlaceRef {
    r_enc::place::encode_some()
}

#[cfg(refs_inlining)]
#[inline(always)]
pub const fn ref_operand_copy_encoded(place_ref: PlaceRef) -> OperandRef {
    r_enc::operand::encode_place_copy(place_ref)
}

#[cfg(refs_inlining)]
#[inline(always)]
pub const fn ref_operand_move_encoded(place_ref: PlaceRef) -> OperandRef {
    r_enc::operand::encode_place_move(place_ref)
}

#[cfg(refs_inlining)]
#[inline(always)]
pub const fn ref_operand_const_zst_encoded() -> OperandRef {
    r_enc::operand::encode_const_zst()
}

#[cfg(refs_inlining)]
#[inline(always)]
pub const fn ref_operand_const_bool_encoded(value: bool) -> OperandRef {
    r_enc::operand::encode_const_bool(value)
}

#[cfg(refs_inlining)]
#[inline(always)]
pub const fn ref_operand_const_some_encoded() -> OperandRef {
    r_enc::operand::encode_const_some()
}

#[cfg(refs_inlining)]
#[inline(always)]
pub const fn ref_operand_some_encoded() -> OperandRef {
    r_enc::operand::encode_some()
}

/*
#[cfg(not(refs_inlining))]
#[inline(always)]
pub fn ref_place_return_value_encoded() -> PlaceRef {
    super::ref_place_return_value()
}

#[cfg(not(refs_inlining))]
#[inline(always)]
pub fn ref_place_argument_encoded(arg_index: u32) -> PlaceRef {
    super::ref_place_argument(arg_index)
}

#[cfg(not(refs_inlining))]
#[inline(always)]
pub fn ref_place_local_encoded(local_index: u32) -> PlaceRef {
    super::ref_place_local(local_index)
}

#[cfg(not(refs_inlining))]
#[inline(always)]
pub fn ref_place_some_encoded() -> PlaceRef {
    super::ref_place_some()
}

#[cfg(not(refs_inlining))]
#[inline(always)]
pub fn ref_operand_copy_encoded(place_ref: PlaceRef) -> OperandRef {
    super::ref_operand_copy(place_ref)
}

#[cfg(not(refs_inlining))]
#[inline(always)]
pub fn ref_operand_move_encoded(place_ref: PlaceRef) -> OperandRef {
    super::ref_operand_move(place_ref)
}

#[cfg(not(refs_inlining))]
#[inline(always)]
pub fn ref_operand_const_zst_encoded() -> OperandRef {
    super::ref_operand_const_zst()
}

#[cfg(not(refs_inlining))]
#[inline(always)]
pub fn ref_operand_const_bool_encoded(value: bool) -> OperandRef {
    super::ref_operand_const_bool(value)
}

#[cfg(not(refs_inlining))]
#[inline(always)]
pub fn ref_operand_const_some_encoded() -> OperandRef {
    super::ref_operand_const_some()
}

#[cfg(not(refs_inlining))]
#[inline(always)]
pub fn ref_operand_some_encoded() -> OperandRef {
    super::ref_operand_some()
}
*/
