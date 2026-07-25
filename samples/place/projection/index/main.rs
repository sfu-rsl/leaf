use leaf::annotations::Symbolizable;

fn main() {
    let a = [1, 2, 3.mark_symbolic()];
    if a[0] + a[1] + a[2] == 10 {
        foo();
    }
}

fn foo() {}
