use leaf::annotations::Symbolizable;

#[allow(unused_variables)]

fn main() {
    let a = get_int().mark_symbolic();
    let b = a as u32;
    if a < -6 {
        foo(b);
    }

    let b = 0.mark_symbolic();
    if b == i128::MIN {
        foo(b);
    }

    let c = get_uint().mark_symbolic();
    let d = c as i64;
    if c < 24 {
        foo(d);
    }

    let e = get_uint().mark_symbolic();
    let f = e as u8;
    if e < 15 {
        foo(f);
    }

    let i = get_bool().mark_symbolic();
    let j = i as u8;
    if i {
        foo(j);
    }

    let k = get_char().mark_symbolic();
    let l = k as u64;
    if k < 'a' {
        foo(l);
    }

    // this is considered a char cast in the runtime (non-integer)
    let m = get_u8().mark_symbolic();
    let n = m as char;
    if m < 66 {
        foo(n);
    }
}

fn get_int() -> isize {
    -10
}

fn get_uint() -> usize {
    1000
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

#[inline(never)]
fn foo<T>(x: T) {}
