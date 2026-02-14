#![feature(associated_type_defaults)]
#![feature(box_patterns)]
#![feature(assert_matches)]
#![feature(iterator_try_collect)]
#![feature(macro_metavar_expr)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(btree_cursors)]
#![feature(core_intrinsics)]
#![feature(iter_map_windows)]
#![feature(seek_stream_len)]
#![feature(try_trait_v2)]
#![feature(cold_path)]
#![feature(more_qualified_paths)]
#![feature(likely_unlikely)]

pub mod abs;
mod backends;
mod call;
pub(crate) mod outgen;
pub mod pri;
pub(crate) mod solvers;
pub(crate) mod trace;
pub mod type_info;
pub(crate) mod utils;

use common::log_info;

fn init<L: utils::logging::LeafTracingSubLayerFactory>() {
    utils::logging::init_logging::<L>();
    log_info!("Initializing runtime library");
}

pub type BasicPri = pri::fluent::FluentPri<backends::basic::BasicInstanceManager>;
