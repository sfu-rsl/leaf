#![allow(unused_variables)]

const A: i32 = 20;

fn main() {
    let b = foo(true);

    let si = foo(A);

    let si = foo(-10_i8);
    let si = foo(-10_i16);
    let si = foo(-10_i32);
    let si = foo(-10_i64);
    let si = foo(-10_i128);

    let ui = foo(10_u8);
    let ui = foo(10_u16);
    let ui = foo(10_u32);
    let ui = foo(10_u64);
    let ui = foo(10_u128);

    let f = foo(10.33_f32);
    let f = foo(10.33_f64);

    let c = foo('a');

    let s = foo("Hello, world!");

    let b = foo(b"Hello, world!");
}

fn foo<T>(_: T) {}
