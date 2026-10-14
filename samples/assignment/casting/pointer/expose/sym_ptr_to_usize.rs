use leaf::annotations::Symbolizable;

fn main() {
    let x = 1.mark_symbolic();
    let a = 20;
    let b = 30;
    let arr = [&a as *const i32, &b as *const i32];
    let y = arr[x];
    do_cast(y);

    let arr_start_ptr = arr.as_ptr();
    let y = unsafe { arr_start_ptr.offset(x as isize) };
    do_cast(y);
}

fn do_cast<T>(x: *const T) -> usize {
    core::hint::black_box(x) as usize
}
