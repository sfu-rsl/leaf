type MdSanPri = leaf_runtime::pri::fluent::FluentPri<super::instance::MdSanInstanceManager>;

leaf_runtime::make_late_init_pri_of!(MdSanPri);

pub type DefaultPri = MdSanPriLateInit;
