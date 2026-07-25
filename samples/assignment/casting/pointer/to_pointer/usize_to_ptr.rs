fn main() {
    let x = 10;
    let y = &x as *const i32;
    let z = y as usize;
    let a = z as *const i32;
    foo(a);
}

#[inline(never)]
fn foo<T>(x: T) {}
