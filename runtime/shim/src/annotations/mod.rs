#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub trait Symbolizable: Sized {
    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    fn symbolize(&self);

    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    #[leaf_attr::instrument(false)]
    fn mark_symbolic(self) -> Self {
        Self::symbolize(&self);
        self
    }
}

#[leaf_attr::instrument(false)]
mod implementation {
    use core::mem::size_of;

    use super::super::pri::{
        compiler_helpers::{f32_to_bits, f64_to_bits},
        *,
    };
    use super::*;

    macro_rules! impl_symbolizable_direct {
        ($($ty:ident),*) => {
            $(
                #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
                impl Symbolizable for $ty {
                    fn symbolize(&self) {
                        let operand_ref = ${concat(new_sym_value_, $ty)}(*self);
                        override_return_value(operand_ref);
                    }
                }
            )*
        };
    }

    impl_symbolizable_direct!(bool, char);

    macro_rules! impl_symbolizable_int {
        ($($ty:ty),*, $signed:literal) => {
            $(
                #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
                impl Symbolizable for $ty {
                    fn symbolize(&self) {
                        let operand_ref = new_sym_value_int(
                            *self as u128,
                            size_of::<$ty>() as u64 * 8,
                            $signed
                        );
                        override_return_value(operand_ref);
                    }
                }
            )*
        };
    }

    impl_symbolizable_int!(i8, i16, i32, i64, i128, isize, true);
    impl_symbolizable_int!(u8, u16, u32, u64, u128, usize, false);

    macro_rules! impl_symbolizable_float {
        ($($ty:ident),*) => {
            $(
                #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
                impl Symbolizable for $ty {
                    fn symbolize(&self) {
                        let bit_rep = ${concat($ty, _to_bits)}(*self);
                        let sbits = <$ty>::MANTISSA_DIGITS as u64;
                        let ebits = (<$ty>::MAX_EXP - <$ty>::MIN_EXP + 1) as u64;
                        let operand_ref = new_sym_value_float(bit_rep, ebits, sbits);
                        override_return_value(operand_ref);
                    }
                }
            )*
        };
    }

    impl_symbolizable_float!(f32, f64);
}

#[leaf_attr::instrument(false)]
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub fn push_tag(tag: &'static str) {
    super::pri::push_tag(tag);
}

#[leaf_attr::instrument(false)]
#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub fn pop_tag() {
    super::pri::pop_tag();
}

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub mod tags {
    #[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
    pub use super::super::common::pri::tags::*;
}
