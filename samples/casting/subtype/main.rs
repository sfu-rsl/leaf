fn useful(_: &()) {}

fn main() {
    let x: fn(&()) = useful;
    let y = test(&x);
}

fn test<'a>(x: &'a fn(&())) -> &fn(&'a ()) {
    x as &fn(&'a ())
}
