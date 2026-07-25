fn main() {
    let foo = Foo { x: 1 };
    let bar = &foo as &dyn Bar;
    test(bar);
}

trait Bar {
    fn bar(&self) -> i32;
}

struct Foo {
    x: i32,
}

impl Bar for Foo {
    fn bar(&self) -> i32 {
        self.x
    }
}

fn test(bar: &dyn Bar) -> i32 {
    bar.bar()
}
