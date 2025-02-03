use leaf::annotations::*;

fn main() {
    let x: u8 = 10.mark_symbolic();

    // No SMT solving should happen
    if calc(x) < 5 {
        println!("Hello, world!");
    }
}

#[leaf_attr::instrument(false)]
fn calc(num: u8) -> u8 {
    num + 5
}
