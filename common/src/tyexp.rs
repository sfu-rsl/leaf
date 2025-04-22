use core::ops::RangeInclusive;
use std::{collections::HashMap, fs::OpenOptions};

use serde::{Deserialize, Serialize};

use crate::{
    types::*,
    utils::{StrError, array_backed_struct},
    *,
};

pub use crate::types::{Alignment, TypeId, TypeSize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    pub fn get_variant(&self, index: VariantIndex) -> Option<&VariantInfo> {
        // There is no guarantee that the index field is as same as the item's index in the array.
        self.variants.iter().find(|v| v.index == index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariantInfo {
    pub index: VariantIndex,
    pub fields: FieldsShapeInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldsShapeInfo {
    NoFields,
    Array(ArrayShape),
    Struct(StructShape),
    Union(UnionShape),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrayShape {
    pub len: u64,
    pub item_ty: TypeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructShape {
    pub fields: Vec<FieldInfo>,
}

// We use the same struct to avoid redundancy. Offset is not used for unions.
pub type UnionShape = StructShape;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldInfo {
    pub ty: TypeId,
    pub offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TagInfo {
    Constant {
        discr_bit_rep: u128,
    },
    Regular {
        as_field: FieldInfo,
        encoding: TagEncodingInfo,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            #[derive(Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
pub struct GenericTypesData<All, Cores> {
    pub all_types: All,
    pub core_types: Cores,
}

pub type TypesData = GenericTypesData<HashMap<TypeId, TypeInfo>, CoreTypes>;

type SerializedTypesData = GenericTypesData<Vec<TypeInfo>, Vec<(String, TypeId)>>;

pub struct TypeExport;

pub const FINAL_TYPE_EXPORT_FILE: &str = "types.json";

// FIXME: Move these functions to a more appropriate place.
// FIXME: Make this configurable and injectable.
impl TypeExport {
    pub fn read() -> Result<TypesData, StrError> {
        let type_info_file_path =
            crate::utils::search_current_ancestor_dirs_for(FINAL_TYPE_EXPORT_FILE)
                .expect("Failed to find the type info file.");

        let data = Self::parse_exported_types(type_info_file_path.display().to_string())?;
        log_debug!("Retrieved {} types from file.", data.all_types.len());

        let types = TypesData {
            all_types: data
                .all_types
                .into_iter()
                .map(|type_info| (type_info.id, type_info))
                .collect(),
            core_types: CoreTypes::try_from(
                data.core_types.into_iter().collect::<Vec<_>>().as_slice(),
            )
            .unwrap(),
        };

        Ok(types)
    }

    pub fn write<'a>(
        types: impl Iterator<Item = &'a TypeInfo>,
        core_types: CoreTypes<TypeId>,
        file_path: String,
    ) {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(file_path)
            .expect("Unable to open file for type export");
        let mut serializer = serde_json::Serializer::pretty(file);
        let data = SerializedTypesData {
            all_types: types.cloned().collect(),
            core_types: core_types.to_pairs().to_vec(),
        };
        data.serialize(&mut serializer)
            .expect("Failed to write types to file.");
    }

    pub fn parse_exported_types(file_path: String) -> Result<SerializedTypesData, StrError> {
        let file =
            OpenOptions::new()
                .read(true)
                .open(file_path)
                .map_err(StrError::with_message(
                    "Failed to open file for type export",
                ))?;

        let type_infos: SerializedTypesData = serde_json::from_reader(file)
            .map_err(StrError::with_message("Failed to parse types from file."))?;
        Ok(type_infos)
    }
}
