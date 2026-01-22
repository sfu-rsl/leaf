use leaf::annotations::Symbolizable;

fn main() {
    let x = 1u128.mark_symbolic();
    let y = unsafe { core::mem::transmute::<u128, &[u8]>(x) };
    if y.len() > 1 {
        foo();
    }
}

fn foo() {}
