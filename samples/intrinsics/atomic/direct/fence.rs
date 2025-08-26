#![feature(core_intrinsics)]

use core::intrinsics::*;

fn main() {
    macro_rules! call_all_and_test {
        ($($op:ident),*$(,)?) => {
            $(
                call_all_and_test!(
                    $op,
                    {
                        AtomicOrdering::AcqRel,
                        AtomicOrdering::Acquire,
                        AtomicOrdering::Release,
                        AtomicOrdering::SeqCst,
                    },
                );
            )*
        };
        ($op:ident, { $($ordering:expr),* $(,)? } $(,)?) => {
            $(
                unsafe { $op::<{ $ordering }>(); }
            )*
        };
    }

    call_all_and_test!(atomic_fence, atomic_singlethreadfence,);
}
