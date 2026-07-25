fn main() {
    let mut a = String::from("Hello");
    unsafe { core::ptr::drop_in_place(&mut a) };
    core::mem::forget(a);
}
