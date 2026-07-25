fn main() {
    foo(());
    foo(UNIT_BLOCK);
    foo(NoFieldZst);
    foo(NoFieldZst2 {});
    foo(ZstFields {
        unit: (),
        array: [],
    });
    foo(MaybeZST::<0> { x: [] });
}

const UNIT_BLOCK: () = { () };

struct NoFieldZst;

struct NoFieldZst2 {}

struct ZstFields {
    unit: (),
    array: [u8; 0],
}

struct MaybeZST<const N: usize> {
    x: [u8; N],
}

fn foo<T>(_: T) {}
