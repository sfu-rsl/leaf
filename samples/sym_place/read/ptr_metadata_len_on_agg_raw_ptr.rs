#![feature(core_intrinsics)]

use leaf::annotations::Symbolizable;

fn main() {
    let array = [1, 2, 3, 4];
    let addr = core::ptr::addr_of!(array) as usize;
    let x = 1.mark_symbolic();
    let len = x;
    let subslice: &[i32] = unsafe {
        &*core::intrinsics::aggregate_raw_ptr::<*const [i32], _, _>(addr as *const i32, len)
    };
    foo(subslice[0]);
}

fn foo<T>(_x: T) {}
