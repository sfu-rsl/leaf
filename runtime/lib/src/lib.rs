#![feature(associated_type_defaults)]
#![feature(box_patterns)]
#![feature(assert_matches)]
#![feature(iterator_try_collect)]
#![feature(macro_metavar_expr)]
#![feature(result_flattening)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(btree_cursors)]
#![feature(core_intrinsics)]
#![feature(iter_map_windows)]
#![feature(path_add_extension)]
#![feature(seek_stream_len)]
#![feature(try_trait_v2)]

pub mod abs;
mod backends;
pub(crate) mod outgen;
pub mod pri;
pub(crate) mod solvers;
pub(crate) mod trace;
pub mod type_info;
pub(crate) mod utils;
use common::log_info;

fn init() {
    utils::logging::init_logging();
    log_info!("Initializing runtime library");
}
