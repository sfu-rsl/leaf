use rustc_middle::{mir, ty::TyCtxt};

use crate::passes::StorageExt;

use super::{Compilation, CompilationPass, Storage};

/// A wrapper pass that enables or disables the inner pass.
pub(crate) struct GatedPass<T> {
    enabled: bool,
    pass: T,
}

impl<T> GatedPass<T> {
    pub fn new(pass: T, enabled: bool) -> Self {
        Self { pass, enabled }
    }
}

macro_rules! storage_key {
    () => {{ core::any::type_name::<T>().to_owned() + "_enabled" }};
}

impl<T> CompilationPass for GatedPass<T>
where
    T: CompilationPass,
{
    fn override_flags() -> super::OverrideFlags {
        T::override_flags()
    }

    fn visit_ast_before(
        &mut self,
        krate: &super::ast::Crate,
        storage: &mut dyn Storage,
    ) -> Compilation {
        storage.get_or_insert_with::<bool>(storage_key!(), || self.enabled);

        if !is_enabled::<T>(storage) {
            return Compilation::Continue;
        }

        self.pass.visit_ast_before(krate, storage)
    }

    fn visit_ast_after(
        &mut self,
        krate: &super::ast::Crate,
        storage: &mut dyn Storage,
    ) -> Compilation {
        if !is_enabled::<T>(storage) {
            return Compilation::Continue;
        }

        self.pass.visit_ast_after(krate, storage)
    }

    fn visit_tcx_after_analysis(&mut self, tcx: TyCtxt, storage: &mut dyn Storage) -> Compilation {
        if !is_enabled::<T>(storage) {
            return Compilation::Continue;
        }

        self.pass.visit_tcx_after_analysis(tcx, storage)
    }

    fn visit_tcx_at_codegen_before(&mut self, tcx: TyCtxt, storage: &mut dyn Storage) {
        if !is_enabled::<T>(storage) {
            return;
        }

        self.pass.visit_tcx_at_codegen_before(tcx, storage)
    }

    fn visit_tcx_at_codegen_after(&mut self, tcx: TyCtxt, storage: &mut dyn Storage) {
        if !is_enabled::<T>(storage) {
            return;
        }

        self.pass.visit_tcx_at_codegen_after(tcx, storage)
    }

    fn visit_mir_body_before<'tcx>(
        tcx: TyCtxt<'tcx>,
        body: &mir::Body<'tcx>,
        storage: &mut dyn Storage,
    ) {
        if !is_enabled::<T>(storage) {
            return;
        }

        T::visit_mir_body_before(tcx, body, storage)
    }

    fn visit_mir_body_after<'tcx>(
        tcx: TyCtxt<'tcx>,
        body: &mir::Body<'tcx>,
        storage: &mut dyn Storage,
    ) {
        if !is_enabled::<T>(storage) {
            return;
        }

        T::visit_mir_body_after(tcx, body, storage)
    }

    fn transform_ast(
        &mut self,
        session: &rustc_session::Session,
        krate: &mut rustc_ast::Crate,
        storage: &mut dyn Storage,
    ) {
        if !is_enabled::<T>(storage) {
            return;
        }

        self.pass.transform_ast(session, krate, storage)
    }

    fn transform_mir_body<'tcx>(
        tcx: TyCtxt<'tcx>,
        body: &mut mir::Body<'tcx>,
        storage: &mut dyn Storage,
    ) {
        if !is_enabled::<T>(storage) {
            return;
        }

        T::transform_mir_body(tcx, body, storage)
    }

    fn visit_codegen_units<'tcx>(
        tcx: TyCtxt<'tcx>,
        units: &mut [mir::mono::CodegenUnit<'tcx>],
        storage: &mut dyn Storage,
    ) {
        if !is_enabled::<T>(storage) {
            return;
        }

        T::visit_codegen_units(tcx, units, storage)
    }
}

#[inline]
fn is_enabled<T>(storage: &mut dyn Storage) -> bool {
    let key = storage_key!();
    *storage.get_mut(&key).unwrap()
}

pub(crate) trait CompilationPassLogExt {
    fn into_gated(self, enabled: bool) -> GatedPass<Self>
    where
        Self: Sized;
}
impl<T: CompilationPass> CompilationPassLogExt for T {
    fn into_gated(self, enabled: bool) -> GatedPass<T>
    where
        Self: Sized,
    {
        GatedPass::new(self, enabled)
    }
}
