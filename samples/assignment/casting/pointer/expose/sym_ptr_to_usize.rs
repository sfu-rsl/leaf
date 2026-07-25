use leaf::annotations::Symbolizable;

fn main() {
    let x = 1.mark_symbolic();
    let a = 20;
    let b = 30;
    let arr = [&a as *const i32, &b as *const i32];
    let y = arr[x];
    let z = y as usize;
    foo(z);
}

#[inline(never)]
fn foo<T>(x: T) {}
