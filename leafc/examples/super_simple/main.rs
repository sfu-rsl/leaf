fn get_int() -> i32 {
    6
}

fn main() {
    // This is treated as a symbolic variable, although there's currently no code to run this
    // program multiple times with different values.
    let a = 8;
    let b = 54;
    let c = 2;
    let leaf_symbolic_x = get_int();
    let leaf_symbolic_y = get_int();

    if a * leaf_symbolic_x + b * leaf_symbolic_y == c {
        "ax + by == c is satisfied"
    } else {
        "ax + by == c is not satisfied"
    };
}
