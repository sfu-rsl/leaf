fn main() {
    let mut a = 10;
    let b: *mut i32 = &mut a as *mut i32;
    let c = b as *const i32;
    foo(c);
}

#[inline(never)]
fn foo<T>(_: T) {}
