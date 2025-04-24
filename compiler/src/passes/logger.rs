use rustc_middle::{mir, ty::TyCtxt};

use crate::utils::mir::TyCtxtExt;

use super::{Compilation, CompilationPass, Storage};

/// A wrapper pass that logs pass methods.
pub(crate) struct LoggerPass<T> {
    pass: T,
}
use common::log_debug;

pub(crate) const TAG_OBJECTS: &str = "pass_objects";

macro_rules! target {
    () => {{ std::any::type_name::<T>() }};
}

impl<T> CompilationPass for LoggerPass<T>
where
    T: CompilationPass,
{
    fn override_flags() -> super::OverrideFlags {
        log_debug!("target: {} Getting override flags", target!());
        T::override_flags()
    }

    fn visit_ast_before(
        &mut self,
        krate: &super::ast::Crate,
        storage: &mut dyn Storage,
    ) -> Compilation {
        log_debug!(
            "target: {} Visiting AST before transformation {}",
            target!(),
            krate.id
        );
        log_debug!(target: TAG_OBJECTS, "AST: {:#?}", krate);
        self.pass.visit_ast_before(krate, storage)
    }

    fn visit_ast_after(
        &mut self,
        krate: &super::ast::Crate,
        storage: &mut dyn Storage,
    ) -> Compilation {
        // log_debug!(target: target!(), "Visiting AST after transformation {}", krate.id);
        log_debug!(
            "target: {} Visiting AST after transformation {}",
            target!(),
            krate.id
        );

        log_debug!(target: TAG_OBJECTS, "AST: {:#?}", krate);
        self.pass.visit_ast_after(krate, storage)
    }

    fn visit_tcx_after_analysis(&mut self, tcx: TyCtxt, storage: &mut dyn Storage) -> Compilation {
        log_debug!("target: {} Visiting TyCtxt after analysis", target!());
        self.pass.visit_tcx_after_analysis(tcx, storage)
    }

    fn visit_tcx_at_codegen_before(&mut self, tcx: TyCtxt, storage: &mut dyn Storage) {
        log_debug!("target: {} Visiting TyCtxt before codegen", target!());
        self.pass.visit_tcx_at_codegen_before(tcx, storage)
    }

    fn visit_tcx_at_codegen_after(&mut self, tcx: TyCtxt, storage: &mut dyn Storage) {
        log_debug!("target: {} Visiting TyCtxt after codegen", target!());
        self.pass.visit_tcx_at_codegen_after(tcx, storage)
    }

    fn visit_mir_body_before<'tcx>(
        tcx: TyCtxt<'tcx>,
        body: &mir::Body<'tcx>,
        storage: &mut dyn Storage,
    ) {
        log_debug!(
            "target: {} Visiting MIR body before transformation {:#?}",
            target!(),
            body.source.def_id(),
        );
        log_debug!(
            target: TAG_OBJECTS,
            "MIR body of {:?} (before):\n{}",
            body.source.def_id(),
            tcx.pretty_mir(body),
        );
        T::visit_mir_body_before(tcx, body, storage)
    }

    fn visit_mir_body_after<'tcx>(
        tcx: TyCtxt<'tcx>,
        body: &mir::Body<'tcx>,
        storage: &mut dyn Storage,
    ) {
        log_debug!(
            "target: {} Visiting MIR body after transformation {:#?}",
            target!(),
            body.source.def_id(),
        );
        log_debug!(
            target: TAG_OBJECTS,
            "MIR body {:?} (after):\n{}",
            body.source.def_id(),
            tcx.pretty_mir(body),
        );
        T::visit_mir_body_after(tcx, body, storage)
    }

    fn transform_ast(
        &mut self,
        session: &rustc_session::Session,
        krate: &mut rustc_ast::Crate,
        storage: &mut dyn Storage,
    ) {
        log_debug!("target: {} Transforming AST", target!());
        log_debug!(target: TAG_OBJECTS, "AST to transform: {:#?}", krate);
        self.pass.transform_ast(session, krate, storage)
    }

    fn transform_mir_body<'tcx>(
        tcx: TyCtxt<'tcx>,
        body: &mut mir::Body<'tcx>,
        storage: &mut dyn Storage,
    ) {
        log_debug!("target: {} Transforming MIR body", target!());
        log_debug!(
            target: TAG_OBJECTS,
            "MIR body to transform:\n{}",
            tcx.pretty_mir(body),
        );
        log_debug!(target: TAG_OBJECTS, "Storage: {:#?}", storage);
        T::transform_mir_body(tcx, body, storage)
    }

    fn visit_codegen_units<'tcx>(
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
        units: &mut [mir::mono::CodegenUnit<'tcx>],
        storage: &mut dyn Storage,
    ) {
        log_debug!("target: {} Visiting codegen units", target!());
        log_debug!(target: TAG_OBJECTS, "Storage: {:#?}", storage);
        T::visit_codegen_units(tcx, units, storage)
    }
}

pub(crate) trait CompilationPassLogExt {
    fn into_logged(self) -> LoggerPass<Self>
    where
        Self: Sized;
}
impl<T: CompilationPass> CompilationPassLogExt for T {
    fn into_logged(self) -> LoggerPass<T>
    where
        Self: Sized,
    {
        LoggerPass { pass: self }
    }
}
