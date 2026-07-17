type SymExPri = leaf_runtime::pri::fluent::FluentPri<super::instance::SymExInstanceManager>;

leaf_runtime::make_late_init_pri_of!(SymExPri);

pub type DefaultPri = SymExPriLateInit;
