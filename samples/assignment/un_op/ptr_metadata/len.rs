fn main() {
    let a = [1, 2, 3];

    match a[..] {
        [x, y] => { /* this arm will not apply because the length doesn't match */ }
        _ => {}
    };
}
