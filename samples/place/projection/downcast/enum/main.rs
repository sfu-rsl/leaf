use leaf::annotations::Symbolizable;

fn main() {
    let a = Bar::First(1, 2.mark_symbolic());
    let x = match a {
        Bar::First(_, x) => x,
        Bar::Second => 5,
    };

    if x == 10 {
        foo();
    }
}

enum Bar {
    First(i32, i32),
    Second,
}

fn foo() {}
