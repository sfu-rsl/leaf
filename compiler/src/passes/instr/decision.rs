use const_format::concatcp;

use rustc_hir::{def_id::DefId, definitions::DefPathData};
use rustc_middle::{
    mir::Body,
    ty::{InstanceKind, IntrinsicDef, TyCtxt},
};
use rustc_span::Symbol;

use common::{log_debug, log_info, log_warn};

use crate::{config::WholeBodyFilter, passes::Storage, utils::mir::TyCtxtExt};

pub(super) const TAG_INSTR_DECISION: &str = concatcp!(super::TAG_INSTRUMENTATION, "::decision");

const TOOL_NAME: &str = crate::constants::TOOL_LEAF;
const ATTR_NAME: &str = "instrument";

pub(super) use rules::{KEY_RULES, get_baked_dyn_def_rules};

pub(super) fn should_instrument<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &mut Body<'tcx>,
    storage: &mut dyn Storage,
) -> bool {
    let def_id = body.source.def_id();

    if !decide_instance_kind(&body.source.instance) {
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

    if is_lang_start_item(tcx, def_id) {
        return false;
    }

    // FIXME: Drops are important for bug detection, however we avoid instrumenting them for now.
    if is_drop_fn(tcx, def_id) {
        return false;
    }

    // Some intrinsic functions have body.
    if tcx.intrinsic(def_id).is_some() {
        return false;
    }

    true
}

fn decide_instance_kind(kind: &InstanceKind) -> bool {
    use InstanceKind::*;
    match kind {
        Item(..) | FnPtrShim(..) | ClosureOnceShim { .. } | CloneShim(..) | ReifyShim(..) => true,
        Intrinsic(..)
        | VTableShim(..)
        | Virtual(..)
        | ConstructCoroutineInClosureShim { .. }
        | ThreadLocalShim(..)
        | FutureDropPollShim(..)
        | DropGlue(..)
        | FnPtrAddrShim(..)
        | AsyncDropGlue(..)
        | AsyncDropGlueCtorShim(..) => false,
    }
}

/// Returns a set of filters to exclude some functions (mostly in the standard library)
/// that are currently problematic to instrument.
fn get_exceptional_exclusions() -> Vec<WholeBodyFilter> {
    use crate::config::{
        EntityLocationFilter,
        rules::{AllFormula, AnyFormula, LogicFormula::*, PatternMatch},
    };

    fn def_path_pattern(pattern: &str) -> EntityLocationFilter {
        EntityLocationFilter::DefPathMatch(PatternMatch::from(pattern.to_owned()))
    }

    fn crate_name(name: &str) -> EntityLocationFilter {
        EntityLocationFilter::Crate(crate::config::CrateFilter::Name(name.to_owned()))
    }

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
    use rustc_hir::{AttrArgs, AttrKind};
    // Avoid possibly problematic const items.
    // See https://github.com/rust-lang/rust/issues/128145
    if matches!(
        tcx.def_key(def_id).disambiguated_data.data,
        DefPathData::AnonConst
    ) {
        return None;
    }

    tcx.get_attrs_by_path(def_id, &[
        Symbol::intern(TOOL_NAME),
        Symbol::intern(ATTR_NAME),
    ])
    .next()
    .and_then(|attr| match &attr.kind {
        AttrKind::Normal(attr) => Some(attr),
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
        [
            Items::drop_in_place_fn,
            Items::async_drop_in_place_fn,
            Items::surface_async_drop_in_place_fn,
            Items::async_drop_surface_drop_in_place_fn,
            Items::async_drop_slice_fn,
            Items::async_drop_chain_fn,
            Items::async_drop_noop_fn,
            Items::async_drop_deferred_drop_in_place_fn,
            Items::async_drop_fuse_fn,
            Items::async_drop_defer_fn,
            Items::async_drop_either_fn,
        ]
        .iter()
        .filter_map(|item| item(tcx.lang_items()))
    };
    drop_fn_ids.any(|id| id == def_id)
        || tcx
            .lang_items()
            .drop_trait()
            .zip(
                tcx.impl_of_assoc(def_id)
                    .and_then(|id| tcx.trait_id_of_impl(id)),
            )
            .is_some_and(|(t1, t2)| t1 == t2)
}

mod intrinsics {
    use common::pri::{AtomicBinaryOp, AtomicOrdering};

    use crate::pri_utils::sym::intrinsics::LeafIntrinsicSymbol;

    use super::*;

    pub(crate) enum IntrinsicDecision {
        OneToOneAssign(LeafIntrinsicSymbol),
        Atomic(AtomicOrdering, AtomicIntrinsicKind),
        Memory {
            kind: MemoryIntrinsicKind,
            is_volatile: bool,
        },
        NoOp,
        ConstEvaluated,
        Contract,
        ToDo,
        NotPlanned,
        Unsupported,
        Unexpected,
    }

    pub(crate) enum AtomicIntrinsicKind {
        Load,
        Store,
        Exchange,
        CompareExchange {
            fail_ordering: AtomicOrdering,
            weak: bool,
        },
        BinOp(AtomicBinaryOp),
        Fence {
            single_thread: bool,
        },
    }

    pub(crate) enum MemoryIntrinsicKind {
        Load { is_ptr_aligned: bool },
        Store { is_ptr_aligned: bool },
        Copy { is_overlapping: bool },
        Set,
        Swap,
    }

    macro_rules! of_mir_translated_funcs {
        ($macro:ident) => {
            // These functions are expected to be translated to MIR elements and not appear at the
            // phase we perform the instrumentation.
            $macro!(
                transmute,
                transmute_unchecked,
                aggregate_raw_ptr,
                ptr_metadata,
                discriminant_value,
                offset,
                min_align_of,
                add_with_overflow,
                sub_with_overflow,
                mul_with_overflow,
                three_way_compare,
                size_of,
                unchecked_add,
                unchecked_div,
                unchecked_mul,
                unchecked_rem,
                unchecked_shl,
                unchecked_shr,
                unchecked_sub,
                wrapping_add,
                wrapping_mul,
                wrapping_sub,
                write_via_move,
                read_via_copy,
                ub_checks,
            )
        };
    }

    macro_rules! of_const_evaluated_funcs {
        ($macro:ident) => {
            // These functions are expected to be evaluated and not appear at the
            // phase we perform the instrumentation.
            $macro!(
                variant_count,
                type_name,
                type_id,
                ptr_guaranteed_cmp,
                pref_align_of,
                needs_drop,
                min_align_of_val,
                // FIXME: These two are probably not intrinsics anymore.
                likely,
                unlikely,
                forget,
                const_allocate,
                const_eval_select,
                const_deallocate,
                caller_location,
                assert_zero_valid,
                assert_mem_uninitialized_valid,
                assume,
                // contract_checks,
            )
        };
    }

    macro_rules! of_contract_funcs {
        ($macro:ident) => {
            $macro!(contract_check_requires, contract_check_ensures,)
        };
    }

    macro_rules! of_noop_funcs {
        ($macro:ident) => {
            $macro!(
                unreachable,
                rustc_peek,
                prefetch_write_instruction,
                prefetch_read_instruction,
                prefetch_write_data,
                prefetch_read_data,
                // const_deallocate,
                breakpoint,
                assert_inhabited,
                cold_path,
            )
        };
    }

    macro_rules! of_float_arith_funcs {
        ($macro:ident) => {
            $macro!(
                truncf16,
                truncf32,
                truncf64,
                truncf128,
                sqrtf128,
                sqrtf64,
                sqrtf16,
                sqrtf32,
                sinf128,
                sinf32,
                sinf16,
                sinf64,
                roundf32,
                roundf64,
                roundf128,
                roundf16,
                round_ties_even_f16,
                round_ties_even_f32,
                round_ties_even_f64,
                round_ties_even_f128,
                powif64,
                powif128,
                powif16,
                powif32,
                powf32,
                powf64,
                powf128,
                powf16,
                minimumf16,
                minimumf32,
                minimumf64,
                minimumf128,
                minnumf16,
                minnumf32,
                minnumf64,
                minnumf128,
                logf128,
                logf64,
                maximumf16,
                maximumf32,
                maximumf64,
                maximumf128,
                maxnumf16,
                maxnumf32,
                maxnumf64,
                maxnumf128,
                log10f128,
                log10f32,
                log10f16,
                logf16,
                log10f64,
                logf32,
                log2f128,
                log2f16,
                log2f32,
                log2f64,
                fsub_fast,
                frem_fast,
                frem_algebraic,
                fsub_algebraic,
                fmul_algebraic,
                fmul_fast,
                fmuladdf16,
                fmuladdf32,
                fmuladdf64,
                fmuladdf128,
                fmaf128,
                fmaf64,
                floorf64,
                floorf128,
                floorf32,
                fmaf32,
                fmaf16,
                floorf16,
                fdiv_fast,
                fadd_fast,
                fdiv_algebraic,
                fabsf32,
                fadd_algebraic,
                fabsf128,
                fabsf64,
                fabsf16,
                expf64,
                exp2f128,
                expf32,
                expf128,
                expf16,
                exp2f64,
                exp2f32,
                exp2f16,
                cosf128,
                cosf64,
                cosf32,
                copysignf32,
                copysignf64,
                copysignf128,
                cosf16,
                copysignf16,
                ceilf128,
                ceilf64,
                ceilf16,
                ceilf32,
                float_to_int_unchecked,
            )
        };
    }

    macro_rules! of_atomic_load_funcs {
        ($macro:ident) => {
            $macro!(
                atomic_load_acquire,
                atomic_load_relaxed,
                atomic_load_seqcst,
                atomic_load_unordered,
            )
        };
    }

    macro_rules! of_atomic_store_funcs {
        ($macro:ident) => {
            $macro!(
                atomic_store_relaxed,
                atomic_store_release,
                atomic_store_seqcst,
                atomic_store_unordered,
            )
        };
    }

    macro_rules! of_atomic_xchg_funcs {
        ($macro:ident) => {
            $macro!(
                atomic_xchg_acqrel,
                atomic_xchg_acquire,
                atomic_xchg_relaxed,
                atomic_xchg_release,
                atomic_xchg_seqcst,
            )
        };
    }

    macro_rules! of_atomic_cxchg_funcs {
        ($macro:ident) => {
            $macro!(
                atomic_cxchg_acqrel_acquire,
                atomic_cxchg_acqrel_relaxed,
                atomic_cxchg_acqrel_seqcst,
                atomic_cxchg_acquire_acquire,
                atomic_cxchg_acquire_relaxed,
                atomic_cxchg_acquire_seqcst,
                atomic_cxchg_relaxed_acquire,
                atomic_cxchg_relaxed_relaxed,
                atomic_cxchg_relaxed_seqcst,
                atomic_cxchg_release_acquire,
                atomic_cxchg_release_relaxed,
                atomic_cxchg_release_seqcst,
                atomic_cxchg_seqcst_acquire,
                atomic_cxchg_seqcst_relaxed,
                atomic_cxchg_seqcst_seqcst,
                atomic_cxchgweak_acqrel_acquire,
                atomic_cxchgweak_acqrel_relaxed,
                atomic_cxchgweak_acqrel_seqcst,
                atomic_cxchgweak_acquire_acquire,
                atomic_cxchgweak_acquire_relaxed,
                atomic_cxchgweak_acquire_seqcst,
                atomic_cxchgweak_relaxed_acquire,
                atomic_cxchgweak_relaxed_relaxed,
                atomic_cxchgweak_relaxed_seqcst,
                atomic_cxchgweak_release_acquire,
                atomic_cxchgweak_release_relaxed,
                atomic_cxchgweak_release_seqcst,
                atomic_cxchgweak_seqcst_acquire,
                atomic_cxchgweak_seqcst_relaxed,
                atomic_cxchgweak_seqcst_seqcst,
            )
        };
    }

    macro_rules! of_atomic_binop_funcs {
        ($macro:ident) => {
            $macro!(
                atomic_and_acqrel,
                atomic_and_acquire,
                atomic_and_relaxed,
                atomic_and_release,
                atomic_and_seqcst,
                atomic_max_acqrel,
                atomic_max_acquire,
                atomic_max_relaxed,
                atomic_max_release,
                atomic_max_seqcst,
                atomic_min_acqrel,
                atomic_min_acquire,
                atomic_min_relaxed,
                atomic_min_release,
                atomic_min_seqcst,
                atomic_nand_acqrel,
                atomic_nand_acquire,
                atomic_nand_relaxed,
                atomic_nand_release,
                atomic_nand_seqcst,
                atomic_or_acqrel,
                atomic_or_acquire,
                atomic_or_relaxed,
                atomic_or_release,
                atomic_or_seqcst,
                atomic_umax_acqrel,
                atomic_umax_acquire,
                atomic_umax_relaxed,
                atomic_umax_release,
                atomic_umax_seqcst,
                atomic_umin_acqrel,
                atomic_umin_acquire,
                atomic_umin_relaxed,
                atomic_umin_release,
                atomic_umin_seqcst,
                atomic_xadd_acqrel,
                atomic_xadd_acquire,
                atomic_xadd_relaxed,
                atomic_xadd_release,
                atomic_xadd_seqcst,
                atomic_xor_acqrel,
                atomic_xor_acquire,
                atomic_xor_relaxed,
                atomic_xor_release,
                atomic_xor_seqcst,
                atomic_xsub_acqrel,
                atomic_xsub_acquire,
                atomic_xsub_relaxed,
                atomic_xsub_release,
                atomic_xsub_seqcst,
            )
        };
    }

    macro_rules! of_atomic_fence_funcs {
        ($macro:ident) => {
            $macro!(
                atomic_fence_acqrel,
                atomic_fence_acquire,
                atomic_fence_release,
                atomic_fence_seqcst,
                atomic_singlethreadfence_acqrel,
                atomic_singlethreadfence_acquire,
                atomic_singlethreadfence_release,
                atomic_singlethreadfence_seqcst,
            )
        };
    }

    macro_rules! of_simd_op_funcs {
        ($macro:ident) => {
            $macro!(
                simd_add,
                simd_and,
                simd_arith_offset,
                simd_as,
                simd_bitmask,
                simd_bitreverse,
                simd_bswap,
                simd_cast,
                simd_cast_ptr,
                simd_ceil,
                simd_ctlz,
                simd_ctpop,
                simd_cttz,
                simd_div,
                simd_eq,
                simd_expose_provenance,
                simd_extract,
                simd_fabs,
                simd_fcos,
                simd_fexp,
                simd_fexp2,
                simd_flog,
                simd_flog2,
                simd_flog10,
                simd_floor,
                simd_fma,
                simd_fmax,
                simd_fmin,
                simd_fsin,
                simd_fsqrt,
                simd_gather,
                simd_ge,
                simd_gt,
                simd_insert,
                simd_le,
                simd_lt,
                simd_masked_load,
                simd_masked_store,
                simd_mul,
                simd_ne,
                simd_neg,
                simd_or,
                simd_reduce_add_ordered,
                simd_reduce_add_unordered,
                simd_reduce_all,
                simd_reduce_and,
                simd_reduce_any,
                simd_reduce_max,
                simd_reduce_min,
                simd_reduce_mul_ordered,
                simd_reduce_xor,
                simd_rem,
                simd_round,
                simd_saturating_add,
                simd_saturating_sub,
                simd_scatter,
                simd_select,
                simd_select_bitmask,
                simd_shl,
                simd_shr,
                simd_shuffle,
                simd_sub,
                simd_trunc,
                simd_with_exposed_provenance,
                simd_xor,
            )
        };
    }

    macro_rules! of_memory_funcs {
        ($macro:ident) => {
            $macro!(
                volatile_load,
                volatile_store,
                unaligned_volatile_load,
                unaligned_volatile_store,
                nontemporal_store,
                copy,
                copy_nonoverlapping,
                volatile_copy_nonoverlapping_memory,
                volatile_copy_memory,
                write_bytes,
                volatile_set_memory,
                typed_swap_nonoverlapping,
            )
        };
    }

    macro_rules! of_to_be_supported_funcs {
        ($macro:ident) => {
            $macro!(
                vtable_size,
                vtable_align,
                select_unpredictable,
                raw_eq,
                ptr_mask,
                ptr_offset_from_unsigned,
                ptr_offset_from,
                compare_bytes,
                catch_unwind,
                abort,
                drop_in_place,
                size_of_val,
                is_val_statically_known,
                arith_offset,
                carrying_mul_add,
            )
        };
    }

    macro_rules! of_one_to_one_funcs {
        ($macro:ident) => {
            $macro!(
                rotate_left,
                rotate_right,
                saturating_sub,
                saturating_add,
                exact_div,
                bitreverse,
                cttz_nonzero,
                cttz,
                ctpop,
                ctlz_nonzero,
                ctlz,
                bswap,
                // disjoint_bitor,
                black_box,
            )
        };
    }

    // To make sure all intrinsics are covered.
    mod sanity_check {
        #![allow(unused)]

        macro_rules! str_array {
            ($($intrinsic:ident),*$(,)?) => {
                [$(stringify!($intrinsic)),*]
            };
        }

        macro_rules! count_all {
            ($($macro:ident),*$(,)?) => {
                0 $(+count($macro!(str_array)))*
            };
        }

        const fn count<const N: usize>(_: [&str; N]) -> usize {
            N
        }

        const TOTAL_COUNT: usize = count_all!(
            of_mir_translated_funcs,
            of_const_evaluated_funcs,
            of_noop_funcs,
            of_float_arith_funcs,
            of_atomic_load_funcs,
            of_atomic_store_funcs,
            of_atomic_xchg_funcs,
            of_atomic_binop_funcs,
            of_atomic_cxchg_funcs,
            of_atomic_fence_funcs,
            of_simd_op_funcs,
            of_to_be_supported_funcs,
            of_one_to_one_funcs,
            of_memory_funcs,
        );

        /* NTOE: This is used as a test to make sure that the list do not contain duplicates.
         * Do not change the count unless some intrinsics are added or removed to Rust.
         */
        const _ALL_INTRINSICS: [u8; 360] = [0; TOTAL_COUNT];
    }

    use crate::pri_utils::sym::intrinsics as psym;
    use rustc_span::sym as rsym;

    pub(crate) fn decide_intrinsic_call<'tcx>(intrinsic: IntrinsicDef) -> IntrinsicDecision {
        macro_rules! any_of {
            ($($intrinsic:ident),*$(,)?) => {
                $(rsym::$intrinsic)|*
            };
        }

        match intrinsic.name {
            of_one_to_one_funcs!(any_of) => decide_one_to_one_intrinsic_call(intrinsic),
            of_noop_funcs!(any_of) => IntrinsicDecision::NoOp,
            // of_contract_funcs!(any_of) => IntrinsicDecision::Contract,
            of_const_evaluated_funcs!(any_of) => IntrinsicDecision::ConstEvaluated,
            of_to_be_supported_funcs!(any_of) => IntrinsicDecision::ToDo,
            of_float_arith_funcs!(any_of) => IntrinsicDecision::NotPlanned,
            of_mir_translated_funcs!(any_of) => IntrinsicDecision::Unexpected,
            of_simd_op_funcs!(any_of) => IntrinsicDecision::Unsupported,
            other if other.as_str().starts_with("atomic") => {
                decide_atomic_intrinsic_call(intrinsic)
            }
            of_memory_funcs!(any_of) => decide_memory_intrinsic_call(intrinsic),
            _ => panic!("Uncovered intrinsic: {:?}", intrinsic),
        }
    }

    fn decide_one_to_one_intrinsic_call(intrinsic: IntrinsicDef) -> IntrinsicDecision {
        let pri_sym = match intrinsic.name {
            rsym::rotate_left => psym::intrinsic_assign_rotate_left,
            rsym::rotate_right => psym::intrinsic_assign_rotate_right,
            rsym::saturating_add => psym::intrinsic_assign_saturating_add,
            rsym::saturating_sub => psym::intrinsic_assign_saturating_sub,
            rsym::exact_div => psym::intrinsic_assign_exact_div,
            rsym::bitreverse => psym::intrinsic_assign_bitreverse,
            rsym::cttz_nonzero => psym::intrinsic_assign_cttz_nonzero,
            rsym::cttz => psym::intrinsic_assign_cttz,
            rsym::ctpop => psym::intrinsic_assign_ctpop,
            rsym::ctlz_nonzero => psym::intrinsic_assign_ctlz_nonzero,
            rsym::ctlz => psym::intrinsic_assign_ctlz,
            rsym::bswap => psym::intrinsic_assign_bswap,
            rsym::black_box => psym::intrinsic_assign_identity,
            _ => unreachable!(),
        };
        IntrinsicDecision::OneToOneAssign(pri_sym)
    }

    fn decide_memory_intrinsic_call(intrinsic: IntrinsicDef) -> IntrinsicDecision {
        let (kind, is_volatile) = match intrinsic.name {
            rsym::volatile_load => (
                MemoryIntrinsicKind::Load {
                    is_ptr_aligned: true,
                },
                true,
            ),
            rsym::unaligned_volatile_load => (
                MemoryIntrinsicKind::Load {
                    is_ptr_aligned: false,
                },
                true,
            ),
            rsym::volatile_store => (
                MemoryIntrinsicKind::Store {
                    is_ptr_aligned: true,
                },
                true,
            ),
            rsym::unaligned_volatile_store => (
                MemoryIntrinsicKind::Store {
                    is_ptr_aligned: false,
                },
                true,
            ),
            rsym::nontemporal_store => (
                MemoryIntrinsicKind::Store {
                    is_ptr_aligned: true,
                },
                false,
            ),
            rsym::copy => (
                MemoryIntrinsicKind::Copy {
                    is_overlapping: true,
                },
                false,
            ),
            rsym::copy_nonoverlapping => (
                MemoryIntrinsicKind::Copy {
                    is_overlapping: false,
                },
                false,
            ),
            rsym::volatile_copy_memory => (
                MemoryIntrinsicKind::Copy {
                    is_overlapping: true,
                },
                true,
            ),
            rsym::volatile_copy_nonoverlapping_memory => (
                MemoryIntrinsicKind::Copy {
                    is_overlapping: false,
                },
                true,
            ),
            rsym::write_bytes => (MemoryIntrinsicKind::Set, false),
            rsym::volatile_set_memory => (MemoryIntrinsicKind::Set, true),
            rsym::typed_swap_nonoverlapping => (MemoryIntrinsicKind::Swap, false),
            _ => unreachable!(),
        };
        IntrinsicDecision::Memory { kind, is_volatile }
    }

    fn decide_atomic_intrinsic_call<'tcx>(intrinsic: IntrinsicDef) -> IntrinsicDecision {
        macro_rules! str_any_of {
            ($($intrinsic:ident),*$(,)?) => {
                $(stringify!($intrinsic))|*
            };
        }

        let name = intrinsic.name.as_str();
        let parts = name.split('_').skip(1).collect::<Vec<_>>();
        let operation = parts[0];
        let ordering = atomic_ordering_from_str(parts[1]);
        let kind = match name {
            of_atomic_load_funcs!(str_any_of) => AtomicIntrinsicKind::Load,
            of_atomic_store_funcs!(str_any_of) => AtomicIntrinsicKind::Store,
            of_atomic_xchg_funcs!(str_any_of) => AtomicIntrinsicKind::Exchange,
            of_atomic_cxchg_funcs!(str_any_of) => AtomicIntrinsicKind::CompareExchange {
                fail_ordering: atomic_ordering_from_str(parts[2]),
                weak: operation.contains("weak"),
            },
            of_atomic_binop_funcs!(str_any_of) => {
                AtomicIntrinsicKind::BinOp(atomic_binop_from_str(&operation))
            }
            of_atomic_fence_funcs!(str_any_of) => AtomicIntrinsicKind::Fence {
                single_thread: operation.contains("singlethread"),
            },
            _ => unreachable!(),
        };
        IntrinsicDecision::Atomic(ordering, kind)
    }

    fn atomic_ordering_from_str(ordering: &str) -> AtomicOrdering {
        match ordering {
            "acqrel" => AtomicOrdering::ACQ_REL,
            "acquire" => AtomicOrdering::ACQUIRE,
            "relaxed" => AtomicOrdering::RELAXED,
            "release" => AtomicOrdering::RELEASE,
            "seqcst" => AtomicOrdering::SEQ_CST,
            "unordered" => AtomicOrdering::UNORDERED,
            _ => unreachable!(),
        }
    }

    fn atomic_binop_from_str(binop: &str) -> AtomicBinaryOp {
        match binop {
            "and" => AtomicBinaryOp::AND,
            "max" => AtomicBinaryOp::MAX,
            "min" => AtomicBinaryOp::MIN,
            "nand" => AtomicBinaryOp::NAND,
            "or" => AtomicBinaryOp::OR,
            "umax" => AtomicBinaryOp::MAX,
            "umin" => AtomicBinaryOp::MIN,
            "xadd" => AtomicBinaryOp::ADD,
            "xor" => AtomicBinaryOp::XOR,
            "xsub" => AtomicBinaryOp::SUB,
            _ => unreachable!(),
        }
    }
}
pub(super) use intrinsics::{
    AtomicIntrinsicKind, IntrinsicDecision, MemoryIntrinsicKind, decide_intrinsic_call,
};

