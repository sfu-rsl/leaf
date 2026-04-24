use std::collections::{HashMap, HashSet};

use rustc_middle::{
    mir::{TerminatorKind, mono::MonoItem},
    ty::{EarlyBinder, Instance, TyCtxt, TyKind, TypingEnv},
};
use rustc_span::{Span, source_map::Spanned};

use common::log_info;

use crate::passes::{CompilationPass, Storage, StorageExt};

use super::KEY_PRI_ITEMS;

#[derive(Default)]
pub(crate) struct InstrumentationRecursionChecker;

impl CompilationPass for InstrumentationRecursionChecker {
    fn override_flags() -> super::OverrideFlags {
        super::OverrideFlags::MAKE_CODEGEN_BACKEND
    }

    fn visit_tcx_at_codegen_after(
        &mut self,
        tcx: rustc_middle::ty::TyCtxt,
        storage: &mut dyn Storage,
    ) {
        let pri_items =
            storage.get_or_insert_with(KEY_PRI_ITEMS.to_owned(), || super::make_pri_items(tcx));

        let all_available_instances = tcx
            .collect_and_partition_mono_items(())
            .codegen_units
            .iter()
            .flat_map(|unit| {
                unit.items().iter().filter_map(|(item, _)| match item {
                    MonoItem::Fn(instance) => Some(*instance),
                    MonoItem::GlobalAsm(..) | MonoItem::Static(..) => None,
                })
            })
            .collect::<HashSet<_>>();

        let all_pri_instances = tcx
            .collect_and_partition_mono_items(())
            .codegen_units
            .iter()
            .flat_map(|unit| {
                unit.items().iter().filter_map(|(item, _)| match item {
                    MonoItem::Fn(instance) => pri_items
                        .all_items
                        .contains(&instance.def_id())
                        .then_some(*instance),
                    MonoItem::GlobalAsm(..) | MonoItem::Static(..) => None,
                })
            })
            .collect::<HashSet<_>>();

        let mut to_visit = all_pri_instances;
        let mut visited = HashSet::new();
        let mut call_sites: HashMap<_, Vec<Span>> = HashMap::new();
        while let Some(caller) = { to_visit.iter().next().copied() } {
            to_visit.remove(&caller);
            if visited.contains(&caller) {
                continue;
            }
            visited.insert(caller);

            for Spanned { node: callee, span } in collect_called_instances(tcx, caller) {
                {
                    call_sites.entry(callee).or_default().push(span);

                    if pri_items.all_items.contains(&callee.def_id())
                        && !pri_items.all_items.contains(&caller.def_id())
                    {
                        panic!(
                            "Recursion observed in instrumentation: {} was called by: {}, which itself called at: {:?}",
                            callee,
                            caller,
                            call_sites.get(&caller)
                        )
                    }

                    if all_available_instances.contains(&callee) {
                        to_visit.insert(callee);
                    }
                }
            }
        }

        log_info!(
            "Recursion check passed for {} instances and {} call sites",
            visited.len(),
            call_sites.values().map(|v| v.len()).sum::<usize>()
        );
    }
}

fn collect_called_instances<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
) -> Vec<Spanned<Instance<'tcx>>> {
    let body = tcx.instance_mir(instance.def);
    body.basic_blocks
        .iter()
        .map(|bb| bb.terminator())
        .filter_map(|t| match &t.kind {
            TerminatorKind::Call { func, fn_span, .. }
            | TerminatorKind::TailCall { func, fn_span, .. } => {
                let callee_ty = func.ty(body, tcx);
                let callee_ty = instance.instantiate_mir_and_normalize_erasing_regions(
                    tcx,
                    TypingEnv::fully_monomorphized(),
                    EarlyBinder::bind(callee_ty),
                );
                if let TyKind::FnDef(def_id, args) = *callee_ty.kind() {
                    Instance::try_resolve(tcx, TypingEnv::fully_monomorphized(), def_id, args)
                        .ok()
                        .flatten()
                        .map(|instance| Spanned {
                            node: instance,
                            span: *fn_span,
                        })
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect()
}
