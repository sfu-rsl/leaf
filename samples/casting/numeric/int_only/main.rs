#![allow(unused_variables)]

fn main() {
    let a = get_int();
    let b = a as u32;

    let c = get_uint();
    let d = c as i64;

    let e = get_uint();
    let f = e as u8;

    // let g = get_foo();
    // let h = g as u16;

    let i = get_bool();
    let j = i as u8;

    let k = get_char();
    let l = k as u64;

    let m = get_u8();
    let n = m as char;
}

fn get_int() -> isize {
    -10
}

fn get_uint() -> usize {
    1000
}

#[allow(dead_code)]
enum Foo {
    Bar,
    Baz,
}

fn get_foo() -> Foo {
    Foo::Bar
}

fn get_bool() -> bool {
    true
}

fn get_char() -> char {
    'a'
}

fn get_u8() -> u8 {
    65 // ASCII 'A'
}
