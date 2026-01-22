use leaf::annotations::Symbolizable;

fn main() {
    let first: [&[u8]; 2] = [&[16, 32], &[48, 64, 80]];
    let i = 1.mark_symbolic();
    let second: [&[u8]; 2] = [&[96, 112], first[i]];
    let j = 1.mark_symbolic();
    if second[j][2] > 0 {
        foo();
    } else {
        bar();
    }
}

fn foo() {}

fn bar() {}
