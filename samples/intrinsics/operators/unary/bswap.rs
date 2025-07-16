#![feature(core_intrinsics)]

use core::intrinsics;

use leaf::annotations::Symbolizable;

fn main() {
    let a = 0x1A2B3C4Du32.mark_symbolic();
    let b = unsafe { core::intrinsics::bswap(a) };
    if b == 0x4D3C2B1A {
        use_num(0);
    }
}

fn use_num<T: Default + Eq>(x: T) {
    if x.eq(&T::default()) {
        intrinsics::black_box(x);
    }
}
