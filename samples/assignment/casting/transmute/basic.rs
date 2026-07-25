fn main() {
    unsafe {
        let bytes = core::mem::transmute::<u32, [u8; 4]>(0xdeadbeef);
        use_value(bytes);
    }
}

fn use_value<T>(_: T) {}
