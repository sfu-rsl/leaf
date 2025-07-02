#![feature(fn_traits)]
#[cfg(leafc)]
use leaf::annotations::Symbolizable;

fn main() {
    let x = 10;
    #[cfg(leafc)]
    let x = x.mark_symbolic();
    let f = add;
    let y = call_through_fn_once(f, (x, 20));
    test_num(y);
    let z = call_through_fn(f, (y, 20));
    test_num(z);
    let t = call_through_fn_mut(f, (z, 20));
    test_num(t);
    let v = call_void_through_fn_once(foo);
    let v = call_void_through_fn_once(foo as fn() -> ());
}

fn test_num(x: i32) {
    if x == 71 {
        foo();
    }
}

fn foo() {}

fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[inline(never)]
fn call_through_fn_once<F, I1, I2, O>(f: F, args: (I1, I2)) -> O
where
    F: FnOnce(I1, I2) -> O,
{
    f(args.0, args.1)
}

#[inline(never)]
fn call_through_fn<F, I1, I2, O>(f: F, args: (I1, I2)) -> O
where
    F: Fn(I1, I2) -> O,
{
    f(args.0, args.1)
}

#[inline(never)]
fn call_through_fn_mut<F, I1, I2, O>(mut f: F, args: (I1, I2)) -> O
where
    F: FnMut(I1, I2) -> O,
{
    f(args.0, args.1)
}

#[inline(never)]
fn call_void_through_fn_once<F, O>(f: F) -> O
where
    F: FnOnce() -> O,
{
    f()
}
