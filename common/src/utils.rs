use super::types::TypeId;

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
    TypeId::new(core::intrinsics::type_id::<T>()).unwrap()
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

macro_rules! array_backed_struct {
    (
        $(#[$($attr: meta)*])*
        $vis:vis struct $name:ident $(<$($generic:ident $(= $default_ty:ty)?),*>)? {
            $($field:ident),*$(,)?
        }: $ty:ty;
    ) => {
        $(#[$($attr)*])*
        $vis struct $name$(<$($generic $(= $default_ty)?),*>)?(
            [$ty; ${count($field)}]
        ) where $($($generic: Copy),*)?;

        impl$(<$($generic),*>)? $name$(<$($generic),*>)?
        where $($($generic: Copy),*)?
        {
            $(
                #[inline(always)]
                $vis fn $field(&self) -> $ty {
                    self.0[${index()}]
                }
            )*

            $vis fn to_pairs(self) -> [(String, $ty); ${count($field)}] {
                [
                    $(
                        (stringify!($field).to_string(), self.0[${index()}]),
                    )*
                ]
            }
        }

        impl$(<$($generic),*>)? AsRef<[$ty; ${count($field)}]> for $name$(<$($generic),*>)?
        where $($($generic: Copy),*)?
        {
            #[inline(always)]
            fn as_ref(&self) -> &[$ty; ${count($field)}] {
                &self.0
            }
        }

        impl$(<$($generic),*>)? From<[$ty; ${count($field)}]> for $name$(<$($generic),*>)?
        where $($($generic: Copy),*)?
        {
            #[inline(always)]
            fn from(array: [$ty; ${count($field)}]) -> Self {
                $name(array)
            }
        }

        impl$(<$($generic),*>)? Into<[$ty; ${count($field)}]> for $name$(<$($generic),*>)?
        where $($($generic: Copy),*)?
        {
            #[inline(always)]
            fn into(self) -> [$ty; ${count($field)}] {
                self.0
            }
        }

        impl$(<$($generic),*>)? TryFrom<&'_ [(String, $ty)]> for $name$(<$($generic),*>)?
        where $($($generic: Copy),*)?
        {
            type Error = &'static str;
            #[inline(always)]
            fn try_from(array: &'_ [(String, $ty)]) -> Result<Self, Self::Error> {
                Ok($name([
                    $(
                        array.iter().find_map(|(name, value)| {
                            if name == stringify!($field) {
                                Some(*value)
                            } else {
                                None
                            }
                        }).ok_or(stringify!($field))?,
                    )*
                ]))
            }
        }
    };
}
pub(crate) use array_backed_struct;

#[cfg(any(feature = "tyexp", feature = "directed"))]
mod str_err {
    use core::{error::Error, fmt::Display};
    use std::boxed::Box;

    #[derive(Debug)]
    pub struct StrError {
        pub message: &'static str,
        pub cause: Option<Box<dyn Error>>,
    }

    impl StrError {
        pub fn new(message: &'static str, cause: impl Error + 'static) -> Self {
            Self {
                message,
                cause: Some(Box::new(cause)),
            }
        }

        pub fn with_message<E: Error + 'static>(message: &'static str) -> impl FnOnce(E) -> Self {
            move |e| Self::new(message, e)
        }
    }

    impl Error for StrError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.cause.as_ref().and_then(|cause| cause.source())
        }
    }

    impl Display for StrError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.message)?;
            if let Some(cause) = &self.cause {
                write!(f, " Cause: {}", cause)?;
            }
            Ok(())
        }
    }
}
#[cfg(any(feature = "tyexp", feature = "directed"))]
pub(crate) use str_err::StrError;

#[cfg(feature = "std")]
pub fn comma_separated<T: core::fmt::Display>(
    iter: impl Iterator<Item = T>,
) -> std::string::String {
    iter.map(|t| std::format!("{t}"))
        .collect::<std::vec::Vec<_>>()
        .join(", ")
}

#[cfg(feature = "unsafe_wrappers")]
mod unsafe_wrappers {
    use derive_more as dm;

    #[derive(Default, dm::Deref, dm::DerefMut)]
    pub struct UnsafeSync<T>(T);

    impl<T> UnsafeSync<T> {
        pub const fn new(value: T) -> Self {
            Self(value)
        }

        pub fn into_inner(self) -> T {
            self.0
        }
    }

    unsafe impl<T> Sync for UnsafeSync<T> {}

    #[derive(Default, dm::Deref, dm::DerefMut)]
    pub struct UnsafeSend<T>(T);

    impl<T> UnsafeSend<T> {
        pub const fn new(value: T) -> Self {
            Self(value)
        }

        pub fn into_inner(self) -> T {
            self.0
        }
    }

    unsafe impl<T> Send for UnsafeSend<T> {}
}
#[cfg(feature = "unsafe_wrappers")]
pub use unsafe_wrappers::*;

#[cfg(feature = "std")]
pub fn current_instant_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
}
