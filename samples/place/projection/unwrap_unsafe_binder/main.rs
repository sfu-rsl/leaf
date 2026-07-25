#![feature(unsafe_binders)]

use core::unsafe_binder::{unwrap_binder, wrap_binder};

fn main() {
    unsafe {
        let x = 1;
        let binder: unsafe<'a> &'a i32 = wrap_binder!(&x);
        let binder = core::hint::black_box(binder);
        let rx = *unwrap_binder!(binder);
        core::hint::black_box(rx);
    }
}
