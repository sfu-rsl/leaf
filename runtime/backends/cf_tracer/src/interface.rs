type CftPri = leaf_runtime::pri::fluent::FluentPri<super::instance::CftInstanceManager>;

leaf_runtime::make_late_init_pri_of!(CftPri);

pub type DefaultPri = CftPriLateInit;
