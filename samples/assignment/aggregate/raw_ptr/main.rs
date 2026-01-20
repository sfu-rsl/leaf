#![feature(core_intrinsics)]
fn main() {
    let x = 10;
    let s: &[i32] =
        unsafe { &*core::intrinsics::aggregate_raw_ptr::<*const [i32], _, _>(&x as *const _, 1) };
    foo(s);

    // More unsafe
    let ptr = &x as *const i32 as *const () as *const u8;
    let s: &[u8] = unsafe { &*core::intrinsics::aggregate_raw_ptr::<*const [u8], _, _>(ptr, 4) };
    foo(s);
}

fn foo<T>(_x: T) {}
