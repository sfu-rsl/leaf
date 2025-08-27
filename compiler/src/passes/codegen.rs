use rustc_hir::{def::DefKind, def_id::LOCAL_CRATE};
use rustc_middle::{
    middle::codegen_fn_attrs::CodegenFnAttrFlags,
    mir::mono::{CodegenUnit, MonoItem},
    ty::TyCtxt,
};

use common::{log_debug, log_info, log_warn};

use crate::config::InternalizationRules;

use super::{CompilationPass, StorageExt};

const KEY_RULES: &str = "internalization_rules";
const KEY_BAKED_RULES: &str = "internalization_rules_baked";

#[derive(Debug)]
pub(crate) struct MonoItemInternalizer {
    rules: Option<InternalizationRules>,
}

impl MonoItemInternalizer {
    pub fn new(rules: InternalizationRules) -> Self {
        Self { rules: Some(rules) }
    }
}

impl CompilationPass for MonoItemInternalizer {
    fn override_flags() -> super::OverrideFlags {
        super::OverrideFlags::COLLECT_PARTITION
    }

    fn visit_tcx_at_codegen_before<'tcx>(
        &mut self,
        _tcx: TyCtxt<'tcx>,
        storage: &mut dyn super::Storage,
    ) {
        storage.get_or_insert_with(KEY_RULES.to_owned(), || self.rules.take().unwrap());
    }

    fn visit_codegen_units<'tcx>(
        tcx: TyCtxt<'tcx>,
        units: &mut [CodegenUnit<'tcx>],
        storage: &mut dyn super::Storage,
    ) {
        log_info!(
            "Internalizing items for crate `{}`",
            tcx.crate_name(LOCAL_CRATE)
        );

        if units.len() > 1 {
            log_warn!(
                "Item internalization can break linking if there are multiple codegen units. Codegen units count: {}",
                units.len()
            )
        }

        let rules = storage.get_or_insert_with_acc(KEY_BAKED_RULES.to_owned(), |storage| {
            let rules = storage.get_or_default::<InternalizationRules>(KEY_RULES.to_owned());
            rules.to_baked()
        });

        for unit in units {
            unit.items_mut().iter_mut().for_each(|(item, data)| {
                if should_be_internalized(tcx, item, |name| rules.accept(name)) {
                    data.linkage = rustc_hir::attrs::Linkage::Internal;
                } else {
                    log_debug!("Not internalizing item: {:?}", item.def_id());
                }
            });
        }
    }
}

fn should_be_internalized<'tcx>(
    tcx: TyCtxt<'tcx>,
    item: &MonoItem<'tcx>,
    rules: impl FnOnce(&str) -> Option<bool>,
) -> bool {
    // pub extern "C" functions. Example: `__rdl_alloc`
    let def_id = item.def_id();
    if matches!(tcx.def_kind(def_id), DefKind::Fn)
        && matches!(
            tcx.fn_sig(def_id).instantiate_identity().abi(),
            rustc_abi::ExternAbi::C { .. }
        )
        && tcx.visibility(def_id).is_public()
    {
        false
    }
    // Weak lang items. Example: `rust_begin_unwind`
    else if tcx
        .lang_items()
        .iter()
        .any(|(l_item, id)| l_item.is_weak() && id == def_id)
    {
        false
    }
    // Rustc internal symbols. Example: `__rg_oom`
    else if tcx
        .codegen_fn_attrs(def_id)
        .flags
        .contains(CodegenFnAttrFlags::RUSTC_STD_INTERNAL_SYMBOL)
    {
        false
    }
    /* Special case for exported symbols of proc-macro crates.
     * Note: In an ideal workflow, proc-macro crates should not be compiled with leaf,
     * but we make the compiler robust enough for this case. */
    else if item.symbol_name(tcx).name
        == tcx
            .sess
            .generate_proc_macro_decls_symbol(tcx.stable_crate_id(LOCAL_CRATE))
    {
        false
    } else if let Some(ruled) = rules(item.symbol_name(tcx).name) {
        log_info!(
            "Internalization rule applied for: {:?} -> {}",
            item.def_id(),
            ruled,
        );
        ruled
    }
    // Everything else
    else {
        true
    }
}
