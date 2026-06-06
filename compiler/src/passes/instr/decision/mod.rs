mod intrinsics;
pub(super) mod rules;

use const_format::concatcp;

use rustc_hir::{def_id::DefId, definitions::DefPathData};
use rustc_middle::{
    mir::Body,
    ty::{InstanceKind, TyCtxt},
};
use rustc_span::Symbol;

use common::{log_debug, log_info, log_warn};

use crate::{passes::Storage, utils::mir::TyCtxtExt};

use super::config::WholeBodyFilter;

pub(super) const TAG_INSTR_DECISION: &str = concatcp!(super::TAG_INSTRUMENTATION, "::decision");

const TOOL_NAME: &str = crate::constants::TOOL_LEAF;
const ATTR_NAME: &str = "instrument";

pub(super) use intrinsics::{
    AtomicIntrinsicKind, IntrinsicDecision, MemoryIntrinsicKind, decide_intrinsic_call,
};

pub(super) fn should_instrument<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mut Body<'tcx>,
    storage: &mut dyn Storage,
) -> bool {
    let def_id = body.source.def_id();

    if !decide_instance_kind(&body.source.instance) {
        return false;
    }

    if is_lang_start_item(tcx, def_id) {
        return false;
    }

    // To be removed once we ensure it is working correctly.
    if false && is_drop_fn(tcx, def_id) {
        return false;
    }

    // Some intrinsic functions have body.
    if tcx.intrinsic(def_id).is_some() {
        return false;
    }

    rules::bake_rules(storage, get_exceptional_exclusions);
    let rules = rules::get_baked_body_rules(storage);
    if let Some((decision, item)) =
        find_inheritable_first_filtered(tcx, def_id, move |tcx, def_id| {
            rules.accept(&(tcx, def_id))
        })
    {
        log_debug!(
            target: TAG_INSTR_DECISION,
            "Found a rule for instrumentation of {:?} on {:?} with decision: {}",
            def_id,
            item,
            decision
        );
        return decision;
    }

    true
}

fn decide_instance_kind(kind: &InstanceKind) -> bool {
    use InstanceKind::*;
    match kind {
        Item(..)
        | FnPtrShim(..)
        | ClosureOnceShim { .. }
        | CloneShim(..)
        | ReifyShim(..)
        | DropGlue(_, Some(..)) => true,
        Intrinsic(..)
        | VTableShim(..)
        | Virtual(..)
        | ConstructCoroutineInClosureShim { .. }
        | ThreadLocalShim(..)
        | FutureDropPollShim(..)
        | FnPtrAddrShim(..)
        | AsyncDropGlue(..)
        | DropGlue(_, None)
        | AsyncDropGlueCtorShim(..) => false,
    }
}

/// Returns a set of filters to exclude some functions (mostly in the standard library)
/// that are currently problematic to instrument.
fn get_exceptional_exclusions() -> Vec<WholeBodyFilter> {
    use super::config::{CrateFilter, EntityLocationFilter};
    use crate::config::rules::*;

    fn def_path_pattern(pattern: &str) -> EntityLocationFilter {
        EntityLocationFilter::DefPathMatch(PatternMatch::from(pattern.to_owned()))
    }

    fn crate_name(name: &str) -> EntityLocationFilter {
        EntityLocationFilter::Crate(CrateFilter::Name(PatternMatch::from(name.to_owned())))
    }

    use LogicFormula::*;
    vec![
        Atom(def_path_pattern(".*panicking.*")),
        Any(AnyFormula::from(vec![
            Atom(def_path_pattern(".*core_arch.*")),
            All(vec![Atom(crate_name("core")), Atom(def_path_pattern(".*arch.*"))].into()),
        ])),
        All(AllFormula::from(vec![
            Atom(crate_name("std")),
            Any(vec![
                Atom(def_path_pattern(".*thread.*")),
                Atom(def_path_pattern(".*sync.*")),
                Atom(def_path_pattern(".*arch.*")),
            ]
            .into()),
        ])),
    ]
    .into_iter()
    .map(Into::into)
    .collect()
}

fn find_inheritable_first_filtered<'tcx>(
    tcx: TyCtxt<'tcx>,
    def_id: DefId,
    rules: impl Fn(TyCtxt<'tcx>, DefId) -> Option<bool>,
) -> Option<(bool, DefId)> {
    let mut current = def_id;
    loop {
        // Attributes take precedence over filters.
        if let Some(explicit) = opt_instrument_attr(tcx, current) {
            log_info!(
                target: TAG_INSTR_DECISION,
                "Found explicit instrumentation attribute for {:?} on {:?} with value: {}",
                def_id,
                current,
                explicit
            );
            return Some((explicit, current));
        }

        if let Some(include) = rules(tcx, current) {
            return Some((include, current));
        }

        let parent = tcx.opt_parent(current);
        current = match parent {
            Some(parent) => parent,
            None => return None,
        };
    }
}

/// Returns the value of the `instrument` attribute if it is placed on the item.
/// If the attribute is not found, or the argument passed to the attribute is invalid
/// returns `None`.
fn opt_instrument_attr<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> Option<bool> {
    use rustc_hir::{AttrArgs, Attribute};
    // Avoid possibly problematic const items.
    // See https://github.com/rust-lang/rust/issues/128145
    if matches!(
        tcx.def_key(def_id).disambiguated_data.data,
        DefPathData::AnonConst
    ) {
        return None;
    }

    tcx.get_attrs_by_path(
        def_id,
        &[Symbol::intern(TOOL_NAME), Symbol::intern(ATTR_NAME)],
    )
    .next()
    .and_then(|attr| match attr {
        Attribute::Unparsed(attr) => Some(attr),
        _ => None,
    })
    .and_then(|attr| match &attr.args {
        AttrArgs::Delimited(delim_args) => Some(delim_args.tokens.iter().next().cloned()),
        AttrArgs::Empty | AttrArgs::Eq { .. } => None,
    })
    .and_then(|token| {
        match token {
            // No argument means it's enabled.
            None => Some(true),
            Some(token) => {
                let as_bool = match &token {
                    rustc_ast::tokenstream::TokenTree::Token(token, ..) => token
                        .is_bool_lit()
                        .then(|| token.ident().unwrap().0.name == rustc_span::symbol::kw::True),
                    _ => None,
                };
                if as_bool.is_none() {
                    log_warn!(
                        "Invalid argument for attribute `{}`: {:?}",
                        ATTR_NAME,
                        token
                    );
                }
                as_bool
            }
        }
    })
}

fn is_lang_start_item(tcx: TyCtxt<'_>, def_id: DefId) -> bool {
    // It is in the module defining lang_start items (std rt module)
    tcx.lang_items()
        .start_fn()
        .map(|id| tcx.module_of(id).collect::<Vec<_>>())
        .zip(Some(tcx.module_of(def_id)))
        .is_some_and(|(start_mod, this_mod)| {
            let n = start_mod.len();
            start_mod.into_iter().eq(this_mod.take(n))
        })
}

fn is_drop_fn(tcx: TyCtxt<'_>, def_id: DefId) -> bool {
    let mut drop_fn_ids = {
        use rustc_hir::LanguageItems as Items;
        [Items::drop_in_place_fn, Items::async_drop_in_place_fn]
            .iter()
            .filter_map(|item| item(tcx.lang_items()))
    };
    drop_fn_ids.any(|id| id == def_id)
        || tcx
            .lang_items()
            .drop_trait()
            .zip(
                tcx.impl_of_assoc(def_id)
                    .and_then(|id| tcx.impl_opt_trait_id(id)),
            )
            .is_some_and(|(t1, t2)| t1 == t2)
}
