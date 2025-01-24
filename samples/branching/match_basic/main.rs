use leaf::annotations::*;

fn main() {
    let x = 10i32.mark_symbolic();

    match x % 3 {
        2 => foo(),
        1 => foo(),
        _ => foo(),
    }

    match x - 100 {
        -2 => foo(),
        1 => foo(),
        _ => foo(),
    }

    let y = 'b'.mark_symbolic();
    match y {
        'a' => foo(),
        'x' => foo(),
        _ => foo(),
    }
}

fn foo() {}
