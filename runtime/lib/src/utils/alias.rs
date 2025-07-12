pub(crate) type RRef<T> = std::rc::Rc<std::cell::RefCell<T>>;

macro_rules! check_sym_value_loss {
    () => {
        cfg!(any(
            // Always enabled in debug builds
            debug_assertions,
            feature = "release_sym_value_loss_checks"
        ))
    };
}
pub(crate) use check_sym_value_loss;
