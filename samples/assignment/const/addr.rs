#![feature(core_intrinsics)]

use core::intrinsics;

static X: u8 = 10;
static mut Y: u8 = 20;

fn main() {
    unsafe {
        intrinsics::copy_nonoverlapping(&raw const X, &raw mut Y, 1);
    }
}
