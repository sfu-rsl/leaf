fn main() {
    let a = core::hint::black_box(get_bar_single());
    match a {
        BarSingle::First(..) => unreachable!(),
        BarSingle::Second => foo(1),
    }

    let b = core::hint::black_box(get_bar_multi());
    match b {
        BarMulti::First(..) => unreachable!(),
        BarMulti::Second => foo(1),
        BarMulti::Third => foo(2),
    }
}

#[inline(never)]
fn foo<T>(x: T) {
    core::hint::black_box(())
}

enum Void {}

enum BarSingle {
    First(Void),
    Second,
}

fn get_bar_single() -> BarSingle {
    BarSingle::Second
}

enum BarMulti {
    First(Void),
    Second,
    Third,
}

fn get_bar_multi() -> BarMulti {
    BarMulti::Third
}
