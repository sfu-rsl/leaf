#![feature(core_intrinsics)]

use core::intrinsics;

use leaf::annotations::*;

fn main() {
    let a = 20u8.mark_symbolic();
    let b = 7u8.mark_symbolic();
    let c = 2u8.mark_symbolic();
    let d = 5u8.mark_symbolic();

    let (e, f) = intrinsics::carrying_mul_add(a, b, c, d);
    use_num(e);
    use_num(f);
}

fn use_num<T: Default + Eq>(x: T) {
    if x.eq(&T::default()) {
        intrinsics::black_box(x);
    }
}
