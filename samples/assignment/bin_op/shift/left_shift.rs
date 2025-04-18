fn main() {
    let u1 = 0b1u16 << 15u16; // == 2^15
    let u2 = 0b10u16 << 15u16; // == 0
    let u3 = 0b100u16 << 15u16; // == 0

    let x1 = i16::MAX << 1i16; // == -2 (340282366920938463463374607431768211454 as u128 bit_rep)

    let a1 = 1i16 << 10i16; // == 1024
    let a2 = 1i16 << 14i16; // == 16384
    let a3 = 1i16 << 15i16; // == -32768

    let d1 = 0b10i16 << 15i16; // == 0
    let d2 = 0b11i16 << 15i16; // == -32768 (340282366920938463463374607431768178688 as u128 bit_rep)
    let d3 = 0b100i16 << 15i16; // == 0

    let e1 = 0b1i128 << 127i128; // == 2^127
    let e2 = 0b10i128 << 127i128; // == 0
    let e3 = 0b100i128 << 127i128; // == 0
}