mod rules {
    use std::ops::DerefMut;

    use crate::{
        config::{
            CrateFilter, EntityFilter, EntityLocationFilter, InstrumentationRules,
            MethodDynDefinitionFilter, WholeBodyFilter, rules::LogicFormula,
        },
        passes::StorageExt,
        utils::rules::{Predicate, ToPredicate},
    };

    use super::*;

    pub(crate) const KEY_RULES: &str = "instr_rules";
    pub(crate) const KEY_BAKED_BODY_RULES: &str = "instr_rules_baked_body";
    pub(crate) const KEY_BAKED_DYN_DEF_RULES: &str = "instr_rules_baked_dyn_def";

    type BakedEntityFilterRules<'tcx> = crate::utils::rules::InclusionPredicate<
        <EntityFilter as ToPredicate<(TyCtxt<'tcx>, DefId)>>::Predicate,
    >;

    pub(crate) fn get_baked_body_rules<'tcx>(
        storage: &mut dyn Storage,
    ) -> impl DerefMut<Target = BakedEntityFilterRules<'tcx>> + '_ {
        storage
            .get_mut::<BakedEntityFilterRules<'tcx>>(&KEY_BAKED_BODY_RULES.to_owned())
            .expect("Filter rules are expected to be baked at this point.")
    }

