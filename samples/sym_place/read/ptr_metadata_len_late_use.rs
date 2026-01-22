fn main() {
    let arr = vec![vec![1, 2], vec![3]];
    let x = 1;
    let l = get_len(arr, x);
    if l > 1 {
        foo();
    }
}

fn get_len(arr: Vec<Vec<u8>>, index: usize) -> usize {
    arr[index].as_slice().len()
}

#[inline(never)]
fn foo() {}
