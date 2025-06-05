#![feature(macro_metavar_expr)]
#![feature(core_intrinsics)]
#![feature(ptr_metadata)]
#![feature(const_type_id)]
#![feature(cold_path)]
#![feature(likely_unlikely)]
#![cfg_attr(not(core_build), feature(register_tool))]
#![cfg_attr(not(core_build), register_tool(leaf_attr))]
#![cfg_attr(all(not(core_build), feature = "answers"), feature(path_add_extension))]
#![cfg_attr(all(not(core_build), feature = "building"), feature(exit_status_error))]
#![no_std]

#[cfg(feature = "std")]
extern crate std;
#[cfg(feature = "std")]
use std::prelude::rust_2021::*;

#[cfg(feature = "answers")]
pub mod answers;
#[cfg(feature = "building")]
pub mod building;
#[cfg(feature = "conc_loop")]
pub mod conc_loop;
#[cfg(feature = "config")]
pub mod config;
#[cfg(feature = "directed")]
pub mod directed;
#[cfg(feature = "logging")]
pub mod logging;
#[cfg(feature = "program_dep")]
pub mod program_dep;
#[cfg(feature = "rkyv")]
mod rkyving;
#[cfg(feature = "serde")]
mod serdes;
#[cfg(feature = "type_info")]
pub mod type_info;
#[cfg(feature = "z3")]
pub mod z3;

#[leaf_attr::instrument(false)]
pub mod ffi;
#[leaf_attr::instrument(false)]
pub mod pri;
#[leaf_attr::instrument(false)]
pub mod types;
#[leaf_attr::instrument(false)]
pub mod utils;

// NOTE: For compatibility when embedded in the core library.
pub mod leaf {
    pub use crate as common;
}
