use common::{ffi::*, pri::*};

// Call order: C ABI -> ExternPri (+ conversions) -> PriImpl

struct ExternPri;

macro_rules! delegate_to_default {
    ($(#[$($attr: meta)*])* fn $name:ident ($($(#[$($arg_attr: meta)*])* $arg:ident : $arg_type:ty),* $(,)?) $(-> $ret_ty:ty)?;) => {
        $(#[$($attr)*])*
        #[inline(always)]
        fn $name ($($(#[$($arg_attr)*])* $arg : $arg_type),*) $(-> $ret_ty)? {
            PriImpl::$name($($arg.into()),*).into()
        }
    };
}

/* NOTE: Why do we add a layer of indirection and implement this trait?
 * Refer to the comment in the common library.
 * Basically, making sure that no function is missed in the exported interface.
 */

impl ProgramRuntimeInterface for ExternPri {
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

    common::pri::list_func_decls!(modifier: delegate_to_default, (from Self));
}
impl FfiPri for ExternPri {}

macro_rules! export_to_c_abi {
    ($(#[$($attr: meta)*])* fn $name:ident ($($(#[$($arg_attr: meta)*])* $arg:ident : $arg_type:ty),* $(,)?) $(-> $ret_ty:ty)?;) => {
        $(#[$($attr)*])*
        #[no_mangle]
        pub extern "C" fn $name ($($(#[$($arg_attr)*])* $arg : $arg_type),*) $(-> $ret_ty)? {
            ExternPri::$name($($arg),*)
        }
    };
}

common::pri::list_func_decls!(modifier: export_to_c_abi, (from common::ffi));
