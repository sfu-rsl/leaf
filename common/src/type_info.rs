use core::ops::RangeInclusive;
use std::{collections::HashMap, fs::OpenOptions, prelude::rust_2021::*};

use macros::cond_derive_serde_rkyv;

pub use crate::types::{Alignment, TypeId, TypeSize};
use crate::{log_info, types::*, utils::array_backed_struct};

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
    pub fn new(mut fields: Vec<FieldInfo>) -> Self {
        fields.sort_by_key(|f| f.offset);
        Self { fields }
    }

    /// # Remarks
    /// The fields are sorted by their offsets.
    #[inline(always)]
    pub fn fields(&self) -> &[FieldInfo] {
        &self.fields
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
pub struct GenericTypesData<All, Cores> {
    pub all_types: All,
    pub core_types: Cores,
}

pub type TypesData = GenericTypesData<HashMap<TypeId, TypeInfo>, CoreTypes>;

pub trait TypeDatabase<'t> {
    fn opt_get_type(&self, key: &TypeId) -> Option<&'t TypeInfo>;

    fn get_type(&self, key: &TypeId) -> &'t TypeInfo {
        self.opt_get_type(key)
            .unwrap_or_else(|| panic!("Type information was not found. TypeId: {}", key))
    }

    fn get_size(&self, key: &TypeId) -> Option<TypeSize> {
        self.get_type(key).size()
    }

    fn core_types(&self) -> &CoreTypes<TypeId>;
}

impl<'t> TypeDatabase<'t> for &'t TypesData {
    fn opt_get_type(&self, key: &TypeId) -> Option<&'t TypeInfo> {
        self.all_types.get(key)
    }

    fn core_types(&self) -> &CoreTypes<TypeId> {
        &self.core_types
    }
}

#[cfg(feature = "std")]
impl<'t, D: TypeDatabase<'t> + ?Sized> TypeDatabase<'t> for std::rc::Rc<D> {
    delegate::delegate! {
        to self.as_ref() {
            fn opt_get_type(&self, key: &TypeId) -> Option<&'t TypeInfo>;
            fn get_type(&self, key: &TypeId) -> &'t TypeInfo;
            fn core_types(&self) -> &CoreTypes<TypeId>;
        }
    }
}

#[cfg(feature = "type_info_rw")]
pub mod rw {
    use core::error::Error as StdError;
    use std::path::{Path, PathBuf};

    use super::*;

    #[cfg(feature = "serde")]
    mod serdes {
        use serde::Serialize;

        use super::*;

        type SerializedTypesData = GenericTypesData<Vec<TypeInfo>, Vec<(String, TypeId)>>;

        pub(super) const FILENAME_DB: &str = "types.json";

        #[cfg(info_db_fmt = "json")]
        pub(super) fn read(db_path: impl AsRef<Path>) -> Result<TypesData, Box<dyn StdError>> {
            use crate::{log_debug, utils::MessagedError};

            let file = OpenOptions::new()
                .read(true)
                .open(db_path.as_ref())
                .map_err(MessagedError::with("Failed to open file for type export"))?;

            let data: SerializedTypesData = serde_json::from_reader(file)
                .map_err(MessagedError::with("Failed to parse types from file."))?;

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

        pub(super) fn write<'a>(
            all_types: impl Iterator<Item = &'a TypeInfo>,
            core_types: CoreTypes<TypeId>,
            out_dir: impl AsRef<Path>,
        ) -> Result<PathBuf, Box<dyn StdError>> {
            let path = out_dir.as_ref().join(FILENAME_DB);
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .map_err(Box::<dyn StdError>::from)?;

            let mut serializer = serde_json::Serializer::pretty(file);
            let data = SerializedTypesData {
                all_types: all_types.cloned().collect(),
                core_types: core_types.to_pairs().to_vec(),
            };
            data.serialize(&mut serializer)
                .map(|_| path)
                .map_err(Box::<dyn StdError>::from)
        }
    }

    #[cfg(feature = "rkyv")]
    mod rkyving {

        use once_map::unsync::OnceMap;
        use rkyv::{
            Archive,
            rancor::{Error, OptionExt},
        };

        use super::*;

        type ArchivedTypesData = <TypesData as rkyv::Archive>::Archived;

        pub struct OwnedArchivedTypesData {
            raw: Box<[u8]>,
            deserialized: GenericTypesData<OnceMap<TypeId, Box<TypeInfo>>, CoreTypes>,
        }

