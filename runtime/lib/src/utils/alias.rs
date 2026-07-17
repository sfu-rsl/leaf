pub type RRef<T> = std::rc::Rc<std::cell::RefCell<T>>;

#[macro_export]
macro_rules! check_value_loss {
    () => {
        cfg!(any(
            // Always enabled in debug builds
            debug_assertions,
            feature = "release_value_loss_checks"
        ))
    };
}
pub use check_value_loss;
