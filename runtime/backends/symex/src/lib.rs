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

pub(crate) mod symex;

use leaf_runtime::init;

type SymExPri = leaf_runtime::pri::fluent::FluentPri<self::symex::SymExInstanceManager>;

leaf_runtime::make_late_init_pri_of!(SymExPri);
