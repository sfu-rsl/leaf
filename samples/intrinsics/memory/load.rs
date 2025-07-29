#![feature(core_intrinsics)]

use core::intrinsics::*;

use leaf::annotations::Symbolizable;

fn main() {
    macro_rules! call_all_and_test {
        ($($op:expr),*$(,)?) => {
            $(
                let mut a = 24u8.mark_symbolic();
                let ptr = &mut a as *const u8;
                let b = unsafe { $op(ptr) };
                if b == 6 {
                    core::hint::black_box(0);
                }
            )*
        };
    }

    call_all_and_test!(volatile_load, unaligned_volatile_load,);
}
