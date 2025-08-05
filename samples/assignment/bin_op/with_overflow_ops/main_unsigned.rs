use leaf::annotations::Symbolizable;

fn main() {
    let x = 20_u8.mark_symbolic();
    let x = x as usize;
    let y = x.overflowing_add(5);
    if y.1 & (y.0 > 2) {
        foo(y.0);
    }
    let y = x.overflowing_sub(5);
    if y.1 & (y.0 < (u8::MAX - 3).into()) {
        foo(y.0);
    }
    let y = x.overflowing_mul(5);
    if y.1 & (y.0 > 100) {
        foo(y.0);
    }
}

#[inline(never)]
fn foo<T>(x: T) {
    core::hint::black_box(x);
}
