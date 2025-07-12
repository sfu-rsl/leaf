fn main() {
    let x = 10;
    let y = &x as *const i32;
    let z = y as *const u32;
    foo(z);

    let a: &[u8] = &[1, 2, 3];
    let x = 1;
    #[cfg(leafc)]
    use leaf::annotations::*;
    #[cfg(leafc)]
    let x = 1.mark_symbolic();
    let a = &a[x..x + 1];

    let b: *const [u8] = a as *const [u8];
    let c: *const str = b as *const str;
    foo(c);

    let d: *const [i32] = b as *const [i32];
    foo(d);

    let e: *const () = b as *const ();
    foo(e);
}

#[inline(never)]
fn foo<T>(x: T) {}
