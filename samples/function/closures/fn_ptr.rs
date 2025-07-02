use leaf::annotations::Symbolizable;

fn main() {
    let x = 10.mark_symbolic();
    let f = |a: i32, b: i32, c: i32| -> i32 { a + b + c };
    let g: fn(i32, i32, i32) -> i32 = f;
    if g(x, 20, 30) > 10 {
        let f2 = || foo();
        let g2: fn() = f2;
        g2();
    }
}

fn foo() {}
