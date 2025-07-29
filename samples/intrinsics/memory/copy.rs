#![feature(core_intrinsics)]

use core::intrinsics::*;

use leaf::annotations::Symbolizable;

fn main() {
    macro_rules! call_all_and_test {
        ($($op:expr),*$(,)?) => {
            $(
                let mut a = 6u8;
                let mut b = 24u8;
                let src = &mut a as *mut u8;
                let dst = &mut b as *mut u8;
                let count = 12u8.mark_symbolic() as usize;
                unsafe { $op(src, dst, count) };
                let b = unsafe { *dst };
                if b == 99 {
                    core::hint::black_box(0);
                }
            )*
        };
    }

    call_all_and_test!(
        copy,
        copy_nonoverlapping,
        volatile_copy_nonoverlapping_memory,
        volatile_copy_memory,
    );
}
