fn main() {
    let a = foo;
    let b = a as *const ();
    bar(b);
}

fn foo() {}

fn bar<T>(x: T) {}
