#![feature(associated_type_defaults)]
#![feature(box_patterns)]
#![feature(assert_matches)]
#![feature(iterator_try_collect)]
#![feature(macro_metavar_expr)]
#![feature(result_flattening)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(const_float_bits_conv)]
#![feature(btree_cursors)]
#![feature(strict_provenance)]
#![feature(core_intrinsics)]
#![feature(exposed_provenance)]

pub mod abs;
mod backends;
pub(crate) mod outgen;
pub(crate) mod pathics;
pub mod pri;
pub(crate) mod solvers;
pub(crate) mod trace;
pub mod tyexp;
pub(crate) mod utils;

fn init() {
    utils::logging::init_logging();
    log::info!("Initializing runtime library");
}
