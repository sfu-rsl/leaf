#![feature(core_intrinsics)]

use core::intrinsics;

use leaf::annotations::Symbolizable;

fn main() {
    let a = 20u8.mark_symbolic();
    let b = unsafe { core::intrinsics::exact_div(a, 2) };
    if b == 5 {
        use_num(0);
    }
}

fn use_num<T: Default + Eq>(x: T) {
    if x.eq(&T::default()) {
        intrinsics::black_box(x);
    }
}