        impl OwnedArchivedTypesData {
            fn new(raw: Box<[u8]>) -> Result<Self, Error> {
                #[cfg(debug_assertions)]
                let core_types = rkyv::access::<ArchivedTypesData, Error>(&raw)
                    .and_then(|a| rkyv::deserialize::<CoreTypes, Error>(&a.core_types))?;
                #[cfg(not(debug_assertions))]
                let core_types = {
                    let serialized =
                        unsafe { &rkyv::access_unchecked::<ArchivedTypesData>(&raw).core_types };
                    rkyv::deserialize::<CoreTypes, Error>(serialized)
                }?;
                Ok(Self {
                    raw,
                    deserialized: GenericTypesData {
                        all_types: Default::default(),
                        core_types,
                    },
                })
            }

            fn access(&self) -> &ArchivedTypesData {
                unsafe { rkyv::access_unchecked(&self.raw) }
            }
        }

        impl<'t> TypeDatabase<'t> for &'static OwnedArchivedTypesData {
            fn opt_get_type(&self, key: &TypeId) -> Option<&'static TypeInfo> {
                self.deserialized
                    .all_types
                    .try_insert(*key, |key| {
                        self.access()
                            .all_types
                            .get(&<TypeId as Archive>::Archived::from_native(*key))
                            .into_error::<Error>()
                            .map(|a| {
                                rkyv::deserialize::<TypeInfo, Error>(a)
                                    .map(Box::new)
                                    .expect("Failed to deserialize")
                            })
                    })
                    .ok()
            }

            fn core_types(&self) -> &CoreTypes<TypeId> {
                &self.deserialized.core_types
            }
        }

        pub(super) const FILENAME_DB: &str = "types.rkyv";

        pub(super) fn read(
            db_path: impl AsRef<Path>,
        ) -> Result<OwnedArchivedTypesData, Box<dyn StdError>> {
            let raw = std::fs::read(db_path)?;
            OwnedArchivedTypesData::new(raw.into_boxed_slice()).map_err(Into::into)
        }

        pub(super) fn write<'a>(
            all_types: impl Iterator<Item = &'a TypeInfo>,
            core_types: CoreTypes<TypeId>,
            out_dir: impl AsRef<Path>,
        ) -> Result<PathBuf, Box<dyn StdError>> {
            let path = out_dir.as_ref().join(FILENAME_DB);
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .map_err(Box::<dyn StdError>::from)?;

            let data = TypesData {
                all_types: all_types
                    .cloned()
                    .map(|mut t| {
                        // Clearing the space-consuming name, as this format is not read by human.
                        t.name = String::new();
                        t
                    })
                    .map(|t| (t.id, t))
                    .collect(),
                core_types,
            };

            rkyv::api::high::to_bytes_in::<_, Error>(&data, rkyv::ser::writer::IoWriter::new(file))
                .map(|_| path)
                .map_err(Box::<dyn StdError>::from)
        }
    }

    #[cfg(info_db_fmt = "json")]
    pub type LoadedTypeDatabase = TypesData;
    #[cfg(info_db_fmt = "rkyv")]
    pub type LoadedTypeDatabase = rkyving::OwnedArchivedTypesData;

    #[cfg(info_db_fmt = "json")]
    pub const FILENAME_DB: &str = serdes::FILENAME_DB;

    #[cfg(info_db_fmt = "rkyv")]
    pub const FILENAME_DB: &str = rkyving::FILENAME_DB;

    pub fn read_types_db() -> Result<LoadedTypeDatabase, Box<dyn StdError>> {
        log_info!("Finding and reading types db");

        let path = crate::utils::search_next_to_exe_for(FILENAME_DB)
            .ok_or_else(|| Box::<dyn StdError>::from("Failed to find types db"))?;

        #[cfg(info_db_fmt = "json")]
        let result = serdes::read();
        #[cfg(info_db_fmt = "rkyv")]
        let result = rkyving::read(path);
        result
    }

    pub fn write_types_db_in<'a>(
        types: impl Iterator<Item = &'a TypeInfo> + Clone,
        core_types: CoreTypes<TypeId>,
        out_dir: impl AsRef<Path>,
    ) -> Result<PathBuf, Box<dyn StdError>> {
        log_info!("Writing type info db in: `{}`", out_dir.as_ref().display());

        if cfg!(debug_assertions) {
            serdes::write(types.clone(), core_types.clone(), out_dir.as_ref())?;
        }

        // Writing in JSON format may be used for debugging purposes, so making it easier to enable.
        let result = if cfg!(info_db_fmt = "json") {
            serdes::write(types, core_types, out_dir)
        } else if cfg!(info_db_fmt = "rkyv") {
            rkyving::write(types, core_types, out_dir)
        } else {
            unreachable!()
        };
        result
    }
}
