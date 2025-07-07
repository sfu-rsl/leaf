#![feature(core_intrinsics)]

use core::intrinsics;

use leaf::annotations::Symbolizable;

fn main() {
    let a = 10u8.mark_symbolic();
    let b = intrinsics::black_box(a);
    if b == 5 {
        core::hint::black_box(foo());
    }
}

#[inline(never)]
fn foo() {}
