#![feature(likely_unlikely)]
#![feature(unboxed_closures)]

pub(crate) mod cf_tracer;

use leaf_runtime::init;

type CftPri = leaf_runtime::pri::fluent::FluentPri<self::cf_tracer::CftInstanceManager>;

leaf_runtime::make_late_init_pri_of!(CftPri);
