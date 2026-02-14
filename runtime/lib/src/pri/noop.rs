use common::pri::*;

use crate::abs;

pub struct NoOpPri;

macro_rules! noop {
    ($(#[$($attr: meta)*])* fn $name:ident ($($(#[$($arg_attr: meta)*])* $arg:ident : $arg_type:ty),* $(,)?) $(-> $ret_ty:ty)?;) => {
        $(#[$($attr)*])*
        #[inline(always)]
        #[allow(unused_variables)]
        fn $name ($($(#[$($arg_attr)*])* $arg : $arg_type),*) $(-> $ret_ty)? {
            Default::default()
        }
    };
}

impl ProgramRuntimeInterface for NoOpPri {
    // We try to match the DefaultPri, but it is not necessary to do so if it causes significant overhead.
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

    common::pri::list_func_decls! { modifier: noop, (from Self) }
}
