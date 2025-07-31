fn main() {
    let a = get_foo();
    let b = get_bar();
    let c = Baz { a, b };

    if c.a.x + c.a.y < c.b.0 {
        core::hint::black_box(foo());
    }
}

fn get_x() -> u8 {
    let x = 5u8;
    #[cfg(leafc)]
    let x: u8 = {
        use leaf::annotations::*;
        x.mark_symbolic()
    };
    x
}

fn get_foo() -> Foo {
    let x = get_x();
    Foo { x, y: x + 2 }
}

fn get_bar() -> Bar {
    Bar(get_x(), 20)
}

struct Foo {
    x: u8,
    y: u8,
}

// The fields may be reordered.
struct Bar(u8, u32);

// The fields may be reordered.
struct Baz {
    a: Foo,
    b: Bar,
}

fn foo() {}
