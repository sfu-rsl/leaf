#![feature(dyn_star)]

trait Bar {
    fn bar(&self) -> usize;
}

struct Foo(usize);

impl Bar for Foo {
    fn bar(&self) -> usize {
        self.0
    }
}

impl<T: Bar> Bar for Box<T> {
    fn bar(&self) -> usize {
        Bar::bar(self.as_ref())
    }
}

fn main() {
    let x = Foo(10);
    let x = Box::new(x);
    let bar = x as dyn* Bar;
    foo(bar);
}

fn foo<T>(_: T) {}
