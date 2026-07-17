#![feature(likely_unlikely)]
#![feature(associated_type_defaults)]
#![feature(unboxed_closures)]

pub(crate) mod mdsan;

use leaf_runtime::init;

type MdSanPri = leaf_runtime::pri::fluent::FluentPri<self::mdsan::MdSanInstanceManager>;

leaf_runtime::make_late_init_pri_of!(MdSanPri);
