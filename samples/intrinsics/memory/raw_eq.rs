#![feature(core_intrinsics)]

use core::intrinsics;

use leafrtsh::annotations::*;

fn main() {
    let a = [0xdeadbeefu64.mark_symbolic(), 7u64.mark_symbolic(), 0u64];
    let b = [0xfeedfaceu64.mark_symbolic(), 2u64.mark_symbolic(), 0u64];

    let eq = unsafe { intrinsics::raw_eq(&a, &b) };
    if eq {
        intrinsics::black_box(0u8);
    }
}
