use crate::pri::TypeId;

#[cfg_attr(not(core_build), macro_export)]
macro_rules! identity {
    ($($input:tt)+) => {
        $($input)+
    };
}
#[cfg(not(core_build))]
pub use identity;
#[cfg(core_build)]
pub(crate) use identity;

#[inline(always)]
pub const fn type_id_of<T: ?Sized + 'static>() -> TypeId {
    // NOTE: Constant evaluation of `Option` is not complete and inlining does not work.
    unsafe { TypeId::new_unchecked(core::intrinsics::type_id::<T>()) }
}

#[cfg(feature = "std")]
/// Searches all ancestor directories of the current working directory
/// (including itself) for a file or directory with the given name.
/// If found, returns the path to the file or directory.
pub fn search_current_ancestor_dirs_for(name: &str) -> Option<std::path::PathBuf> {
    [
        std::env::current_dir().as_ref(),
        std::env::current_exe().as_ref(),
    ]
    .iter()
    .filter_map(|p| p.ok())
    .flat_map(|p| p.ancestors())
    .find(|p| {
        p.read_dir().is_ok_and(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_str().is_some_and(|n| n.starts_with(name)))
        })
    })
    .map(|p| p.join(name))
}

#[cfg(feature = "std")]
pub fn try_join_path(
    path: impl AsRef<std::path::Path>,
    child: impl AsRef<std::path::Path>,
) -> Option<std::path::PathBuf> {
    let path = path.as_ref().join(child);
    if path.exists() { Some(path) } else { None }
}
