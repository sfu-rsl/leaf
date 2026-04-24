use super::types::TypeId;

/// A marker trait for other conversions to prevent mistakenly low-level conversions.
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
unsafe trait Transmute<T> {}
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
unsafe impl<T> Transmute<T> for T {}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[repr(transparent)]
#[derive(Debug)]
pub struct U128Pack<T = u128>(
    [u8; core::mem::size_of::<u128>()],
    core::marker::PhantomData<T>,
);
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
unsafe impl Transmute<u128> for U128Pack {}
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
unsafe impl Transmute<TypeId> for U128Pack<TypeId> {}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[repr(transparent)]
#[derive(Debug)]
pub struct CharPack([u8; core::mem::size_of::<char>()]);

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
unsafe impl Transmute<char> for CharPack {}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[repr(C)]
#[derive(Debug)]
pub struct SlicePack<T> {
    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    pub ptr: *const T,
    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    pub len: usize,
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[repr(C)]
#[derive(Debug)]
pub struct ConstStrPack {
    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    pub ptr: *const u8,
    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    pub len: usize,
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub type ConstByteStrPack = ConstStrPack;

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub type DebugInfo = ConstByteStrPack;

// Functions in this module are carefully examined to ensure they do not call any possibly instrumented functions.
mod no_call_convert {
    use super::*;

    pub trait NoCallFrom<T> {
        fn from(value: T) -> Self;
    }

    impl<T> NoCallFrom<T> for T {
        #[inline(always)]
        fn from(value: T) -> Self {
            value
        }
    }

    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl<T> NoCallFrom<u128> for U128Pack<T> {
        #[inline(always)]
        fn from(value: u128) -> Self {
            Self(
                unsafe {
                    #[allow(unnecessary_transmutes)]
                    core::intrinsics::transmute(value)
                },
                core::marker::PhantomData,
            )
        }
    }

    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl<T> NoCallFrom<TypeId> for U128Pack<T> {
        #[inline(always)]
        fn from(value: TypeId) -> Self {
            NoCallFrom::from(unsafe {
                core::intrinsics::transmute_unchecked::<TypeId, u128>(value)
            })
        }
    }

    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl NoCallFrom<char> for CharPack {
        #[inline(always)]
        fn from(value: char) -> Self {
            Self(unsafe { core::intrinsics::transmute(value) })
        }
    }

    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl<T, U> NoCallFrom<&[T]> for SlicePack<U>
    where
        U: Transmute<T>,
    {
        #[inline(always)]
        fn from(value: &[T]) -> Self {
            Self {
                ptr: value as *const [T] as *const T as *const U,
                len: core::intrinsics::ptr_metadata(value),
            }
        }
    }
    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl NoCallFrom<ConstStrPack> for &'static str {
        #[inline(always)]
        fn from(value: ConstStrPack) -> Self {
            unsafe {
                core::intrinsics::transmute(
                    core::intrinsics::aggregate_raw_ptr::<*const [u8], _, _>(value.ptr, value.len),
                )
            }
        }
    }
    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl NoCallFrom<&'static str> for ConstStrPack {
        #[inline(always)]
        fn from(value: &'static str) -> Self {
            Self {
                ptr: value as *const str as *const u8,
                len: core::intrinsics::ptr_metadata(value),
            }
        }
    }

    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl NoCallFrom<&'static [u8]> for ConstByteStrPack {
        #[inline(always)]
        fn from(value: &'static [u8]) -> Self {
            Self {
                ptr: value as *const [u8] as *const u8,
                len: core::intrinsics::ptr_metadata(value),
            }
        }
    }
}
pub use no_call_convert::NoCallFrom;

// Conversions in this module are not present at instrumentation time, so we do not need to worry about recursions.
#[cfg(feature = "ffi_ty_std_convert")]
mod std_convert_impls {
    use super::*;

    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl<T> From<U128Pack<T>> for u128 {
        #[inline(always)]
        fn from(value: U128Pack<T>) -> Self {
            unsafe { core::mem::transmute(value.0) }
        }
    }
    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl<T> From<u128> for U128Pack<T> {
        #[inline(always)]
        fn from(value: u128) -> Self {
            Self(
                unsafe { core::mem::transmute(value) },
                core::marker::PhantomData,
            )
        }
    }
    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl<T> From<U128Pack<T>> for TypeId {
        #[inline(always)]
        fn from(value: U128Pack<T>) -> Self {
            unsafe { TypeId::new_unchecked(From::from(value)) }
        }
    }
    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl<T> From<TypeId> for U128Pack<T> {
        #[inline(always)]
        fn from(value: TypeId) -> Self {
            From::from(Into::<u128>::into(value))
        }
    }

    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl From<CharPack> for char {
        #[inline(always)]
        fn from(value: CharPack) -> Self {
            unsafe { core::mem::transmute(value.0) }
        }
    }
    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl From<char> for CharPack {
        #[inline(always)]
        fn from(value: char) -> Self {
            Self(unsafe { core::mem::transmute(value) })
        }
    }

    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl<T, U> From<&[T]> for SlicePack<U>
    where
        U: Transmute<T>,
    {
        #[inline(always)]
        fn from(value: &[T]) -> Self {
            Self {
                ptr: value.as_ptr() as *const U,
                len: value.len(),
            }
        }
    }
    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl<T, U> From<SlicePack<U>> for &[T]
    where
        U: Transmute<T>,
    {
        #[inline(always)]
        fn from(value: SlicePack<U>) -> Self {
            unsafe { core::slice::from_raw_parts(value.ptr as *const T, value.len) }
        }
    }

    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl From<ConstStrPack> for &'static str {
        #[inline(always)]
        fn from(value: ConstStrPack) -> Self {
            unsafe {
                core::str::from_utf8_unchecked(core::slice::from_raw_parts(value.ptr, value.len))
            }
        }
    }

    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    impl From<ConstByteStrPack> for &'static [u8] {
        #[inline(always)]
        fn from(value: ConstByteStrPack) -> Self {
            unsafe { core::slice::from_raw_parts(value.ptr, value.len) }
        }
    }
}
