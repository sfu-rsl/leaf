#![feature(associated_type_defaults)]
#![feature(box_patterns)]
#![feature(macro_metavar_expr)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(btree_cursors)]
#![allow(internal_features)]
#![feature(core_intrinsics)]
#![feature(iter_map_windows)]
#![feature(seek_stream_len)]
#![feature(more_qualified_paths)]
#![feature(likely_unlikely)]
#![feature(never_type)]

pub mod abs;
pub mod call;
pub mod memory;
pub mod outgen;
pub mod pri;
pub mod solvers;
pub mod trace;
pub mod type_info;
pub mod utils;

use common::log_info;

pub fn init<L: utils::logging::LeafTracingSubLayerFactory>() {
    utils::logging::init_logging::<L>();
    log_info!("Initializing runtime library");
}
