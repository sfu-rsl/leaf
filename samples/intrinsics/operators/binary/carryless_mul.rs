#![feature(core_intrinsics)]
#![feature(uint_carryless_mul)]

use core::intrinsics;

use leaf::annotations::*;

fn main() {
    let a = 0b00100100u8.mark_symbolic();
    let b = intrinsics::carryless_mul(a, u8::MAX) & !a;

    if (b == 0b00011110u8) & (a.count_ones() == 2) {
        intrinsics::black_box(0);
    }
}
