#[cfg(feature = "type_info_rw")]
pub mod rw;

use core::ops::RangeInclusive;
use std::{collections::HashMap, prelude::rust_2024::*};

use macros::cond_derive_serde_rkyv;

pub use crate::types::{Alignment, TypeId, TypeSize};
use crate::{types::*, utils::array_backed_struct};

#[cond_derive_serde_rkyv]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeInfo {
    pub id: TypeId,
    // Type name.
    pub name: String,
    // Variants of the ADT. If this is a struct or union, then there will be a single variant.
    pub variants: Vec<VariantInfo>,
    pub tag: Option<TagInfo>,

    pub pointee_ty: Option<TypeId>,

    pub align: Alignment,
    pub size: TypeSize,
}

impl TypeInfo {
    pub const SIZE_UNSIZED: TypeSize = TypeSize::MAX;

    #[inline(always)]
    pub fn is_sized(&self) -> bool {
        self.size != Self::SIZE_UNSIZED
    }

    pub fn size(&self) -> Option<TypeSize> {
        self.is_sized().then_some(self.size)
    }

    pub fn get_variant(&self, index: VariantIndex) -> Option<&VariantInfo> {
        // There is no guarantee that the index field is as same as the item's index in the array.
        self.variants.iter().find(|v| v.index == index)
    }
}

#[cond_derive_serde_rkyv]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantInfo {
    pub index: VariantIndex,
    pub fields: FieldsShapeInfo,
}

#[cond_derive_serde_rkyv]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldsShapeInfo {
    NoFields,
    Array(ArrayShape),
    Struct(StructShape),
    Union(UnionShape),
}

#[cond_derive_serde_rkyv]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayShape {
    pub len: u64,
    pub item_ty: TypeId,
}

#[cond_derive_serde_rkyv]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructShape {
    fields: Vec<FieldInfo>,
    indices_ordered_by_offset: Vec<FieldIndex>,
}

// We use the same struct to avoid redundancy. Offset is not used for unions.
pub type UnionShape = StructShape;

#[cond_derive_serde_rkyv]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    pub ty: TypeId,
    pub offset: u64,
}

#[cond_derive_serde_rkyv]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagInfo {
    Constant {
        discr_bit_rep: u128,
    },
    Regular {
        as_field: FieldInfo,
        encoding: TagEncodingInfo,
    },
}

#[cond_derive_serde_rkyv]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagEncodingInfo {
    Direct,
    Niche {
        /// The discriminant value when the variant is not a niche.
        non_niche_value: u128,
        /// The range of values for the discriminant when the variant is a niche.
        // NOTE: As the range check is wrapping, we need the end value.
        niche_value_range: RangeInclusive<u128>,
        /// The value of the tag when the variant is at the start of the niche range.
        tag_value_start: u128,
    },
}

impl StructShape {
    pub fn new(fields: Vec<FieldInfo>) -> Self {
        let indices_ordered_by_offset = {
            let mut indices: Vec<_> = (0..fields.len()).collect();
            indices.sort_by_key(|i| fields[*i as usize].offset);
            indices
                .into_iter()
                .map(|i| FieldIndex::try_from(i).unwrap())
                .collect()
        };
        Self {
            fields,
            indices_ordered_by_offset,
        }
    }

    /// # Remarks
    /// The order is based on the field indices used in MIR.
    /// The offsets do not necessarily match the order of the fields in the struct.
    #[inline(always)]
    pub fn fields(&self) -> &[FieldInfo] {
        &self.fields
    }

    pub fn fields_in_offset_order(&self) -> impl Iterator<Item = (FieldIndex, &FieldInfo)> + Clone {
        self.indices_ordered_by_offset.iter().copied().map(|i| {
            let field = &self.fields[i as usize];
            (FieldIndex::try_from(i).unwrap(), field)
        })
    }
}

#[cfg_attr(not(core_build), macro_export)]
macro_rules! pass_core_type_names_to {
    ($macro:ident) => {
        $macro! {
            bool,
            char,
            i8, i16, i32, i64, i128, isize,
            u8, u16, u32, u64, u128, usize,
            f16, f32, f64, f128,
            raw_addr, raw_mut_addr,
        }
    };
}
pub use pass_core_type_names_to;

macro_rules! define_core_types {
    ($($name: ident),*$(,)?) => {
        array_backed_struct! {
            #[cond_derive_serde_rkyv]
            #[derive(Clone)]
            pub struct CoreTypes<V = TypeId> {
                $($name),*
            }: V;
        }
    };
}

pass_core_type_names_to!(define_core_types);

impl<V: Copy> CoreTypes<V> {
    pub fn map<T: Copy>(&self, f: impl FnMut(V) -> T) -> CoreTypes<T> {
        self.0.map(f).into()
    }
}

macro_rules! define_named_core_types {
    ($($name: ident),*$(,)?) => {
        pub struct NamedCoreTypes<V = TypeId> {
            $(
                pub $name: V
            ),*
        }
    };
}

pass_core_type_names_to!(define_named_core_types);

impl<V: Copy> From<NamedCoreTypes<V>> for CoreTypes<V> {
    fn from(named: NamedCoreTypes<V>) -> Self {
        macro_rules! to_array {
            ($($name: ident),*$(,)?) => {
                [$(named.$name),*]
            };
        }
        CoreTypes(pass_core_type_names_to!(to_array))
    }
}

#[cond_derive_serde_rkyv]
#[derive(Clone)]
pub struct GenericTypesData<All, Cores> {
    pub all_types: All,
    pub core_types: Cores,
    pub metadata: HashMap<String, MetadataValue>,
}

pub type MetadataValue = super::utils::JsonLikeValue;

pub type TypesData = GenericTypesData<HashMap<TypeId, TypeInfo>, CoreTypes>;

pub trait TypeDatabase<'t> {
    fn opt_get_type(&self, key: &TypeId) -> Option<&'t TypeInfo>;

    fn get_type(&self, key: &TypeId) -> &'t TypeInfo {
        self.opt_get_type(key)
            .unwrap_or_else(|| core::panic!("Type information was not found. TypeId: {}", key))
    }

    fn get_size(&self, key: &TypeId) -> Option<TypeSize> {
        self.get_type(key).size()
    }

    fn get_pointee_ty(&self, key: &TypeId) -> Option<TypeId> {
        self.get_type(key).pointee_ty
    }

    fn get_pointee_size(&self, key: &TypeId) -> Option<TypeSize> {
        self.get_pointee_ty(key)
            .and_then(|pointee| self.get_size(&pointee))
    }

    fn core_types(&self) -> &CoreTypes<TypeId>;

    fn get_metadata(&self, key: &str) -> Option<&MetadataValue>;
}

impl<'t> TypeDatabase<'t> for &'t TypesData {
    fn opt_get_type(&self, key: &TypeId) -> Option<&'t TypeInfo> {
        self.all_types.get(key)
    }

    fn core_types(&self) -> &CoreTypes<TypeId> {
        &self.core_types
    }

    fn get_metadata(&self, key: &str) -> Option<&MetadataValue> {
        self.metadata.get(key)
    }
}

#[cfg(feature = "std")]
impl<'t, D: TypeDatabase<'t> + ?Sized> TypeDatabase<'t> for std::rc::Rc<D> {
    delegate::delegate! {
        to self.as_ref() {
            fn opt_get_type(&self, key: &TypeId) -> Option<&'t TypeInfo>;
            fn get_type(&self, key: &TypeId) -> &'t TypeInfo;
            fn core_types(&self) -> &CoreTypes<TypeId>;
            fn get_metadata(&self, key: &str) -> Option<&MetadataValue>;
        }
    }
}
