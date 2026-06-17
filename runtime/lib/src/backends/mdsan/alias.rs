use crate::abs;

pub(super) use crate::backends::mdsan as backend;

use backend::MdTypeProvider;

pub(super) trait TypeDatabase: abs::backend::TypeDatabase<'static> + MdTypeProvider {}
impl<T> TypeDatabase for T where T: abs::backend::TypeDatabase<'static> + MdTypeProvider {}
