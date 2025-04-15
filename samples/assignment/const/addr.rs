unsafe extern "Rust" {
    static X: u8;
}

fn main() {
    core::hint::black_box(foo(unsafe { &X }));
}

#[inline(never)]
fn foo<T>(ptr: *const T) {}
