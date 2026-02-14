#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub mod compiler_helpers;

use super::common;

use common::pri::*;

// Call order: Rust ABI -> ForeignPri (+ conversions) -> runtime lib's C ABI

/*
 * This field serves as a marker to find the module in the compiler easier.
 */
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[rustc_diagnostic_item = "leaf_module_marker"]
pub static MODULE_MARKER: u8 = 0;

mod ffi {
    use common::ffi::*;

    use super::*;

    macro_rules! declare_fn {
        ($(#[$($attr: meta)*])* fn $name:ident ($($(#[$($arg_attr: meta)*])* $arg:ident : $arg_type:ty),* $(,)?) $(-> $ret_ty:ty)?;) => {
            $(#[$($attr)*])*
            pub(super) fn $name ($($(#[$($arg_attr)*])* $arg : $arg_type),*) $(-> $ret_ty)?;
        };
    }

    #[link(name = "leafrt")]
    extern "C" {
        common::pri::macros::list_func_decls!(modifier: declare_fn, (from common::ffi));
    }

    macro_rules! delegate_to_leafrt {
        ($(#[$($attr: meta)*])* fn $name:ident ($($(#[$($arg_attr: meta)*])* $arg:ident : $arg_type:ty),* $(,)?) $(-> $ret_ty:ty)?;) => {
            $(#[$($attr)*])*
            #[inline(always)]
            fn $name ($($(#[$($arg_attr)*])* $arg : $arg_type),*) $(-> $ret_ty)? {
                unsafe { self::$name($($arg),*) }
            }
        };
    }

    pub(super) struct ForeignPri;

    impl ProgramRuntimeInterface for ForeignPri {
        type U128 = U128Pack;
        type Char = CharPack;
        type ConstStr = ConstStrPack;
        type ConstByteStr = ConstByteStrPack;
        type Slice<'a, T: 'a> = SlicePack<T>;
        type TypeId = U128Pack<TypeId>;
        type BinaryOp = common::pri::BinaryOp;
        type UnaryOp = common::pri::UnaryOp;
        type AtomicOrdering = common::pri::AtomicOrdering;
        type AtomicBinaryOp = common::pri::AtomicBinaryOp;
        type DebugInfo = common::ffi::DebugInfo;
        type Tag = common::ffi::ConstStrPack;

        common::pri::macros::list_func_decls!(modifier: delegate_to_leafrt, (from Self));
    }

    impl FfiPri for ForeignPri {}
}

/* Making it thread-local as recursions can only happen within the same thread.
 * The motivation is to make optimizations easier, but based on the latest checks,
 * LLVM generates similar instructions in both cases and both optimized. */
#[thread_local]
static mut REC_GUARD: bool = false;

#[inline(always)]
pub(crate) fn run_rec_guarded<const UNLIKELY: bool, T>(default: T, f: impl FnOnce() -> T) -> T {
    let guarded = unsafe { REC_GUARD };
    if guarded {
        if UNLIKELY {
            core::intrinsics::cold_path();
        }
        return default;
    }
    unsafe {
        REC_GUARD = true;
    }
    let result = f();
    unsafe {
        REC_GUARD = false;
    }
    result
}

macro_rules! guarded_body {
    ($name:ident( $($arg:ident),* ) else { $default:expr }) => {
        /* NOTE: This might be an inefficient way of preventing recursions.
         * The recursion is possible as we call `into` function here that is
         * instrumented (note that we are still in the space of the program
         * here and not in the runtime library).
         * Another solution is to make sure that we are not calling any
         * instrumented function statically, but that does not seem to be
         * easy to achieve. So unless we have evidence that this is significantly
         * affecting the performance, we will keep this solution. */

        /* NOTE: Be very careful about the functions you call in the following block as we don't have the guard yet.
         * No function that may possibly be instrumented must not be called.
         * Watch out for intrinsics or inlineable functions that may not be optimized in
         * some build configs like codegen_all_mir=false. */
        if unsafe { REC_GUARD } {
            core::intrinsics::cold_path();
            return const { $default };  // Use const block to ensure no function calls happen in runtime.
        }

        unsafe {
            REC_GUARD = true;
        }
        let result = ffi::ForeignPri::$name($($arg.into()),*).into();
        unsafe {
            REC_GUARD = false;
        }
        result
    };
}

macro_rules! guarded_func {
    ($(#[$($attr: meta)*])* fn $name:ident ($($(#[$($arg_attr: meta)*])* $arg:ident : $arg_type:ty),* $(,)?) $(-> $ret_ty:ty)?; with default: $default:expr) => {
        $(#[$($attr)*])*
        #[inline(always)]
        #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
        pub fn $name ($($(#[$($arg_attr)*])* $arg : $arg_type),*) $(-> $ret_ty)? {
            guarded_body!{ $name($($arg),*) else { $default } }
        }
    };
}

/* NOTE: Default::default may not get inlined in some build configs,
 * thus we explicitly switch case over the 3 cases of return type. */
macro_rules! export_to_rust_abi {
    ($(#[$($attr: meta)*])* fn $name:ident ($($(#[$($arg_attr: meta)*])* $arg:ident : $arg_type:ty),* $(,)?) ;) => {
        guarded_func! {
            $(#[$($attr)*])*
            fn $name($($(#[$($arg_attr)*])* $arg : $arg_type),*);
            with default: ()
        }
    };

    ($(#[$($attr: meta)*])* fn $name:ident ($($(#[$($arg_attr: meta)*])* $arg:ident : $arg_type:ty),* $(,)?) -> PlaceRef;) => {
        guarded_func! {
            $(#[$($attr)*])*
            fn $name($($(#[$($arg_attr)*])* $arg : $arg_type),*) -> PlaceRef;
            with default: 0
        }
    };

    ($(#[$($attr: meta)*])* fn $name:ident ($($(#[$($arg_attr: meta)*])* $arg:ident : $arg_type:ty),* $(,)?) -> OperandRef;) => {
        guarded_func! {
            $(#[$($attr)*])*
            fn $name($($(#[$($arg_attr)*])* $arg : $arg_type),*) -> OperandRef;
            with default: 0
        }
    };
}

macro_rules! slice_of {
    ($t:ty) => {
        &[$t]
    };
}

common::pri::macros::list_func_decls! {
    modifier: export_to_rust_abi,
    (
        u128: u128,
        char: char,
        &str: &'static str,
        &[u8]: &'static [u8],
        slice: slice_of,
        type_id: TypeId,
        binary_op: BinaryOp,
        unary_op: UnaryOp,
        atomic_ord: AtomicOrdering,
        atomic_bin_op: AtomicBinaryOp,
        dbg_info: DebugInfo,
        tag: Tag,
    )
}
