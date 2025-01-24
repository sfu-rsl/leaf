use leaf::annotations::*;

fn main() {
    let x = 10u8.mark_symbolic();

    match x % 5 {
        1..=3 => foo(),
        4 => foo(),
        _ => foo(),
    }
}

fn foo() {}
