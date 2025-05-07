use core::{
    intrinsics::{self, transmute, transmute_unchecked},
    marker::FnPtr,
    ops::{CoerceUnsized, Deref},
};

use super::common::{
    self,
    pri::*,
    types::{BasicBlockLocation, DefId},
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
static _SET_PLACE_ADDR_TYPED_REFERENCER: fn(PlaceRef, *const u32) -> () =
    set_place_address_typed::<u32>;

#[used]
static _TYPE_ID_OF_REFERENCER: fn() -> TypeId = type_id_of::<u32>;

#[used]
static _SIZE_OF_REFERENCER: fn() -> TypeSize = size_of::<u32>;

#[used]
static _BASIC_BLOCK_LOCATION_REFERENCER: fn(u32, u32, BasicBlockIndex) -> BasicBlockLocation =
    basic_block_location;

#[used]
static _SWITCH_INFO_REFERENCER: fn(BasicBlockLocation, OperandRef) -> SwitchInfo = switch_info;

#[used]
static _CONST_BINARY_OP_OF_REFERENCER: fn(u8) -> BinaryOp = const_binary_op_of;

#[used]
static _CONST_UNARY_OP_OF_REFERENCER: fn(u8) -> UnaryOp = const_unary_op_of;

#[used]
static _CONST_ATOMIC_ORD_OF_REFERENCER: fn(u8) -> AtomicOrdering = const_atomic_ord_of;

#[used]
static _CONST_ATOMIC_BINARY_OP_OF_REFERENCER: fn(u8) -> AtomicBinaryOp = const_atomic_binary_op_of;

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub fn set_place_address_typed<T>(place: PlaceRef, address: *const T) {
    super::set_place_address(place, address as RawAddress)
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(
    core_build,
    rustc_const_unstable(feature = "const_eval_select", issue = "124625")
)]
#[inline(always)]
pub const fn type_id_of<T: ?Sized + 'static>() -> TypeId {
    /* NOTE: Once this function is const in stable build, we can mark this
     * function as constant as well. */
    /* NOTE: Do we need to bother about inlining?
     * Based on the last checks, LLVM is smart enough to inline this function
     * automatically and even replace everything with u128. */
    fn rt<T: ?Sized + 'static>() -> TypeId {
        super::run_rec_guarded::<true, _>(
            /* If we are recursing, the value doesn't matter (although unsafe) */
            unsafe { intrinsics::transmute([0xFFu8; intrinsics::size_of::<TypeId>()]) },
            || common::utils::type_id_of::<T>(),
        )
    }
    intrinsics::const_eval_select((), common::utils::type_id_of::<T>, rt::<T>)
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub const fn size_of<T>() -> TypeSize {
    intrinsics::size_of::<T>() as TypeSize
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub const fn basic_block_location(
    crate_id: u32,
    body_id: u32,
    index: BasicBlockIndex,
) -> BasicBlockLocation {
    BasicBlockLocation {
        body: DefId(crate_id, body_id),
        index,
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub const fn switch_info(
    node_location: BasicBlockLocation,
    discriminant: OperandRef,
) -> SwitchInfo {
    SwitchInfo {
        node_location,
        discriminant,
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub const fn assertion_info(
    location: BasicBlockLocation,
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
pub fn callee_def_static<F: FnPtr>(func_addr: F) -> CalleeDef {
    CalleeDef {
        static_addr: func_addr.addr(),
        as_virtual: None,
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub fn callee_def_maybe_virtual<F: FnPtr, R: ?Sized>(
    func_addr: F,
    receiver_ptr: *const R,
    identifier: u64,
) -> CalleeDef {
    CalleeDef {
        static_addr: func_addr.addr(),
        as_virtual: {
            if is_ptr_of_dyn(receiver_ptr) {
                let metadata = intrinsics::ptr_metadata(receiver_ptr);
                Some((
                    unsafe { intrinsics::transmute_unchecked(metadata) },
                    identifier,
                ))
            } else {
                None
            }
        },
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub fn func_def_static<F: FnPtr>(addr: F, crate_id: u32, body_id: u32) -> FuncDef {
    FuncDef {
        static_addr: addr.addr(),
        as_dyn_method: None,
        def_id: DefId(crate_id, body_id),
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub fn func_def_dyn_method<F: FnPtr, T: ?Sized, Dyn: ?Sized>(
    static_addr: F,
    receiver_ptr: *const T,
    identifier: u64,
    crate_id: u32,
    body_id: u32,
) -> FuncDef
where
    *const T: CoerceUnsized<*const Dyn>,
{
    FuncDef {
        static_addr: static_addr.addr(),
        as_dyn_method: Some((
            {
                let receiver_ptr = receiver_ptr as *const Dyn;
                let metadata = intrinsics::ptr_metadata(receiver_ptr);
                if !is_ptr_of_dyn(receiver_ptr) {
                    unsafe { intrinsics::unreachable() }
                }
                unsafe { transmute_unchecked(metadata) }
            },
            identifier,
        )),
        def_id: DefId(crate_id, body_id),
    }
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[inline]
pub fn receiver_to_raw_ptr<Pointee: ?Sized, Ptr: Deref<Target = Pointee>>(
    receiver: &Ptr,
) -> *const Pointee {
    // NOTE: This is because of call to Deref (which is not inlined)
    super::run_rec_guarded::<false, _>(
        unsafe {
            const ZEROS: [usize; 2] = [0; 2];
            intrinsics::read_via_copy(&ZEROS as *const _ as *const () as *const *const Pointee)
        },
        || receiver.deref() as *const Pointee,
    )
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[cfg_attr(core_build, rustc_const_stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
const fn is_ptr_of_dyn<T: ?Sized>(_ptr: *const T) -> bool {
    intrinsics::size_of::<<T as core::ptr::Pointee>::Metadata>()
        == intrinsics::size_of::<DynRawMetadata>()
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub fn receiver_pin_to_raw_ptr<Pointee: ?Sized, Ptr: Deref<Target = Pointee>>(
    receiver: &core::pin::Pin<Ptr>,
) -> *const Pointee {
    receiver_to_raw_ptr(receiver)
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[inline(always)]
pub fn receiver_self_to_raw_ptr<Pointee: ?Sized>(receiver_ref: &Pointee) -> *const Pointee {
    receiver_ref as *const Pointee
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

/* This function is used as a replacement for special functions like intrinsics
 * that cannot be treated as ordinary ones. */
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub fn special_func_placeholder() {}
