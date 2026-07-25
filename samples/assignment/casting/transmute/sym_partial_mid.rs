use std::mem::transmute;

use leaf::annotations::Symbolizable;

fn main() {
    let x = 10u32.mark_symbolic();
    let y = read_index(x);
    let z = read_field(x);

    // Unsatisfiable:
    if y != z {
        bar();
    }

    let w = read_mid_reversed(x);
    // Unsatisfiable:
    if w == x {
        bar();
    }
}

fn foo(x: u8) {
    if x == 1 {
        bar();
    }
}

#[inline(never)]
fn bar() {}

struct Splitter {
    head: u8,
    tail: [u8; 3],
}

#[repr(packed(1))]
struct MidSplitter {
    head: u16,
    mid: u32,
    tail: u16,
}

fn read_index(x: u32) -> u8 {
    let array = unsafe { transmute::<u32, [u8; 4]>(x) };
    array[0]
}

fn read_field(x: u32) -> u8 {
    let splitter = unsafe { transmute::<u32, Splitter>(x) };
    splitter.head
}

fn read_mid_reversed(x: u32) -> u32 {
    let mid_split = unsafe { transmute::<[u32; 2], MidSplitter>([x; 2]) };
    mid_split.mid
}
