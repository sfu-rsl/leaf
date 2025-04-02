#![feature(macro_metavar_expr)]
#![feature(core_intrinsics)]
#![feature(ptr_metadata)]
#![feature(const_type_id)]
#![cfg_attr(not(core_build), feature(register_tool))]
#![cfg_attr(not(core_build), register_tool(leaf_attr))]
#![no_std]

#[cfg(feature = "std")]
extern crate std;
#[cfg(feature = "std")]
use std::prelude::rust_2021::*;

#[cfg(feature = "config")]
pub mod config;
#[cfg(feature = "logging")]
pub mod logging;
#[cfg(feature = "serde")]
mod serdex;
#[cfg(feature = "tyexp")]
pub mod tyexp;

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
