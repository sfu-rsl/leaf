#![feature(macro_metavar_expr)]
#![feature(core_intrinsics)]
#![feature(ptr_metadata)]
#![feature(const_type_id)]
#![cfg_attr(not(core_build), feature(register_tool))]
#![cfg_attr(not(core_build), register_tool(leaf_attr))]
#![cfg_attr(not(core_build), feature(path_add_extension))]
#![no_std]

#[cfg(feature = "std")]
extern crate std;
#[cfg(feature = "std")]
use std::prelude::rust_2021::*;

#[cfg(feature = "answers")]
pub mod answers;
#[cfg(feature = "config")]
pub mod config;
#[cfg(feature = "directed")]
pub mod directed;
#[cfg(feature = "logging")]
pub mod logging;
#[cfg(feature = "rkyv")]
mod rkyving;
#[cfg(feature = "serde")]
mod serdes;
#[cfg(feature = "tyexp")]
pub mod tyexp;
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
