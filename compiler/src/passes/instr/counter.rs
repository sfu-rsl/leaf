use std::collections::{HashMap, HashSet};

use rustc_hir::def_id::DefId;
use rustc_middle::{mir::Body, mono::MonoItem};

use crate::{
    passes::{CompilationPass, Storage, StorageExt},
    utils::file::TyCtxtFileExt,
};

use super::{KEY_PRI_ITEMS, called_pri_func, pri_utils::sym::LeafSymbol};

const FILE_OUTPUT: &str = "instr_counts.json";

#[derive(Default)]
pub(crate) struct InstrumentationCounter;

impl CompilationPass for InstrumentationCounter {
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

        let mut counts = HashMap::new();

        tcx.collect_and_partition_mono_items(())
            .codegen_units
            .iter()
            .for_each(|unit| {
                unit.items().iter().for_each(|(item, _)| match item {
                    MonoItem::Fn(instance) => {
                        let body = tcx.instance_mir(instance.def);
                        count_calls(&mut counts, body, &pri_items.all_items);
                    }
                    _ => {}
                })
            });

        let all_funcs = pri_items
            .funcs
            .iter()
            .map(|(s, info)| (*s, info.def_id))
            .chain(
                pri_items
                    .helper_funcs
                    .all_helpers
                    .iter()
                    .map(|(s, def_id)| (*s, *def_id)),
            )
            .collect::<HashMap<_, _>>();
        let counts = map_back_to_leaf_symbol(counts, &all_funcs);

        write_to_file(counts, tcx.output_dir());
    }
}

fn count_calls(counts: &mut HashMap<DefId, usize>, body: &Body, all_pri_funcs: &HashSet<DefId>) {
    body.basic_blocks.iter().for_each(|bb| {
        if let Some(terminator) = &bb.terminator {
            if let Some(func) = called_pri_func(&terminator.kind, all_pri_funcs) {
                *counts.entry(func).or_default() += 1;
            }
        }
    });
}

fn map_back_to_leaf_symbol(
    counts: HashMap<DefId, usize>,
    funcs: &HashMap<LeafSymbol, DefId>,
) -> HashMap<LeafSymbol, usize> {
    funcs
        .into_iter()
        .map(|(sym, info)| {
            let count = counts.get(info).copied().unwrap_or(0);
            (*sym, count)
        })
        .collect()
}

fn write_to_file(counts: HashMap<LeafSymbol, usize>, out_dir: impl AsRef<std::path::Path>) {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(out_dir.as_ref().join(FILE_OUTPUT))
        .expect("Failed to open file for writing instrumentation counts");

    serde_json::to_writer_pretty(
        file,
        &counts
            .into_iter()
            .map(|(sym, count)| (sym.to_string(), count))
            .collect::<HashMap<_, _>>(),
    )
    .expect("Failed to write instrumentation counts to file");
}
