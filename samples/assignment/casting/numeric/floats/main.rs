#![allow(unused_variables)]

fn main() {
    let a = get_int();
    let b = a as f32; // IntToFloat

    let d = get_float();
    let e = d as f32; // FloatToFloat
    let f = d as u32; // FloatToInt
}

fn get_int() -> isize {
    10
}

fn get_float() -> f64 {
    10.0
}
