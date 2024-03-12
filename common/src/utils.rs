#[macro_export]
macro_rules! identity {
    ($($input:tt)+) => {
        $($input)+
    };
}
pub use identity;

#[inline(always)]
pub fn type_id_of<T: ?Sized + 'static>() -> u128 {
    core::intrinsics::type_id::<T>()
}

#[cfg(feature = "std")]
/// Searches all ancestor directories of the current working directory
/// (including itself) for a file or directory with the given name.
/// If found, returns the path to the file or directory.
/// otherwise, returns the given name.
pub fn search_current_ancestor_dirs_for(name: &str) -> std::string::String {
    use std::string::ToString;
    std::env::current_dir()
        .unwrap()
        .ancestors()
        .find(|p| {
            p.read_dir().is_ok_and(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .any(|e| e.file_name().to_str().is_some_and(|n| n.starts_with(name)))
            })
        })
        .map(|p| p.join(name).to_string_lossy().to_string())
        .unwrap_or(name.to_string())
}
