#![feature(core_intrinsics)]

use leafrtsh::annotations::Symbolizable;

fn main() {
    let x = false.mark_symbolic();
    test([x; 4]);

    let x = 0u8.mark_symbolic();
    test([x; 4]);

    let x = 0x1110u16.mark_symbolic();
    test(Pack(x as u8, x, 0x01));
}

#[repr(packed(1))]
struct Pack(u8, u16, u8);

fn test<T>(x: T) {
    let num = as_u32(x);
    if num == 0x01010101 {
        bar();
    }
}

fn as_u32<T>(x: T) -> u32 {
    unsafe { core::intrinsics::transmute_unchecked(x) }
}

fn bar() {}
