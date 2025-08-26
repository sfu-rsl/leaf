#![feature(core_intrinsics)]

use core::intrinsics::*;

use leaf::annotations::Symbolizable;

fn main() {
    macro_rules! call_all_and_test {
        ($($op:ident),*$(,)?) => {
            $(
                call_all_and_test!(
                    $op,
                    {
                        AtomicOrdering::Relaxed,
                        AtomicOrdering::Release,
                        AtomicOrdering::SeqCst,
                    },
                );
            )*
        };
        ($op:ident, { $($ordering:expr),* $(,)? } $(,)?) => {
            $(
                let mut a = 0u16;
                let ptr = &mut a as *mut u16;
                let value = 20u16.mark_symbolic();
                unsafe { $op::<_, { $ordering }>(ptr, value) };
                let b = unsafe { *ptr };
                if b == 30 {
                    core::hint::black_box(0);
                }
            )*
        };
    }

    call_all_and_test!(atomic_store);
}
