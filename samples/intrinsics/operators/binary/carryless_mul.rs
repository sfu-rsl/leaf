#![feature(core_intrinsics)]
#![feature(uint_carryless_mul)]

use core::intrinsics;

use leaf::annotations::*;

fn main() {
    let a = 20u8.mark_symbolic();
    let b = core::intrinsics::carryless_mul(a, 2);

    if b == 5 {
        use_num(0);
    }
}

fn use_num<T: Default + Eq>(x: T) {
    if x.eq(&T::default()) {
        intrinsics::black_box(x);
    }
}
