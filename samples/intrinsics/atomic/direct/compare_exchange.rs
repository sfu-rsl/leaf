#![feature(core_intrinsics)]

use core::intrinsics::*;

use leaf::annotations::Symbolizable;

fn main() {
    macro_rules! call_all_and_test {
        ($($op:ident),* $(,)?) => {
            $(
                call_all_and_test!(
                    $op,
                    {
                        AtomicOrdering::AcqRel,
                        AtomicOrdering::Acquire,
                        AtomicOrdering::Relaxed,
                        AtomicOrdering::Release,
                        AtomicOrdering::SeqCst
                    },
                );
            )*
        };
        ($op:ident, { $($s_ordering:expr),* $(,)? } $(,)?) => {
            $(
                call_all_and_test!(
                    $op,
                    $s_ordering,
                    { AtomicOrdering::Acquire, AtomicOrdering::Relaxed, AtomicOrdering::SeqCst },
                );
            )*
        };
        ($op:ident, $s_ordering:expr, { $($f_ordering:expr),* $(,)?} $(,)?) => {
            $(
                call_all_and_test!($op, $s_ordering, $f_ordering);
            )*
        };
        ($op:ident, $s_ordering:expr, $f_ordering:expr $(,)?) => {
            let mut a = 10u16;
            let val = 20u16.mark_symbolic();
            let ptr = &mut a as *mut u16;
            let (previous, successful) = unsafe { $op::<_, { $s_ordering }, { $f_ordering }>(ptr, 10u16, val) };
            assert!(successful);

            let original = a;
            let val = 30u16;
            let (previous, successful) = unsafe { $op::<_, { $s_ordering }, { $f_ordering }>(ptr, original, val) };
            // Symbolic condition
            assert!(successful);
            if previous + 10 == 40u16 {
                core::hint::black_box(0);
            }

            let new_val = 40u16.mark_symbolic();
            let (previous, successful) = unsafe { $op::<_, { $s_ordering }, { $f_ordering }>(ptr, val, new_val) };
            // Semi-symbolic condition
            assert!(successful);
            if previous + 10 == 40u16 {
                core::hint::black_box(0);
            } else {
                panic!();
            }

            let (previous, successful) = unsafe { $op::<_, { $s_ordering }, { $f_ordering }>(ptr, 50u16, original) };
            // Symbolic condition
            assert!(!successful);
            if previous + 10 == 60u16 {
                panic!();
            }
        };
    }

    call_all_and_test!(atomic_cxchg, atomic_cxchgweak,);
}
