use leaf::annotations::Symbolizable;

fn main() {
    let array: [&[u8]; 2] = [&[16, 32], &[48]];
    let i = 0.mark_symbolic();
    let j = 0.mark_symbolic();
    if array[i][j] == 48 {
        foo();
    }
}

fn foo() {}
