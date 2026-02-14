use common::pri::*;

use crate::abs;

static mut IS_ACTIVE: bool = false;

type NoOpPri = super::NoOpPri;

#[derive(Default)]
pub struct LateInitPri<P> {
    _phantom: core::marker::PhantomData<P>,
}

macro_rules! late_init {
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
            if core::intrinsics::likely(unsafe { IS_ACTIVE }) {
                MainPri::$name($($arg.into()),*).into()
            } else {
                NoOpPri::$name($($arg.into()),*).into()
            }
        }
    };
}

mod instantiations {
    use super::*;
    /* NOTE: Making the implementation generic is desired but not currently possible
     * due to some limitations in Rust like higher-order generic types.
     * Particularly, Slice and its generic parameter T cause issues.
     * Providing an implementation per type is easier at the moment.
     */

    macro_rules! impl_pri_for_late_init_pri_of {
        ($t:ident) => {
            paste::paste! {
                #[allow(non_snake_case)]
                mod [<_for_ $t>] {
                    use super::*;
                    type MainPri = $t;

                    impl ProgramRuntimeInterface for LateInitPri<MainPri> {
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
                        type DebugInfo = DebugInfo;
                        type Tag = Tag;

                        common::pri::list_func_decls! { modifier: late_init, (from Self) }
                    }
                }
            }
        };
    }

    use crate::BasicPri;
    impl_pri_for_late_init_pri_of!(BasicPri);

    use crate::CftPri;
    impl_pri_for_late_init_pri_of!(CftPri);
}
