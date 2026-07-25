use leaf::annotations::Symbolizable;

fn main() {
    let a = Bar {
        x: 1.mark_symbolic(),
        y: 2,
    };

    if a.x + a.y == 10 {
        foo();
    }
}

struct Bar {
    x: i32,
    y: i32,
}

fn foo() {}
