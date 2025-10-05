fn main() {
    let x: *const i32 = std::ptr::null();
    #[cfg(leafc)]
    let x: *const i32 = {
        use leaf::annotations::*;
        (x as usize).mark_symbolic() as *const i32
    };
    let x = core::hint::black_box(x);

    core::hint::black_box(unsafe { *x });
}
