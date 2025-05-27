use core::hash::Hash;
use std::{collections::HashMap, fs::OpenOptions, prelude::rust_2021::*};

use macros::cond_derive_serde_rkyv;

use super::types::{AdjListGraph, AssignmentId, BasicBlockIndex, InstanceKindId};

// A -> B(s) === A control-depends on B(s)
pub type ControlDependencyGraph = AdjListGraph<BasicBlockIndex, BasicBlockIndex>;

type AssignmentIdMap = Vec<BasicBlockIndex>;

#[derive(Default, Clone)]
#[cond_derive_serde_rkyv]
pub struct PlainProgramDependenceMap<I: Eq + Hash = InstanceKindId> {
    pub control_dep: HashMap<I, ControlDependencyGraph>,
    pub assignment_bb_map: HashMap<I, AssignmentIdMap>,
}

pub trait ProgramDependenceMap {
    fn control_dependency(&self, body: InstanceKindId) -> Option<impl ControlDependency>;

    fn assignment_id_map(&self, body: InstanceKindId) -> Option<impl ProgramDepAssignmentIdMap>;
}

pub trait ControlDependency {
    fn controllers<'a>(
        &'a self,
        block: BasicBlockIndex,
    ) -> impl IntoIterator<Item = BasicBlockIndex>;
}

pub trait ProgramDepAssignmentIdMap {
    fn basic_block_index(&self, id: AssignmentId) -> BasicBlockIndex;
}

impl ControlDependency for ControlDependencyGraph {
    fn controllers<'a>(
        &'a self,
        block: BasicBlockIndex,
    ) -> impl IntoIterator<Item = BasicBlockIndex> {
        self.get(&block).into_iter().flat_map(|d| d.iter().cloned())
    }
}

impl ProgramDepAssignmentIdMap for AssignmentIdMap {
    fn basic_block_index(&self, id: AssignmentId) -> BasicBlockIndex {
        self[id as usize]
    }
}

pub mod rw {
    use core::error::Error as StdError;
    use std::{fs::File, path::Path};

    use super::super::log_info;

    use super::*;

    #[cfg(feature = "serde")]
    mod serdes {
        use serde::Serialize;

        use super::*;

        type SerializedMap = PlainProgramDependenceMap;

        pub(super) const FILENAME: &str = "program_dep.json";

        #[cfg(info_db_fmt = "json")]
        pub(super) fn read(
            db_path: impl AsRef<Path>,
        ) -> Result<PlainProgramDependenceMap, Box<dyn StdError>> {
            use crate::{log_debug, utils::MessagedError};

            let file = OpenOptions::new()
                .read(true)
                .open(db_path.as_ref())
                .map_err(MessagedError::with("Failed to open file for type export"))?;

            let data: SerializedMap = serde_json::from_reader(file)
                .map_err(MessagedError::with("Failed to parse types from file."))?;

            Ok(data)
        }

