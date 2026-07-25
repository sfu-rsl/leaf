#![feature(inline_const)]

const BOOL_TEST: bool = true;
const INT_TEST: i32 = 10;
const FLOAT_TEST: f32 = 10.33;
const STR_TEST: &str = "Hello, world!";
const BYTE_STR_TEST: &[u8; 13] = b"Hello, world!";
const BYTE_STR_UNSIZED_TEST: &[u8] = b"Hello, world!";
const CHAR_TEST: char = 'a';
const BYTE_CHAR_TEST: u8 = b'a';

const TUPLE_TEST: (i32, i32) = (10, 20);
const ARRAY_TEST: [i32; 3] = [1, 2, 3];
const SLICE_TEST: &[i32] = &[1, 2, 3];

struct TestStruct {
    a: i32,
}

enum TestEnum {
    A(i32),
    B,
}

union TestUnion {
    a: i32,
    b: f32,
}

const STRUCT_TEST: TestStruct = TestStruct { a: 10 };
const ENUM_TEST: TestEnum = TestEnum::A(10);

const REF_TEST: &i32 = &2;

trait TestTrait {
    fn foo(&self) -> i32;
}

impl TestTrait for TestStruct {
    fn foo(&self) -> i32 {
        self.a
    }
}

const TRAIT_TEST: &dyn TestTrait = &TestStruct { a: 10 };

const UNION_TEST: TestUnion = TestUnion { a: 10 };
const CLOSURE_TEST: fn(i32) -> i32 = |x| x + 1;
const RAW_PTR_TEST: *const i32 = &2 as *const i32;
const FN_PTR_TEST: fn() -> () = foo;

fn main() {
    use_item(BOOL_TEST);
    use_item(INT_TEST);
    use_item(FLOAT_TEST);
    use_item(STR_TEST);
    use_item(BYTE_STR_TEST);
    use_item(BYTE_STR_UNSIZED_TEST);
    use_item(CHAR_TEST);
    use_item(BYTE_CHAR_TEST);

    use_item(TUPLE_TEST);
    use_item(ARRAY_TEST);
    use_item(SLICE_TEST);

    use_item(STRUCT_TEST);
    use_item(ENUM_TEST);

    use_item(REF_TEST);

    use_item(TRAIT_TEST);

    const LOCAL_CONST: [i32; 3] = ARRAY_TEST;
    use_item(LOCAL_CONST);

    let inline_const: i32 = const { 10 + 2 };
    use_item(inline_const);

    let const_param = bar::<20>();
    use_item(const_param);

    use_item(UNION_TEST);
    use_item(CLOSURE_TEST);
    use_item(RAW_PTR_TEST);
    use_item(FN_PTR_TEST);
}

fn use_item<T>(x: T) -> () {
    foo()
}

fn foo() {}

#[inline(never)]
fn bar<const N: usize>() -> i32 {
    let a = [1; N];
    a[0]
}
