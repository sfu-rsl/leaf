#![feature(likely_unlikely)]
#![feature(associated_type_defaults)]
#![feature(macro_metavar_expr)]
#![feature(box_patterns)]
#![feature(fn_traits)]
#![feature(unboxed_closures)]
#![feature(more_qualified_paths)]
#![feature(iter_map_windows)]
#![feature(seek_stream_len)]
#![feature(btree_cursors)]

pub(crate) mod cf_tracer;
pub(crate) mod mdsan;
pub(crate) mod symex;
mod utilsx;

use leaf_runtime::{
    abs, call, init, make_late_init_pri_of, memory, outgen, pri, solvers, trace, type_info, utils,
};

type SymExPri = pri::fluent::FluentPri<self::symex::SymExInstanceManager>;
type CftPri = pri::fluent::FluentPri<self::cf_tracer::CftInstanceManager>;
type MdSanPri = pri::fluent::FluentPri<self::mdsan::MdSanInstanceManager>;

make_late_init_pri_of!(SymExPri);

make_late_init_pri_of!(CftPri);

make_late_init_pri_of!(MdSanPri);