        pub(super) fn write<'a>(
            pdm: &PlainProgramDependenceMap,
            file: File,
        ) -> Result<(), Box<dyn StdError>> {
            let mut serializer = serde_json::Serializer::pretty(file);
            pdm.serialize(&mut serializer)
                .map_err(Box::<dyn StdError>::from)
        }
    }

    #[cfg(feature = "rkyv")]
    mod rkyving {

        use rkyv::{Archive, rancor::Error};

        use crate::program_dep::PlainProgramDependenceMap;

        use super::*;

        type ArchivedMap = <PlainProgramDependenceMap as Archive>::Archived;

        pub struct OwnedArchivedMap {
            raw: Box<[u8]>,
        }

        impl OwnedArchivedMap {
            fn new(raw: Box<[u8]>) -> Result<Self, Error> {
                rkyv::access::<ArchivedMap, Error>(&raw)?;
                Ok(Self { raw })
            }

            fn access(&self) -> &ArchivedMap {
                unsafe { rkyv::access_unchecked(&self.raw) }
            }
        }

        impl ProgramDependenceMap for OwnedArchivedMap {
            #[tracing::instrument(level = "trace", skip(self))]
            fn control_dependency(&self, body: InstanceKindId) -> Option<impl ControlDependency> {
                self.access().control_dep.get(&body.into())
            }

            #[tracing::instrument(level = "trace", skip(self))]
            fn assignment_id_map(
                &self,
                body: InstanceKindId,
            ) -> Option<impl ProgramDepAssignmentIdMap> {
                self.access().assignment_bb_map.get(&body.into())
            }
        }

        impl ControlDependency for &<ControlDependencyGraph as Archive>::Archived {
            #[tracing::instrument(level = "trace", skip(self))]
            fn controllers<'a>(
                &'a self,
                block: BasicBlockIndex,
            ) -> impl IntoIterator<Item = BasicBlockIndex> {
                self.get(&block.into())
                    .into_iter()
                    .flat_map(|d| d.iter())
                    .map(|index| index.to_native())
            }
        }

        impl ProgramDepAssignmentIdMap for &<AssignmentIdMap as Archive>::Archived {
            #[tracing::instrument(level = "trace", skip(self))]
            fn basic_block_index(&self, id: AssignmentId) -> BasicBlockIndex {
                self[id as usize].to_native()
            }
        }

        pub(super) const FILENAME: &str = "program_dep.rkyv";

        pub(super) fn read(path: impl AsRef<Path>) -> Result<OwnedArchivedMap, Box<dyn StdError>> {
            let raw = std::fs::read(path)?;
            OwnedArchivedMap::new(raw.into_boxed_slice()).map_err(Into::into)
        }

        pub(super) fn write<'a>(
            pdm: &PlainProgramDependenceMap,
            file: File,
        ) -> Result<(), Box<dyn StdError>> {
            rkyv::api::high::to_bytes_in::<_, Error>(pdm, rkyv::ser::writer::IoWriter::new(file))
                .map(|_| ())
                .map_err(Box::<dyn StdError>::from)
        }
    }

    #[cfg(info_db_fmt = "json")]
    pub type LoadedTypeDatabase = TypesData;
    #[cfg(info_db_fmt = "rkyv")]
    pub type LoadedProgramDepMap = rkyving::OwnedArchivedMap;

    #[cfg(info_db_fmt = "json")]
    pub const FILENAME_MAP: &str = serdes::FILENAME;

    // #[cfg(info_db_fmt = "rkyv")]
    pub const FILENAME_MAP: &str = rkyving::FILENAME;

    pub fn read_program_dep_map() -> Result<LoadedProgramDepMap, Box<dyn StdError>> {
        log_info!("Finding and reading dependence map");

        let path = crate::utils::search_current_ancestor_dirs_for(FILENAME_MAP)
            .ok_or_else(|| Box::<dyn StdError>::from("Failed to find program dependence map"))?;

        #[cfg(info_db_fmt = "json")]
        let result = serdes::read();
        #[cfg(info_db_fmt = "rkyv")]
        let result = rkyving::read(path);
        result
    }

    pub fn write_program_dep_map<'a>(
        pdm: &PlainProgramDependenceMap,
        out_dir: impl AsRef<Path>,
    ) -> Result<(), Box<dyn StdError>> {
        log_info!(
            "Writing program dependence map in: `{}`",
            out_dir.as_ref().display()
        );

        let create_file = |name: &str| {
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(out_dir.as_ref().join(name))
                .map_err(Box::<dyn StdError>::from)
        };

        if cfg!(debug_assertions) {
            serdes::write(pdm, create_file(serdes::FILENAME)?)?;
        }

        let result = if cfg!(info_db_fmt = "json") {
            serdes::write(pdm, create_file(serdes::FILENAME)?)?;
        } else if cfg!(info_db_fmt = "rkyv") {
            rkyving::write(pdm, create_file(rkyving::FILENAME)?)?;
        } else {
            unreachable!()
        };

        Ok(result)
    }
}
