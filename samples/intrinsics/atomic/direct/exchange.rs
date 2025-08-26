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
                        AtomicOrdering::AcqRel,
                        AtomicOrdering::Acquire,
                        AtomicOrdering::Relaxed,
                        AtomicOrdering::Release,
                        AtomicOrdering::SeqCst,
                    },
                );
            )*
        };
        ($op:ident, { $($ordering:expr),* $(,)? } $(,)?) => {
            $(
                let mut a = 20u16.mark_symbolic();
                let original = a;
                let ptr = &mut a as *mut u16;
                let val = 30u16;
                let previous = unsafe { $op::<_, { $ordering }>(ptr, val) };
                let current = unsafe { *ptr };

                // Concrete condition
                if current == 30u16 && current == a {
                    core::hint::black_box(0);
                } else {
                    panic!();
                }
                // Symbolic condition
                if previous == original  {
                    core::hint::black_box(0);
                }

                let new_val = 40u16.mark_symbolic();
                let previous = unsafe { $op::<_, { $ordering }>(ptr, new_val) };
                let current = unsafe { *ptr };

                // Symbolic condition
                if (current == 50u16) & (a + 10 == 60u16) {
                    core::hint::black_box(0);
                }

                // Concrete condition
                if previous == val {
                    core::hint::black_box(0);
                } else {
                    panic!();
                }
            )*
        };
    }

    call_all_and_test!(atomic_xchg);
}
