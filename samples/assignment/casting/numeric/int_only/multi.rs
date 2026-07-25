use leaf::annotations::Symbolizable;

#[allow(unused_variables)]

fn main() {
    let a = get_bool().mark_symbolic();
    let b = a as u8;
    foo(b);
    let c = b as char;
    foo(c);
    let d = c as u32;
    foo(d);
    let e = d as i64;
    if e == 0 {
        foo(e);
    }
}

fn get_bool() -> bool {
    true
}

#[inline(never)]
fn foo<T>(x: T) {}
