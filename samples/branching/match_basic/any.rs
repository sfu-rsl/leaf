fn main() {
    let x: u8 = core::hint::black_box(10);
    match x {
        10 | 11 => foo(x),
        _ => foo(0),
    }
}

#[inline(never)]
fn foo(_x: u8) {}
