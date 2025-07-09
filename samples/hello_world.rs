fn main() {
    let x: u8 = core::hint::black_box(10);
    #[cfg(leafc)]
    let x: u8 = {
        use leaf::annotations::*;
        x.mark_symbolic()
    };

    if x < 5 {
        println!("Hello, world!");
    }
}
