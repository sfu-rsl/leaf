#![feature(core_intrinsics)]

use core::intrinsics;

use leaf::annotations::*;

fn main() {
    let a = 20u8.mark_symbolic();
    let b = 7u8.mark_symbolic();
    let cond = a > b + 2;

    let c = unsafe { intrinsics::select_unpredictable(cond, a, b) };
    use_num(c);
}

fn use_num<T: Default + Eq>(x: T) {
    if x.eq(&T::default()) {
        intrinsics::black_box(x);
    }
}
