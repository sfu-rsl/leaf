use std::collections::HashMap;

use common::tyexp::{CoreTypes, TypeExport, TypesData, pass_core_type_names_to};

use crate::{
    abs::{IntType, backend::TypeManager},
    tyexp,
};

use super::{CoreTypeProvider, LazyTypeInfo, TypeId, TypeInfo, ValueType};

pub(crate) struct BasicTypeManager<'t> {
    all_types: &'t HashMap<TypeId, TypeInfo>,
    core_types: CoreTypes<&'t TypeInfo>,
}

impl<'t> BasicTypeManager<'t> {
    fn new(types: &'t TypesData) -> Self {
        Self {
            all_types: &types.all_types,
            core_types: types.core_types.map(|id| &types.all_types[&id]),
        }
    }
}

impl Default for BasicTypeManager<'static> {
    fn default() -> Self {
        Self::new(tyexp::instance::PROGRAM_TYPES.get_or_init(|| TypeExport::read().unwrap()))
    }
}

impl<'t> TypeManager for BasicTypeManager<'t> {
    type Key = TypeId;
    type Value = &'t TypeInfo;

    fn get_type(&self, key: Self::Key) -> Self::Value {
        self.all_types
            .get(&key)
            .unwrap_or_else(|| panic!("Type information was not found. TypeId: {}", key))
    }
}

macro_rules! impl_int_type {
    ($($name: ident),*$(,)?) => {
        $(
            fn $name(&self) -> ValueType {
                #[allow(unused_comparisons)]
                const IS_SIGNED: bool = $name::MIN < 0;
                IntType {
                    bit_size: $name::BITS as u64,
                    is_signed: IS_SIGNED,
                }.into()
            }
        )*
    };
}

macro_rules! impl_float_type {
    ($($name: ident),*$(,)?) => {
        $(
            fn $name(&self) -> ValueType {
                unimplemented!()
            }
        )*
    };
}

impl<'t> CoreTypeProvider<ValueType> for BasicTypeManager<'t> {
    fn bool(&self) -> ValueType {
        ValueType::Bool
    }

    fn char(&self) -> ValueType {
        ValueType::Char
    }

    impl_int_type!(
        i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
    );

    impl_float_type!(f16, f32, f64, f128);

    fn raw_addr(&self) -> ValueType {
        ValueType::Int(IntType {
            bit_size: usize::BITS as u64,
            is_signed: false,
        })
    }

    fn raw_mut_addr(&self) -> ValueType {
        ValueType::Int(IntType {
            bit_size: usize::BITS as u64,
            is_signed: false,
        })
    }

    fn try_to_value_type(&self, ty: ValueType) -> Option<ValueType> {
        Some(ty)
    }
}

macro_rules! delegate_to_core_types {
    ($($method: ident),*$(,)?) => {
        delegate::delegate! {
            to self.core_types {
                $(
                    fn $method(&self) -> &'t TypeInfo;
                )*
            }
        }
    };
}

impl<'t> CoreTypeProvider<&'t TypeInfo> for BasicTypeManager<'t> {
    pass_core_type_names_to!(delegate_to_core_types);

    fn try_to_value_type(&self, ty: &'t TypeInfo) -> Option<ValueType> {
        macro_rules! by_func {
            ($($name: ident),*$(,)?) => {
                $(
                    if ty.id == self.core_types.$name().id {
                        return Some(CoreTypeProvider::<ValueType>::$name(self));
                    }
                )*
            };
        }

        pass_core_type_names_to!(by_func);

        None
    }
}

macro_rules! impl_type {
    ($($name: ident),*$(,)?) => {
        $(
            fn $name(&self) -> LazyTypeInfo {
                LazyTypeInfo::IdPrimitive(
                    self.core_types.$name().id,
                    CoreTypeProvider::<ValueType>::$name(self),
                )
            }
        )*
    };
}

impl CoreTypeProvider<LazyTypeInfo> for BasicTypeManager<'static>
where
    Self: CoreTypeProvider<&'static TypeInfo> + CoreTypeProvider<ValueType>,
{
    pass_core_type_names_to!(impl_type);

    fn try_to_value_type<'a>(&self, ty: LazyTypeInfo) -> Option<ValueType> {
        match ty {
            LazyTypeInfo::IdPrimitive(_, ty) => self.try_to_value_type(ty),
            LazyTypeInfo::Id(id) => self.try_to_value_type(self.get_type(id)),
            LazyTypeInfo::Fetched(ty) => self.try_to_value_type(ty),
            LazyTypeInfo::Forced(ty) => self.try_to_value_type(ty.as_ref()),
            LazyTypeInfo::None => None,
        }
    }
}
