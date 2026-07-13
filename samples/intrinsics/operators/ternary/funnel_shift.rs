#![feature(core_intrinsics)]
#![feature(funnel_shifts)]

use core::intrinsics;

use leaf::annotations::*;

fn main() {
    let a = 20u8.mark_symbolic();
    let b = 7u8.mark_symbolic();

    let l = unsafe { intrinsics::unchecked_funnel_shl(a, b, 2) };
    let r = unsafe { intrinsics::unchecked_funnel_shr(a, b, 2) };
    if (a.count_ones() == 1) & (b.count_ones() == 1) & (l == r) {
        use_num(0);
    }

    let num = 0b11000011u8;
    let shift_l = 3.mark_symbolic();
    let shift_r = 2.mark_symbolic();
    let l = unsafe { intrinsics::unchecked_funnel_shl(num, 0b11u8, shift_l) };
    let r = unsafe { intrinsics::unchecked_funnel_shr(num, 0b11u8, shift_r) };
    if (shift_l < 8) & (shift_r < 8) & (l == r) {
        use_num(0);
    }
}

fn use_num<T: Default + Eq>(x: T) {
    if x.eq(&T::default()) {
        intrinsics::black_box(x);
    }
}
