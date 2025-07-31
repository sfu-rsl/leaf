#![feature(core_intrinsics)]

use core::intrinsics::*;

use leaf::annotations::Symbolizable;

const N: usize = 3;

fn main() {
    sym_content();

    sym_count();
    sym_src_ptr();
    sym_dst_ptr();
}

fn sym_count() {
    let src = [1u8, 2, 3];
    let mut dst = [0u8, 0, 0];
    let count = 1u8.mark_symbolic() as usize;

    let src_ptr = &src as *const u8;
    let dst_ptr = &mut dst as *mut u8;

    copy(src_ptr, dst_ptr, count);

    assert_eq!(dst[0], src[0]);
}

fn sym_src_ptr() {
    let src = [1u8, 2, 3];
    let mut dst = [0u8, 0, 0];
    let count = 1;

    let i = 0u8.mark_symbolic() as usize;
    let src_ptr = src[i..].as_ptr();
    let dst_ptr = &mut dst as *mut u8;

    copy(src_ptr, dst_ptr, count);

    assert_eq!(dst[0], src[0]);
}

fn sym_dst_ptr() {
    let src = [1u8, 2, 3];
    let mut dst = [0u8, 0, 0];
    let count = 1;

    let i = 0u8.mark_symbolic() as usize;
    let src_ptr = &src as *const u8;
    let dst_ptr = dst[i..].as_mut_ptr();

    copy(src_ptr, dst_ptr, count);

    assert_eq!(dst[0], src[0]);
}

fn sym_content() {
    #[derive(Clone)]
    struct Item(u8, u32);

    let src: [Item; N] = [
        Item(10.mark_symbolic(), 1000),
        Item(20.mark_symbolic(), 2000),
        Item(30.mark_symbolic(), 3000),
    ];

    let mut dst: [Item; N] = [Item(0, 0), Item(0, 0), Item(0, 0)];

    let count = src.len();
    let src_ptr = &src as *const Item;
    let dst_ptr = &mut dst as *mut Item;

    macro_rules! call_all_and_test {
        ($($op:expr),*$(,)?) => {
            $(
                unsafe { core::hint::black_box($op(src_ptr, dst_ptr, count)) };
            )*
        };
    }

    macro_rules! call_all_and_test_vol {
        ($($op:expr),*$(,)?) => {
            $(
                unsafe { core::hint::black_box($op(dst_ptr, src_ptr, count)) };
            )*
        };
    }

    call_all_and_test!(core::intrinsics::copy, copy_nonoverlapping,);
    call_all_and_test_vol!(volatile_copy_nonoverlapping_memory, volatile_copy_memory,);

    if dst[0].0 + dst[1].0 == dst[2].0 {
        core::hint::black_box(foo());
    }
}

fn foo() {}

fn copy<T>(src_ptr: *const T, dst_ptr: *mut T, count: usize) {
    core::hint::black_box(unsafe { core::intrinsics::copy(src_ptr, dst_ptr, count) });
}
