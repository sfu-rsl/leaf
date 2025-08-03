/// Defines an enum with two variants, one for each of the two possible orders
/// of two items of possibly different types.
/// In other words, it keeps the order information as variant index.
/// The memory layout for variants are the same.
macro_rules! define_reversible_pair {
    (
        $(#[$($attr: meta)*])*
        $name: ident $(<$($generic_type: ident),*>)? {
            ($orig: ident, $rev: ident) {
                $first: ident : $t_first: ty,
                $second: ident : $t_second: ty $(,)?
            }
        } $(,)?
        $($(#[$($impl_attr:meta)*])* impl)?
    ) => {
        $(#[$($attr)*])*
        pub(crate) enum $name$(<$($generic_type),*>)? {
            $orig {
                $first: $t_first,
                $second: $t_second,
            },
            $rev {
                $first: $t_second,
                $second: $t_first,
            },
        }

        $($(#[$($impl_attr)*])*)?
        impl$(<$($generic_type),*>)? $name$(<$($generic_type),*>)? {
            #[inline]
            pub fn flatten(self) -> ($t_first, $t_second, bool) {
                match self {
                    Self::$orig { $first, $second } => ($first, $second, false),
                    Self::$rev { $second, $first } => ($second, $first, true),
                }
            }

            #[inline]
            pub fn as_flat(&self) -> (&$t_first, &$t_second, bool) {
                match self {
                    Self::$orig { $first, $second } => ($first, $second, false),
                    Self::$rev { $second, $first } => ($second, $first, true),
                }
            }

            #[inline]
            pub fn as_flat_mut(&mut self) -> (&mut $t_first, &mut $t_second, bool) {
                match self {
                    Self::$orig { $first, $second } => ($first, $second, false),
                    Self::$rev { $second, $first } => ($second, $first, true),
                }
            }
        }

        impl$(<$($generic_type),*>)? From<($t_first, $t_second)> for $name$(<$($generic_type),*>)? {
            fn from(pair: ($t_first, $t_second)) -> Self {
                Self::$orig {
                    $first: pair.0,
                    $second: pair.1,
                }
            }
        }

        impl$(<$($generic_type),*>)? From<($t_first, $t_second, bool)> for $name$(<$($generic_type),*>)? {
            fn from(pair: ($t_first, $t_second, bool)) -> Self {
                if !pair.2 {
                    Self::$orig {
                        $first: pair.0,
                        $second: pair.1,
                    }
                } else {
                    Self::$rev {
                        $first: pair.1,
                        $second: pair.0,
                    }
                }
            }
        }

        impl$(<$($generic_type),*>)? core::fmt::Display for $name$(<$($generic_type),*>)?
            where $t_first: core::fmt::Display,
                  $t_second: core::fmt::Display,
        {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                match self {
                    Self::$orig { $first, $second } => write!(f, "({}, {})", $first, $second),
                    Self::$rev { $second, $first } => write!(f, "({}, {})", $second, $first),
                }
            }
        }
    };
}

pub(crate) use define_reversible_pair;

macro_rules! impl_enum_from_enum {
    (
        From<$name:ty> for $src_name:ty {
            $($variant:ident $(+| $eq_variant:ident)*),* $(,)?
        }
    ) => {
        impl From<&$name> for $src_name {
            #[inline]
            fn from(value: &$name) -> Self {
                match value {
                    $(
                        <$name>::$variant { .. } => <$src_name>::$variant
                    ),*
                }
            }
        }

        impl From<$name> for $src_name {
            #[inline]
            fn from(value: $name) -> Self {
                <$src_name>::from(&value)
            }
        }
    }
}
pub(crate) use impl_enum_from_enum;

macro_rules! impl_sub_enum_conversions {
    (
        $name:ident from $src_name:ty {
            $($variant:ident $(+| $eq_variant:ident)*),* $(,)?
        }
    ) => {
        $crate::utils::meta::impl_enum_from_enum! {
            From<$name> for $src_name {
                $($variant $(+| $eq_variant)*),*
            }
        }

        impl TryFrom<$src_name> for $name {
            type Error = $src_name;

            #[inline]
            fn try_from(value: $src_name) -> Result<Self, Self::Error> {
                match value {
                    $(
                        <$src_name>::$variant $(| <$src_name>::$eq_variant)* => Ok($name::$variant)
                    ),*,
                    _ => Err(value)
                }
            }
        }
    };
}
pub(crate) use impl_sub_enum_conversions;

macro_rules! sub_enum {
    (
        #[repr($repr_ty:ty)]
        $(#[$($attr: meta)*])*
        $vis:vis enum $name:ident from $src_name:ty {
            $($variant:ident $(+| $eq_variant:ident)*),* $(,)?
        } using $discr:expr
    ) => {
        #[repr($repr_ty)]
        $(#[$($attr)*])*
        $vis enum $name {
            $(
                $variant = $discr (<$src_name>::$variant)
            ),*
        }

        $crate::utils::meta::impl_sub_enum_conversions! {
            $name from $src_name {
                $($variant $(+| $eq_variant)*),*
            }
        }
    };

    (
        #[repr($repr_ty:ty)]
        $(#[$($attr: meta)*])*
        $vis:vis enum $name:ident from $src_name:ty {
            $($variant:ident $(+| $eq_variant:ident)*),* $(,)?
        }
    ) => {
        $crate::utils::meta::sub_enum! {
            #[repr($repr_ty)]
            $(#[$($attr)*])*
            $vis enum $name from $src_name {
                $($variant $(+| $eq_variant)*),*
            } using $name::super_discriminant
        }

        impl $name {
            const fn super_discriminant(instant: $src_name) -> $repr_ty {
                instant as $repr_ty
            }
        }
    };
}
pub(crate) use sub_enum;

macro_rules! impl_aug_enum_conversions {
    (
        $name:ident from $src_name:ty {
            $($variant:ident $(+| $eq_variant:ident)*),+ $(,)?
            +
            $($aug_variant:ident $(($($field_ty:ty),*))?),+ $(,)?
        }
        with | $err_closure_var:ident | -> $err_ty:ty { $err_fn:expr }
    ) => {
        $crate::utils::meta::impl_enum_from_enum! {
            From<$name> for $src_name {
                $($variant $(+| $eq_variant)*),+
                ,
                $($aug_variant),+
            }
        }

        impl TryFrom<$src_name> for $name {
            type Error = $err_ty;

            #[inline]
            fn try_from(value: $src_name) -> Result<Self, Self::Error> {
                match value {
                    $(
                        <$src_name>::$variant $(| <$src_name>::$eq_variant)* => Ok($name::$variant)
                    ),+,
                    $(<$src_name>::$aug_variant)|+ => {
                        Err(| $err_closure_var | -> $err_ty {
                            $err_fn
                        }(value))
                    }
                }
            }
        }
    };
}
pub(crate) use impl_aug_enum_conversions;

macro_rules! aug_enum {
    (
        #[repr($repr_ty:ty)]
        $(#[$($attr: meta)*])*
        $vis:vis enum $name:ident from $src_name:ty {
            $($variant:ident $(+| $eq_variant:ident)*),+ $(,)?
            +
            $($aug_variant:ident $(($($field_ty:ty),*))?),+ $(,)?
        }
        using $discr:expr,
        with | $err_closure_var:ident | -> $err_ty:ty { $err_fn:expr }
    ) => {
        #[repr($repr_ty)]
        $(#[$($attr)*])*
        $vis enum $name {
            $(
                $variant = $discr (<$src_name>::$variant)
            ),+
            ,
            $(
                $aug_variant $(($($field_ty),*))? = $discr (<$src_name>::$aug_variant)
            ),+
        }

        $crate::utils::meta::impl_aug_enum_conversions! {
            $name from $src_name {
                $($variant $(+| $eq_variant)*),+
                +
                $($aug_variant $(($($field_ty),*))?),+
            }
            with | $err_closure_var | -> $err_ty { $err_fn }
        }

        impl $name {
            $vis const fn discriminant(self) -> $repr_ty {
                match self {
                    $(
                        $name::$variant => $discr (<$src_name>::$variant)
                    ),+
                    ,
                    $(
                        $name::$aug_variant { .. } => $discr (<$src_name>::$aug_variant),
                    ),+
                }
            }
        }
    };

    (
        #[repr($repr_ty:ty)]
        $(#[$($attr: meta)*])*
        $vis:vis enum $name:ident from $src_name:ty {
            $($variant:ident $(+| $eq_variant:ident)*),* $(,)?
            +
            $($aug_variant:ident $(($($field_ty:ty),*))?),* $(,)?
        }
        with | $err_closure_var:ident | -> $err_ty:ty { $err_fn:expr }
    ) => {
        $crate::utils::meta::aug_enum! {
            #[repr($repr_ty)]
            $(#[$($attr)*])*
            $vis enum $name from $src_name {
                $($variant $(+| $eq_variant)*),*
                +
                $($aug_variant $(($($field_ty),*))?),*
            }
            using $name::super_discriminant,
            with | $err_closure_var | -> $err_ty { $err_fn }
        }

        impl $name {
            const fn super_discriminant(instant: $src_name) -> $repr_ty {
                instant as $repr_ty
            }
        }
    };
}
pub(crate) use aug_enum;

macro_rules! impl_super_enum_conversions {
    (
        $name:ident from $src_name:ty {
            $($variant:ident),+ $(,)?
            +
            $($extra_variant:ident $(($($field_ty:ty),*))?),+ $(,)?
        }
    ) => {
        $crate::utils::meta::impl_enum_from_enum! {
            From<$src_name> for $name {
                $($variant),+
            }
        }

        impl<'a> TryFrom<&'a $name> for $src_name {
            type Error = &'a $name;

            #[inline]
            fn try_from(value: &'a $name) -> Result<Self, Self::Error> {
                match value {
                    $(
                        <$name>::$variant => Ok(<$src_name>::$variant)
                    ),+,
                    $(<$name>::$extra_variant)|+ => Err(value),
                }
            }
        }

        impl TryFrom<$name> for $src_name {
            type Error = $name;

            #[inline]
            fn try_from(value: $name) -> Result<Self, Self::Error> {
                match value {
                    $(
                        <$name>::$variant => Ok(<$src_name>::$variant)
                    ),+,
                    $(<$name>::$extra_variant)|+ => Err(value),
                }
            }
        }
    };
}
pub(crate) use impl_super_enum_conversions;

macro_rules! super_enum {
    (
        #[repr($repr_ty:ty)]
        $(#[$($attr: meta)*])*
        $vis:vis enum $name:ident from $src_name:ty {
            $($variant:ident),* $(,)?
            +
            $($extra_variant:ident $(($($field_ty:ty),*))?),+ $(,)?
        }
        using $discr:expr
    ) => {
        #[repr($repr_ty)]
        $(#[$($attr)*])*
        $vis enum $name {
            $(
                $variant = $discr (<$src_name>::$variant)
            ),*
            ,
            $(
                $extra_variant $(($($field_ty),*))?
            ),+
        }

        $crate::utils::meta::impl_super_enum_conversions! {
            $name from $src_name {
                $($variant),*
                +
                $($extra_variant $(($($field_ty),*))?),+
            }
        }
    };
    (
        #[repr($repr_ty:ty)]
        $(#[$($attr: meta)*])*
        $vis:vis enum $name:ident from $src_name:ty {
            $($variant:ident),* $(,)?
            +
            $($extra_variant:ident $(($($field_ty:ty),*))?),+ $(,)?
        }
    ) => {
        $crate::utils::meta::super_enum! {
            #[repr($repr_ty)]
            $(#[$($attr)*])*
            $vis enum $name from $src_name {
                $($variant),*
                +
                $($extra_variant $(($($field_ty),*))?),+
            } using $name::super_discriminant
        }

        impl $name {
            const fn super_discriminant(instant: $src_name) -> $repr_ty {
                instant as $repr_ty
            }
        }
    }
}
pub(crate) use super_enum;
