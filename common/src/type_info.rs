use core::ops::RangeInclusive;
use std::{collections::HashMap, fs::OpenOptions, prelude::rust_2024::*};

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

        #[cfg(info_db_fmt = "json")]
        pub(super) const FILENAME_DB: &str = "types.json";

        pub(super) const FILENAME_DB_EXT: &str = ".json";

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
                metadata: data.metadata,
            };

            Ok(types)
        }

        pub(super) fn write<'a>(
            all_types: impl Iterator<Item = &'a TypeInfo>,
            core_types: CoreTypes<TypeId>,
            metadata: HashMap<String, MetadataValue>,
            out_file: impl AsRef<Path>,
        ) -> Result<PathBuf, Box<dyn StdError>> {
            let path = out_file.as_ref();
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(path)
                .map_err(Box::<dyn StdError>::from)?;

            let mut serializer = serde_json::Serializer::pretty(file);
            let data = SerializedTypesData {
                all_types: all_types.cloned().collect(),
                core_types: core_types.to_pairs().to_vec(),
                metadata,
            };
            data.serialize(&mut serializer)
                .map(|_| path.to_path_buf())
                .map_err(Box::<dyn StdError>::from)
        }
    }

    #[cfg(feature = "rkyv")]
    mod rkyving {

        #[cfg(not(feature = "type_db_access_unsync"))]
        use once_map::sync::OnceMap;
        #[cfg(feature = "type_db_access_unsync")]
        use once_map::unsync::OnceMap;
        use rkyv::{
            Archive,
            hash::FxHasher64,
            option::ArchivedOption,
            rancor::{Error, OptionExt},
        };

        use super::*;

        type ArchivedTypesData = <TypesData as rkyv::Archive>::Archived;

        #[derive(Default)]
        struct FxBuildHasher64;

        impl core::hash::BuildHasher for FxBuildHasher64 {
            type Hasher = FxHasher64;

            fn build_hasher(&self) -> Self::Hasher {
                Default::default()
            }
        }

        pub struct OwnedArchivedTypesData {
            raw: Box<[u8]>,
            deserialized:
                GenericTypesData<OnceMap<TypeId, Box<TypeInfo>, FxBuildHasher64>, CoreTypes>,
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
                #[cfg(debug_assertions)]
                let metadata = rkyv::access::<ArchivedTypesData, Error>(&raw)
                    .and_then(|a| rkyv::deserialize::<_, Error>(&a.metadata))?;
                #[cfg(not(debug_assertions))]
                let metadata = {
                    let serialized =
                        unsafe { &rkyv::access_unchecked::<ArchivedTypesData>(&raw).metadata };
                    rkyv::deserialize::<_, Error>(serialized)
                }?;
                Ok(Self {
                    raw,
                    deserialized: GenericTypesData {
                        all_types: Default::default(),
                        core_types,
                        metadata,
                    },
                })
            }

            fn access(&self) -> &ArchivedTypesData {
                unsafe { rkyv::access_unchecked(&self.raw) }
            }

            fn expect_type(&self, key: &TypeId) -> &ArchivedTypeInfo {
                self.access().all_types.get(&key.into()).unwrap_or_else(|| {
                    core::panic!("Type information was not found. TypeId: {}", key)
                })
            }
        }

        impl ArchivedTypeInfo {
            #[inline(always)]
            pub fn is_sized(&self) -> bool {
                self.size != TypeInfo::SIZE_UNSIZED
            }

            pub fn size(&self) -> Option<TypeSize> {
                self.is_sized().then_some(self.size.to_native())
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

            fn get_size(&self, key: &TypeId) -> Option<TypeSize> {
                self.expect_type(key).size()
            }

            fn get_pointee_ty(&self, key: &TypeId) -> Option<TypeId> {
                match self.expect_type(key).pointee_ty {
                    ArchivedOption::None => None,
                    ArchivedOption::Some(pointee_ty) => Some(pointee_ty.into()),
                }
            }

            fn core_types(&self) -> &CoreTypes<TypeId> {
                &self.deserialized.core_types
            }

            fn get_metadata(&self, key: &str) -> Option<&MetadataValue> {
                self.deserialized.metadata.get(key)
            }
        }

        pub(super) const FILENAME_DB: &str = "types.rkyv";

        pub(super) fn read(
            db_path: impl AsRef<Path>,
        ) -> Result<OwnedArchivedTypesData, Box<dyn StdError>> {
            let raw = std::fs::read(db_path)?;
            OwnedArchivedTypesData::new(raw.into_boxed_slice()).map_err(Into::into)
        }

        pub(super) fn read_materialized(
            db_path: impl AsRef<Path>,
        ) -> Result<TypesData, Box<dyn StdError>> {
            let raw = std::fs::read(db_path)?;
            let archived = rkyv::access::<ArchivedTypesData, Error>(&raw)?;
            rkyv::deserialize::<TypesData, Error>(archived).map_err(Into::into)
        }

        pub(super) fn write<'a>(
            all_types: impl Iterator<Item = &'a TypeInfo>,
            core_types: CoreTypes<TypeId>,
            metadata: HashMap<String, MetadataValue>,
            out_file: impl AsRef<Path>,
        ) -> Result<PathBuf, Box<dyn StdError>> {
            let path = out_file.as_ref();
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(path)
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
                metadata,
            };

            rkyv::api::high::to_bytes_in::<_, Error>(&data, rkyv::ser::writer::IoWriter::new(file))
                .map(|_| path.to_path_buf())
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

    pub const FILENAME_STABLE_PREFIX: &str = "types-";
    pub const ENV_TYPES_DB: &str = "LEAF_TYPES_DB";

    pub fn stable_db_file_name(stable_crate_id: impl std::fmt::Display) -> String {
        format!(
            "{FILENAME_STABLE_PREFIX}{stable_crate_id}.{}",
            FILENAME_DB
                .rsplit_once('.')
                .map_or("", |(_, extension)| extension)
        )
    }

    pub fn is_stable_db_file_name(name: &str) -> bool {
        let Some(extension_start) = FILENAME_DB.find('.') else {
            return false;
        };
        name.strip_prefix(FILENAME_STABLE_PREFIX)
            .and_then(|name| name.strip_suffix(&FILENAME_DB[extension_start..]))
            .is_some_and(|stable_id| !stable_id.is_empty())
    }

    pub fn read_types_db_from(db_path: impl AsRef<Path>) -> Result<TypesData, Box<dyn StdError>> {
        #[cfg(info_db_fmt = "json")]
        let result = serdes::read(db_path);
        #[cfg(info_db_fmt = "rkyv")]
        let result = rkyving::read_materialized(db_path);
        result
    }

    pub fn read_types_db() -> Result<LoadedTypeDatabase, Box<dyn StdError>> {
        log_info!("Finding and reading types db");

        let path = std::env::var_os(ENV_TYPES_DB)
            .map(PathBuf::from)
            .map(|path| {
                if path
                    .parent()
                    .is_some_and(|parent| parent.as_os_str().is_empty())
                {
                    path.to_str()
                        .and_then(crate::utils::search_next_to_exe_for)
                        .unwrap_or(path)
                } else {
                    path
                }
            })
            .or_else(|| crate::utils::search_next_to_exe_for(FILENAME_DB))
            .ok_or_else(|| Box::<dyn StdError>::from("Failed to find types db"))?;

        #[cfg(info_db_fmt = "json")]
        let result = serdes::read(path);
        #[cfg(info_db_fmt = "rkyv")]
        let result = rkyving::read(path);
        result
    }

    pub fn merge_types_dbs(
        databases: impl IntoIterator<Item = TypesData>,
    ) -> Result<TypesData, Box<dyn StdError>> {
        let mut merged: Option<TypesData> = None;

        for database in databases {
            if let Some(existing) = &mut merged {
                for (id, type_info) in database.all_types {
                    match existing.all_types.get(&id) {
                        Some(previous) if previous != &type_info => {
                            return Err(
                                format!("Conflicting type information for TypeId {id}").into()
                            );
                        }
                        Some(_) => {}
                        None => {
                            existing.all_types.insert(id, type_info);
                        }
                    }
                }

                for ((key, value), previous_value) in database
                    .core_types
                    .to_pairs()
                    .into_iter()
                    .zip(existing.core_types.clone().to_pairs())
                {
                    if value != previous_value.1 {
                        return Err(format!("Conflicting core type mapping for `{key}`").into());
                    }
                }

                for (key, value) in database.metadata {
                    match existing.metadata.get(&key) {
                        Some(previous) if previous != &value => {
                            return Err(format!("Conflicting metadata value for `{key}`").into());
                        }
                        Some(_) => {}
                        None => {
                            existing.metadata.insert(key, value);
                        }
                    }
                }
            } else {
                merged = Some(database);
            }
        }

        merged.ok_or_else(|| "Cannot merge an empty collection of type databases".into())
    }

    pub fn write_types_db_to<'a>(
        types: impl Iterator<Item = &'a TypeInfo> + Clone,
        core_types: CoreTypes<TypeId>,
        metadata: HashMap<String, MetadataValue>,
        out_file: impl AsRef<Path>,
    ) -> Result<PathBuf, Box<dyn StdError>> {
        log_info!("Writing type info db to: `{}`", out_file.as_ref().display());

        if cfg!(debug_assertions) {
            serdes::write(
                types.clone(),
                core_types.clone(),
                metadata.clone(),
                out_file.as_ref().with_extension(serdes::FILENAME_DB_EXT),
            )?;
        }

        // Writing in JSON format may be used for debugging purposes, so making it easier to enable.
        let result = if cfg!(info_db_fmt = "json") {
            serdes::write(types, core_types, metadata, out_file)
        } else if cfg!(info_db_fmt = "rkyv") {
            rkyving::write(types, core_types, metadata, out_file)
        } else {
            unreachable!()
        };
        result
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn type_id(value: u128) -> TypeId {
            TypeId::new(value).unwrap()
        }

        fn core_types(id: TypeId) -> CoreTypes {
            CoreTypes::from([id; 20])
        }

        fn database(
            type_info: Option<TypeInfo>,
            core_id: TypeId,
            metadata: MetadataValue,
        ) -> TypesData {
            TypesData {
                all_types: type_info
                    .map(|type_info| HashMap::from([(type_info.id, type_info)]))
                    .unwrap_or_default(),
                core_types: core_types(core_id),
                metadata: HashMap::from([("build".to_owned(), metadata)]),
            }
        }

        fn type_info(id: TypeId, size: TypeSize) -> TypeInfo {
            TypeInfo {
                id,
                name: "test".to_owned(),
                variants: vec![],
                tag: None,
                pointee_ty: None,
                align: 1,
                size,
            }
        }

        #[test]
        fn named_file_round_trip_materializes_types_data() {
            let generated_type = type_info(type_id(1), 8);
            let database = database(
                Some(generated_type.clone()),
                type_id(2),
                MetadataValue::String("debug".to_owned()),
            );
            let directory =
                std::env::temp_dir().join(format!("leaf-typedb-{}", std::process::id()));
            std::fs::create_dir_all(&directory).unwrap();
            let file = directory.join(stable_db_file_name("round-trip"));

            write_types_db_to(
                database.all_types.values(),
                database.core_types.clone(),
                database.metadata.clone(),
                &file,
            )
            .unwrap();
            let loaded = read_types_db_from(&file).unwrap();

            let loaded_type = loaded.all_types.get(&type_id(1)).unwrap();
            let expected_type = TypeInfo {
                name: String::new(),
                ..generated_type
            };
            assert_eq!(loaded_type, &expected_type);
            assert_eq!(loaded.core_types.as_ref(), database.core_types.as_ref());
            assert_eq!(loaded.metadata, database.metadata);
            std::fs::remove_dir_all(directory).unwrap();
        }

        #[test]
        fn merge_keeps_identical_entries() {
            let database = database(
                Some(type_info(type_id(1), 8)),
                type_id(2),
                MetadataValue::String("debug".to_owned()),
            );
            let merged = merge_types_dbs([database.clone(), database]).unwrap();

            assert_eq!(merged.all_types.len(), 1);
        }

        #[test]
        fn merge_rejects_type_core_and_metadata_conflicts() {
            assert!(
                merge_types_dbs([
                    database(
                        Some(type_info(type_id(1), 8)),
                        type_id(2),
                        MetadataValue::String("debug".to_owned()),
                    ),
                    database(
                        Some(type_info(type_id(1), 16)),
                        type_id(2),
                        MetadataValue::String("debug".to_owned()),
                    ),
                ])
                .is_err()
            );

            assert!(
                merge_types_dbs([
                    database(None, type_id(2), MetadataValue::String("debug".to_owned())),
                    database(None, type_id(3), MetadataValue::String("debug".to_owned())),
                ])
                .is_err()
            );

            assert!(
                merge_types_dbs([
                    database(None, type_id(2), MetadataValue::String("debug".to_owned())),
                    database(
                        None,
                        type_id(2),
                        MetadataValue::String("release".to_owned()),
                    ),
                ])
                .is_err()
            );
        }
    }
}
