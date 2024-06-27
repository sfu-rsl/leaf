#![feature(macro_metavar_expr)]
#![feature(core_intrinsics)]
#![no_std]

#[cfg(feature = "std")]
extern crate std;
#[cfg(feature = "std")]
use std::prelude::rust_2021::*;

#[cfg(feature = "config")]
pub mod config;
pub mod ffi;
#[cfg(feature = "logging")]
pub mod logging;
pub mod pri;
#[cfg(feature = "tyexp")]
pub mod tyexp;
pub mod types;
pub mod utils;

// NOTE: For compatibility when embedded in the core library.
pub mod leaf {
    pub use crate as common;
}