    pub(crate) fn get_baked_dyn_def_rules<'tcx>(
        storage: &mut dyn Storage,
    ) -> impl DerefMut<Target = BakedEntityFilterRules<'tcx>> + '_ {
        storage
            .get_mut::<BakedEntityFilterRules<'tcx>>(&KEY_BAKED_DYN_DEF_RULES.to_owned())
            .expect("Filter rules are expected to be baked at this point.")
    }

    pub(super) fn bake_rules(
        storage: &mut dyn Storage,
        additional_exclusions: impl FnOnce() -> Vec<WholeBodyFilter>,
    ) {
        // We use explicit types to ensure not using the wrong type by mistake.
        let _ = storage.get_or_insert_with_acc(
            KEY_BAKED_DYN_DEF_RULES.to_owned(),
            |storage| -> BakedEntityFilterRules<'_> {
                let rules = storage.get_or_default::<InstrumentationRules>(KEY_RULES.to_owned());
                rules
                    .clone()
                    .filter(|r| matches!(r, EntityFilter::MethodDynDefinition(..)))
                    .to_baked()
            },
        );
        let _ = storage.get_or_insert_with_acc(
            KEY_BAKED_BODY_RULES.to_owned(),
            |storage| -> BakedEntityFilterRules<'_> {
                let rules = storage.get_or_default::<InstrumentationRules>(KEY_RULES.to_owned());
                let mut rules = rules
                    .clone()
                    .filter(|r| matches!(r, EntityFilter::WholeBody(..)));
                rules.exclude.extend(
                    additional_exclusions()
                        .into_iter()
                        .map(EntityFilter::WholeBody),
                );
                rules.to_baked()
            },
        );
    }

    impl<'tcx> ToPredicate<(TyCtxt<'tcx>, DefId)> for EntityFilter {
        type Predicate =
            <LogicFormula<EntityLocationFilter> as ToPredicate<(TyCtxt<'tcx>, DefId)>>::Predicate;
        fn to_predicate(&self) -> Self::Predicate {
            match self {
                EntityFilter::WholeBody(WholeBodyFilter(formula))
                | EntityFilter::MethodDynDefinition(MethodDynDefinitionFilter(formula)) => {
                    formula.to_predicate()
                }
            }
        }
    }

    impl ToPredicate<(TyCtxt<'_>, DefId)> for EntityLocationFilter {
        type Predicate = Box<dyn Fn(&(TyCtxt, DefId)) -> bool>;

        fn to_predicate(&self) -> Self::Predicate {
            match self {
                EntityLocationFilter::Crate(crate_filter) => match crate_filter.clone() {
                    CrateFilter::Externality(is_external) => {
                        Box::new(move |(_, def_id)| def_id.is_local() != is_external)
                    }
                    CrateFilter::Name(name) => {
                        Box::new(move |(tcx, def_id)| tcx.crate_name(def_id.krate).as_str() == name)
                    }
                },
                EntityLocationFilter::DefPathMatch(pattern) => {
                    let pred = pattern.to_predicate();
                    Box::new(move |(tcx, def_id)| {
                        let def_path = tcx.def_path_str(def_id);
                        pred.accept(&def_path)
                    })
                }
            }
        }
    }
}
