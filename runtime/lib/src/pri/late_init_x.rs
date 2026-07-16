#[macro_export]
macro_rules! def_late_init {
    ($(#[$($attr: meta)*])* fn init_runtime_lib ($($(#[$($arg_attr: meta)*])* $arg:ident : $arg_type:ty),* $(,)?) $(-> $ret_ty:ty)?;) => {
        $(#[$($attr)*])*
        #[inline(always)]
        fn init_runtime_lib ($($(#[$($arg_attr)*])* $arg : $arg_type),*) $(-> $ret_ty)? {
            MainPri::init_runtime_lib($($arg.into()),*);
            unsafe { IS_ACTIVE = true; }
        }
    };
    ($(#[$($attr: meta)*])* fn shutdown_runtime_lib ($($(#[$($arg_attr: meta)*])* $arg:ident : $arg_type:ty),* $(,)?) $(-> $ret_ty:ty)?;) => {
        $(#[$($attr)*])*
        #[inline(always)]
        fn shutdown_runtime_lib ($($(#[$($arg_attr)*])* $arg : $arg_type),*) $(-> $ret_ty)? {
            MainPri::shutdown_runtime_lib($($arg.into()),*);
            unsafe { IS_ACTIVE = false; }
        }
    };
    ($(#[$($attr: meta)*])* fn $name:ident ($($(#[$($arg_attr: meta)*])* $arg:ident : $arg_type:ty),* $(,)?) $(-> $ret_ty:ty)?;) => {
        $(#[$($attr)*])*
        #[inline(always)]
        fn $name ($($(#[$($arg_attr)*])* $arg : $arg_type),*) $(-> $ret_ty)? {
            if core::hint::likely(unsafe { IS_ACTIVE }) {
                MainPri::$name($($arg.into()),*).into()
            } else {
                NoOpPri::$name($($arg.into()),*).into()
            }
        }
    };
}

#[macro_export]
macro_rules! make_late_init_pri_of {
    ($t:ident) => {
        paste::paste! {
            #[allow(non_snake_case)]
            mod [<_for_ $t>] {
                use common::pri::*;

                use $crate::{abs, pri::{LateInitPri, NoOpPri}};

                use super::*;

                type MainPri = $t;

                static mut IS_ACTIVE: bool = false;

                #[derive(Default)]
                pub struct [<$t LateInit>] {
                    _phantom: core::marker::PhantomData<MainPri>,
                }

                impl common::pri::ProgramRuntimeInterface for [<$t LateInit>] {
                    type U128 = u128;
                    type Char = char;
                    type ConstStr = &'static str;
                    type ConstByteStr = &'static [u8];
                    type Slice<'a, T: 'a> = &'a [T];
                    type TypeId = abs::TypeId;
                    type PrimitiveType = abs::PrimitiveType;
                    type BinaryOp = abs::BinaryOp;
                    type UnaryOp = abs::UnaryOp;
                    type AtomicOrdering = abs::AtomicOrdering;
                    type AtomicBinaryOp = abs::AtomicBinaryOp;
                    type DebugInfo = common::pri::DebugInfo;
                    type Tag = abs::Tag;

                    common::pri::list_func_decls! { modifier: $crate::def_late_init, (from Self) }
                }
            }
            pub use [<_for_ $t>]::[<$t LateInit>];
        }
    };
}
