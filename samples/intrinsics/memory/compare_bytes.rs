#![feature(core_intrinsics)]

use core::intrinsics;

use leaf::annotations::Symbolizable;

const N: usize = 3;

fn main() {
    sym_content();

    sym_count();
    sym_left_ptr();
    sym_right_ptr();
}

fn sym_count() {
    let left = [1u8, 2, 3];
    let mut right = [0u8, 0, 0];
    let count = 1u8.mark_symbolic() as usize;

    let left_ptr = &left as *const u8;
    let right_ptr = &mut right as *const u8;

    let result = compare_bytes(left_ptr, right_ptr, count);
    use_result(result);
}

fn sym_left_ptr() {
    let left = [1u8, 2, 3];
    let mut right = [0u8, 0, 0];
    let count = N - 1;

    let i = 1u8.mark_symbolic() as usize;
    let left_ptr = left[i..].as_ptr();
    let right_ptr = &mut right as *const u8;

    let result = compare_bytes(left_ptr, right_ptr, count);

    use_result(result);
}

fn sym_right_ptr() {
    let left = [1u8, 2, 3];
    let mut right = [0u8, 0, 0];
    let count = N - 1;

    let i = 1u8.mark_symbolic() as usize;
    let left_ptr = &left as *const u8;
    let right_ptr = right[i..].as_mut_ptr();

    let result = compare_bytes(left_ptr, right_ptr, count);
    use_result(result);
}

fn sym_content() {
    let left: [u8; N] = [
        10u8.mark_symbolic(),
        20u8.mark_symbolic(),
        30u8.mark_symbolic(),
    ];

    let mut right: [u8; N] = [10u8, 20, 30];

    let count = left.len();
    let left_ptr = &left as *const u8;
    let right_ptr = &mut right as *const u8;

    let result = compare_bytes(left_ptr, right_ptr, count);
    use_result(result);
}

fn compare_bytes(left_ptr: *const u8, right_ptr: *const u8, count: usize) -> i32 {
    core::hint::black_box(unsafe { intrinsics::compare_bytes(left_ptr, right_ptr, count) })
}

fn use_result(result: i32) {
    if result > 0 {
        core::hint::black_box(0u8);
    }
}
