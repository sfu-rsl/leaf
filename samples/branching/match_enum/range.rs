fn main() {
    let x = core::hint::black_box(10u8);
    #[cfg(leafc)]
    let x: u8 = {
        use leaf::annotations::*;
        x.mark_symbolic()
    };

    match x % 5 {
        1..=2 => core::hint::black_box(()),
        3..=4 => core::hint::black_box(()),
        _ => core::hint::black_box(()),
    }
}
