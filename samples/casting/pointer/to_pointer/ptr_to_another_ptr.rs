fn main() {
    let x = 10;
    let y = &x as *const i32;
    let z = y as *const u32;
    foo(z);

    let a: &[u8] = &[1, 2, 3];
    let b: *const [u8] = a as *const [u8];
    let c: *const str = b as *const str;
    foo(c);
    let d: *const [i32] = b as *const [i32];
    foo(d);
}

#[inline(never)]
fn foo<T>(x: T) {}
