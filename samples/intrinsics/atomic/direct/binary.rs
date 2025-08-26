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
                let mut a = 20u8.mark_symbolic();
                let original = a;
                let ptr = &mut a as *mut u8;
                let val = 0b00001010u8;
                let check = 0b00001000u8;
                let previous = unsafe { $op::<_, _, { $ordering }>(ptr, val) };
                let current = unsafe { *ptr };
                if current == check {
                    core::hint::black_box(0);
                }
                if previous == original {
                    core::hint::black_box(0);
                }
            )*
        };

    }
    call_all_and_test!(
        atomic_and,
        // atomic_max,
        // atomic_min,
        // atomic_nand,
        atomic_or,
        // atomic_umax,
        // atomic_umin,
        atomic_xadd,
        atomic_xor,
        atomic_xsub,
    );
}
