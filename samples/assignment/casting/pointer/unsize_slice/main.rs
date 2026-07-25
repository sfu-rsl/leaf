fn main() {
    let a: [u8; 3] = [1, 2, 3];
    foo(&a);

    let b = b"123";
    foo(b);
}

fn foo(a: &[u8]) -> u8 {
    a[0]
}
