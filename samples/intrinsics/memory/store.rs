#![feature(core_intrinsics)]

use core::intrinsics::*;

use leaf::annotations::Symbolizable;

fn main() {
    macro_rules! call_all_and_test {
        ($($op:expr),*$(,)?) => {
            $(
                let mut a = 6u8;
                let ptr = &mut a as *mut u8;
                let value = 24u8.mark_symbolic();
                unsafe { $op(ptr, value) };
                let b = unsafe { *ptr };
                if b == 99 {
                    core::hint::black_box(0);
                }
            )*
        };
    }

    call_all_and_test!(volatile_store, unaligned_volatile_store,);
}
