#[allow(unused_variables)]
fn main() {
    let x = foo();
    let y = x as u8;
}

#[allow(dead_code)]
#[repr(i8)]
enum Foo {
    First = -5,
    Second = -6,
}

fn foo() -> Foo {
    Foo::First
}
