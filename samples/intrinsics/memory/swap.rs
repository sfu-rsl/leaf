#![feature(core_intrinsics)]

use core::intrinsics;

fn main() {
    sym_content();

    sym_ptr::<true, false>();
    sym_ptr::<false, true>();
    sym_ptr::<true, true>();
}

fn sym_ptr<const F_SYM: bool, const S_SYM: bool>() {
    let mut a = [10, 20, 10u32];
    let mut b = [20, 10, 20u32];

    let i = if F_SYM { get_sym_byte() as usize } else { 1 };
    let j = if S_SYM { get_sym_byte() as usize } else { 1 };

    let a_ptr = &mut a[i] as *mut _;
    let b_ptr = &mut b[j] as *mut _;

    unsafe {
        intrinsics::typed_swap_nonoverlapping(a_ptr, b_ptr);
    }

    if b[0] == b[1] && b[1] == b[2] {
        core::hint::black_box(foo());
    }
}

fn sym_content() {
    let mut a = (get_sym_byte(), 10u32);
    let mut b = (20u8, 11u32);

    let (orig_a, orig_b) = (a, b);

    let a_ptr = &mut a as *mut _;
    let b_ptr = &mut b as *mut _;

    unsafe {
        intrinsics::typed_swap_nonoverlapping(a_ptr, b_ptr);
    }

    assert_eq!(a, orig_b);
    assert_eq!(b, orig_a);
}

fn get_sym_byte() -> u8 {
    let x = 1u8;
    #[cfg(leafc)]
    let x = {
        use leaf::annotations::Symbolizable;
        x.mark_symbolic()
    };
    x
}

fn foo() {}
