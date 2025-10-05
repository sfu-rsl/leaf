fn main() {
    let x = [0u8; core::mem::size_of::<Test>()];
    #[cfg(leafc)]
    let x = {
        use leaf::annotations::*;
        let mut arr = [0u8; core::mem::size_of::<Test>()];
        for i in 0..arr.len() {
            arr[i] = x[i].mark_symbolic();
        }
        arr
    };

    let x = core::hint::black_box(x);
    let t: Test = unsafe { core::mem::transmute(x) };
    core::hint::black_box(t);
}

enum Test {
    A,
    B(i32),
}
