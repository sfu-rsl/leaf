#![feature(core_intrinsics)]

use core::intrinsics;

use leaf::annotations::Symbolizable;

fn main() {
    let a = 20u8.mark_symbolic();

    let l = intrinsics::rotate_left(a, 2);
    let r = intrinsics::rotate_right(a, 2);
    use_num(l);
    use_num(r);

    let num = 0b11001100u8;
    let shift_l = 3.mark_symbolic();
    let shift_r = 2.mark_symbolic();
    let l = intrinsics::rotate_left(num, shift_l);
    let r = intrinsics::rotate_right(num, shift_r);
    if l == r {
        use_num(0);
    }
}

fn use_num<T: Default + Eq>(x: T) {
    if x.eq(&T::default()) {
        intrinsics::black_box(x);
    }
}
