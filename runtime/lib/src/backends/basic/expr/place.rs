use std::rc::Rc;

use derive_more as dm;

use common::types::PointerOffset;

use crate::backends::basic::place::PlaceMetadata;

use super::prelude::*;

pub(crate) type PlaceValueRef = Rc<PlaceValue>;
pub(crate) type DeterPlaceValueRef = guards::DeterPlaceValueGuard<PlaceValueRef>;
pub(crate) type SymPlaceValueRef = guards::SymPlaceValueGuard<PlaceValueRef>;

#[derive(Clone, Debug, PartialEq, Eq, dm::From)]
pub(crate) enum PlaceValue {
    #[from]
    Deterministic(DeterministicPlaceValue),
    #[from(forward)]
    Symbolic(SymbolicPlaceValue),
}

impl PlaceValue {
    #[inline]
    pub(crate) fn is_symbolic(&self) -> bool {
        matches!(self, PlaceValue::Symbolic(..))
    }

    #[inline]
    pub(crate) fn type_info(&self) -> &LazyTypeInfo {
        match self {
            PlaceValue::Deterministic(value) => value.type_info(),
            PlaceValue::Symbolic(value) => value.type_info(),
        }
    }

    #[inline]
    pub(crate) fn type_id(&self) -> TypeId {
        self.type_info().id().unwrap()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeterministicPlaceValue {
    addr: RawAddress,
    ty_info: LazyTypeInfo,
}

impl DeterministicPlaceValue {
    pub(crate) fn new(metadata: &PlaceMetadata) -> Self {
        Self::from_addr_type_info(metadata.address(), LazyTypeInfo::from(metadata))
    }

    pub(crate) fn from_addr_type(addr: RawAddress, ty: TypeId) -> Self {
        Self::from_addr_type_info(addr, LazyTypeInfo::from(ty))
    }

    pub(crate) fn from_addr_type_info(addr: RawAddress, ty_info: LazyTypeInfo) -> Self {
        Self {
            addr,
            ty_info: ty_info,
        }
    }

    #[inline]
    pub(crate) fn address(&self) -> RawAddress {
        self.addr
    }

    #[inline]
    pub(crate) fn type_info(&self) -> &LazyTypeInfo {
        &self.ty_info
    }

    #[inline]
    pub(crate) fn type_id(&self) -> TypeId {
        self.ty_info.id().unwrap()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SymbolicPlaceValue {
    pub base: SymbolicPlaceBase,
    pub proj: Option<DeterministicProjection>,
    ty_info: LazyTypeInfo,
}

impl SymbolicPlaceValue {
    pub(crate) fn from_base<B: Into<SymbolicPlaceBase>>(base: B, ty_info: LazyTypeInfo) -> Self {
        Self {
            base: base.into(),
            proj: None,
            ty_info,
        }
    }

    #[inline]
    pub(crate) fn type_info(&self) -> &LazyTypeInfo {
        &self.ty_info
    }

    #[inline]
    pub(crate) fn type_id(&self) -> TypeId {
        self.ty_info.id().unwrap()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, dm::From)]
pub(crate) enum SymbolicPlaceBase {
    Deref(DerefSymHostPlace),
    SymIndex(SymIndexedPlace),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeterministicProjection {
    pub offset: PointerOffset,
    pub ty_id: TypeId,
}

impl DeterministicProjection {
    pub(crate) fn on_deter(&self, base: &DeterministicPlaceValue) -> DeterministicPlaceValue {
        DeterministicPlaceValue::from_addr_type(
            base.address().wrapping_byte_add(self.offset as usize),
            self.ty_id,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, dm::From)]
pub(crate) struct DerefSymHostPlace {
    pub host: SymValueRef,
    pub host_type_id: TypeId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SymIndexedPlace {
    pub host: PlaceValueRef,
    pub index: SymValueRef,
}

mod guards {
    use super::super::define_guard;
    use super::*;

    macro_rules! define_place_value_guard {
        ($guarded_type:ty, $name: ident, $pattern:pat, $value_name:ident) => {
            define_guard!(
                PlaceValue,
                PlaceValueRef,
                $guarded_type,
                $name,
                $pattern,
                $value_name
            );

            impl<V> core::ops::Deref for $name<V>
            where
                V: AsRef<PlaceValue>,
            {
                type Target = $guarded_type;

                fn deref(&self) -> &Self::Target {
                    self.$value_name()
                }
            }

            impl<V> AsRef<$guarded_type> for $name<V>
            where
                Self: core::ops::Deref<Target = $guarded_type>,
            {
                fn as_ref(&self) -> &$guarded_type {
                    self
                }
            }
        };
    }

    define_place_value_guard!(
        DeterministicPlaceValue,
        DeterPlaceValueGuard,
        PlaceValue::Deterministic(place),
        place
    );

    define_place_value_guard!(
        SymbolicPlaceValue,
        SymPlaceValueGuard,
        PlaceValue::Symbolic(place),
        place
    );
}

mod convert {
    use super::*;

    impl PlaceValue {
        #[inline]
        pub fn to_value_ref(self) -> PlaceValueRef {
            Rc::new(self)
        }
    }

    macro_rules! impl_to_value_ref {
        ($($ty: ty),*) => {
            $(
                impl $ty {
                    #[inline]
                    pub(crate) fn to_value_ref(self) -> PlaceValueRef {
                        Into::<PlaceValue>::into(self).to_value_ref()
                    }
                }
            )*
        };
    }

    impl_to_value_ref!(DeterministicPlaceValue, SymbolicPlaceValue);

    impl<'a> From<&'a PlaceMetadata> for LazyTypeInfo {
        fn from(metadata: &'a PlaceMetadata) -> Self {
            LazyTypeInfo::from((metadata.type_id(), metadata.ty().copied()))
        }
    }

    impl DeterministicPlaceValue {
        pub(crate) fn to_raw_value(&self) -> RawConcreteValue {
            RawConcreteValue(self.addr, self.ty_info.clone())
        }
    }

    impl From<RawConcreteValue> for DeterministicPlaceValue {
        fn from(value: RawConcreteValue) -> Self {
            Self {
                addr: value.0,
                ty_info: value.1,
            }
        }
    }
}
