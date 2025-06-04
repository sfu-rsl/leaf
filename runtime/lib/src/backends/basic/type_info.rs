use delegate::delegate;

use common::type_info::{
    CoreTypes, FieldsShapeInfo, StructShape, TypeInfo, pass_core_type_names_to,
};

use crate::{
    abs::{IntType, PointerOffset, TypeSize, backend::TypeDatabase},
    type_info::{self, TypeInfoExt},
};

use crate::backends::basic as backend;
use backend::{CoreTypeProvider, LazyTypeInfo, TypeId, TypeLayoutResolver, ValueType};

pub(crate) struct BasicTypeManager<D: TypeDatabase<'static>> {
    inner: D,
    core_types: CoreTypes<&'static TypeInfo>,
}

impl<D: TypeDatabase<'static>> BasicTypeManager<D> {
    fn new(db: D) -> Self {
        let core_types = db.core_types().map(|id| db.get_type(&id));
        Self {
            inner: db,
            core_types,
        }
    }
}

pub(crate) fn default_type_manager() -> BasicTypeManager<impl TypeDatabase<'static>> {
    BasicTypeManager::new(type_info::instance::get())
}

impl<D: TypeDatabase<'static>> TypeDatabase<'static> for BasicTypeManager<D> {
    delegate! {
        to self.inner {
            fn opt_get_type(&self, key: &TypeId) -> Option<&'static TypeInfo>;
            fn core_types(&self) -> &CoreTypes<TypeId>;
        }
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

impl<D: TypeDatabase<'static>> CoreTypeProvider<ValueType> for BasicTypeManager<D> {
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
                    fn $method(&self) -> &'static TypeInfo;
                )*
            }
        }
    };
}

impl<'t, 'd, D: TypeDatabase<'static>> CoreTypeProvider<&'t TypeInfo> for BasicTypeManager<D> {
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

impl<D: TypeDatabase<'static>> CoreTypeProvider<LazyTypeInfo> for BasicTypeManager<D>
where
    Self: for<'t> CoreTypeProvider<&'t TypeInfo> + CoreTypeProvider<ValueType>,
{
    pass_core_type_names_to!(impl_type);

    fn try_to_value_type<'a>(&self, ty: LazyTypeInfo) -> Option<ValueType> {
        match ty {
            LazyTypeInfo::IdPrimitive(_, ty) => self.try_to_value_type(ty),
            LazyTypeInfo::Id(id) => self.try_to_value_type(self.get_type(&id)),
            LazyTypeInfo::Fetched(ty) => self.try_to_value_type(ty),
            LazyTypeInfo::Forced(ty) => self.try_to_value_type(ty.as_ref()),
            LazyTypeInfo::None => None,
        }
    }
}

struct LayoutResolver<'a, D: ?Sized>(&'a D);

// https://doc.rust-lang.org/reference/type-layout.html
impl<'a, 't, D: TypeDatabase<'t> + ?Sized> TypeLayoutResolver<'t> for LayoutResolver<'a, D> {
    fn resolve_array_elements(
        &self,
        type_id: TypeId,
    ) -> (TypeId, impl Iterator<Item = (PointerOffset, TypeSize)> + 't) {
        let ty = self.0.get_type(&type_id).expect_array();
        let item_ty = self.0.get_type(&ty.item_ty);
        let item_size = item_ty.size().unwrap();
        (
            item_ty.id,
            (0..ty.len).into_iter().map(move |i| {
                let offset = item_size * i;
                (offset, item_size)
            }),
        )
    }

    fn resolve_adt_fields(
        &self,
        type_id: TypeId,
        variant: Option<crate::abs::VariantIndex>,
    ) -> impl Iterator<Item = (TypeId, PointerOffset, TypeSize)> + 't {
        let ty = self.0.get_type(&type_id);
        let variant = match variant {
            Some(variant) => ty.get_variant(variant).unwrap(),
            None => ty.expect_single_variant(),
        };

        use FieldsShapeInfo::*;
        match &variant.fields {
            Struct(StructShape { fields }) | Union(StructShape { fields }) => {
                if cfg!(debug_assertions) {}
                {
                    if matches!(&variant.fields, Union(..)) {
                        assert!(
                            fields.iter().all(|f| f.offset == 0),
                            "Union fields must have zero offset"
                        );
                    }
                }

                // We collect them to break the borrow
                let field_tys = fields
                    .iter()
                    .map(|f| self.0.get_type(&f.ty))
                    .collect::<Vec<_>>();
                fields
                    .iter()
                    .zip(field_tys.into_iter())
                    .map(|(f, field_ty)| {
                        let offset = f.offset;
                        let size = field_ty.size().unwrap();
                        (field_ty.id, offset, size)
                    })
            }
            NoFields | Array(..) => panic!(
                "Unexpected shape for fields of an ADT: {:?}",
                variant.fields
            ),
        }
    }
}

pub(super) trait TypeLayoutResolverExt<'t>: TypeDatabase<'t> {
    fn layouts<'a>(&'a self) -> impl TypeLayoutResolver<'t> + 'a {
        LayoutResolver(self)
    }
}

impl<'t, D: TypeDatabase<'t> + ?Sized> TypeLayoutResolverExt<'t> for D {}
