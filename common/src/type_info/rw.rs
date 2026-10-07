use core::error::Error as StdError;
use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
    prelude::rust_2024::*,
};

use crate::log_info;

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
        deserialized: GenericTypesData<OnceMap<TypeId, Box<TypeInfo>, FxBuildHasher64>, CoreTypes>,
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
            self.access()
                .all_types
                .get(&key.into())
                .unwrap_or_else(|| core::panic!("Type information was not found. TypeId: {}", key))
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
                        return Err(format!("Conflicting type information for TypeId {id}").into());
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
        let directory = std::env::temp_dir().join(format!("leaf-typedb-{}", std::process::id()));
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
