#![feature(core_intrinsics)]

use core::intrinsics;

fn main() {
    sym_content();

    sym_ptr();
    sym_count();
}

fn sym_content() {
    let mut arr = [(get_sym_byte(), 3u32); 5];

    let value = get_sym_byte() + 13;

    let ptr = &mut arr as *mut _;

    unsafe {
        intrinsics::write_bytes(ptr, value, arr.len() - 1);
    }

    if arr[0].0 * 3 == arr[1].0 + arr[2].0 {
        core::hint::black_box(foo());
    }

    assert_eq!(arr[1].1, u32::from_ne_bytes([value; 4]));
}

fn sym_ptr() {
    let mut arr = [(2u8, 3u32); 5];

    let i = get_sym_byte() as usize;
    let ptr = &mut arr[i] as *mut _;

    unsafe {
        intrinsics::write_bytes(ptr, 0, 1);
    }

    if arr[0].0 == 0 {
        core::hint::black_box(foo());
    }
}

fn sym_count() {
    let mut arr = [(2u8, 3u32); 5];
    let ptr = &mut arr as *mut _;

    let count = get_sym_byte() as usize;

    unsafe {
        intrinsics::write_bytes(ptr, 0, count);
    }

    if arr[2].0 == 0 && arr[3].0 == 0 && arr[4].0 == 0 {
        core::hint::black_box(foo());
    }
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
