use leaf::annotations::Symbolizable;

fn main() {
    let array: &[u8] = &[1, 2, 3, 4];
    let i = 1.mark_symbolic();
    let j = 2.mark_symbolic();
    let k = 3.mark_symbolic();
    if i != j && j != k && array[i] + array[j] == array[k] {
        foo();
    }
}

fn foo() {}
