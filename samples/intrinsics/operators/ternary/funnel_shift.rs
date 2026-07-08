#![feature(core_intrinsics)]
#![feature(funnel_shifts)]

use core::intrinsics;

use leaf::annotations::*;

fn main() {
    let a = 20u8.mark_symbolic();
    let b = 7u8.mark_symbolic();

    let l = unsafe { intrinsics::unchecked_funnel_shl(a, b, 2) };
    let r = unsafe { intrinsics::unchecked_funnel_shr(a, b, 2) };
    use_num(l);
    use_num(r);

    let num = 0b11001100u8;
    let shift_l = 3.mark_symbolic();
    let shift_r = 2.mark_symbolic();
    let l = unsafe { intrinsics::unchecked_funnel_shl(num, 0b1u8, shift_l) };
    let r = unsafe { intrinsics::unchecked_funnel_shr(num, 0b1u8, shift_r) };
    if l == r {
        use_num(0);
    }
}

fn use_num<T: Default + Eq>(x: T) {
    if x.eq(&T::default()) {
        intrinsics::black_box(x);
    }
}
