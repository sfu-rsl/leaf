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
mod backends;
mod call;
pub(crate) mod memory;
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

pub type SymExPri = pri::fluent::FluentPri<backends::symex::SymExInstanceManager>;
pub type CftPri = pri::fluent::FluentPri<backends::cf_tracer::CftInstanceManager>;
pub type MdSanPri = pri::fluent::FluentPri<backends::mdsan::MdSanInstanceManager>;
