#![feature(fn_traits)]
use leaf::annotations::Symbolizable;

fn main() {
    let x = 10.mark_symbolic();
    let f = |a: i32, b: i32| -> i32 { a + b };
    let y = call_fn_through_call_once(f, (x, 20));
    test_num(y);
    let mut z = 0;
    let g = |a: i32, b: i32| -> i32 {
        z = a - b;
        z + 1
    };
    let y = call_fn_mut_through_call_once(g, (x, 20));
    test_num(y);
}

fn test_num(x: i32) {
    if x == 71 {
        foo();
    }
}

fn foo() {}

fn call_fn_through_call_once<I1, I2, O, F: Fn(I1, I2) -> O>(f: F, args: (I1, I2)) -> O {
    <F as FnOnce<(I1, I2)>>::call_once(f, args)
}

fn call_fn_mut_through_call_once<I1, I2, O, F: FnMut(I1, I2) -> O>(f: F, args: (I1, I2)) -> O {
    <F as FnOnce<(I1, I2)>>::call_once(f, args)
}
