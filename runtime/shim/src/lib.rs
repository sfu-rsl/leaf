#![cfg_attr(not(core_build), feature(concat_idents))]
#![cfg_attr(not(core_build), feature(const_eval_select))]
#![cfg_attr(not(core_build), feature(core_intrinsics))]
#![cfg_attr(not(core_build), feature(fn_ptr_trait))]
#![cfg_attr(not(core_build), feature(ptr_metadata))]
#![cfg_attr(not(core_build), feature(coerce_unsized))]
#![cfg_attr(not(core_build), no_std)]
#![cfg_attr(not(core_build), feature(rustc_attrs))]
#![cfg_attr(not(core_build), feature(register_tool))]
#![cfg_attr(not(core_build), register_tool(leaf_attr))]
// -----
#![cfg_attr(core_build, allow(missing_docs))]
#![cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
pub mod annotations;

#[cfg_attr(core_build, stable(feature = "rust1", since = "1.0.0"))]
#[leaf_attr::instrument(false)]
pub mod pri;

#[cfg(core_build)]
use super::common;
#[cfg(not(core_build))]
use common;
