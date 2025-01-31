use common::{log_debug, log_info};
use std::collections::{HashMap, HashSet};

use rustc_hir::{
    def::DefKind,
    def_id::{CrateNum, DefId, LOCAL_CRATE},
};
use rustc_middle::ty::{Ty, TyCtxt};

pub(super) const TAG_DISCOVERY: &str = "pri_discovery";

pub mod sym {
    #![allow(non_upper_case_globals)]

    use derive_more as dm;

    #[derive(Clone, Copy, dm::Deref, Debug, dm::Display, Hash, PartialEq, Eq, PartialOrd, Ord)]
    #[repr(transparent)]
    pub struct LeafSymbol(&'static str);

    use const_format::concatcp;

    use LeafSymbol as LS;

    pub const CORE_LIB_CRATE: LS = LS("core");
    pub const RUNTIME_LIB_CRATE: LS = LS(crate::constants::CRATE_RUNTIME_SHIM);

    pub const MODULE_MARKER_DIAG_NAME: LS = LS("leaf_module_marker");

    macro_rules! in_lib {
        ($name: ident) => {
            concatcp!(RUNTIME_LIB_CRATE.0, "::", stringify!($name))
        };
    }
    macro_rules! symbols_in_lib {
        ($($name: ident),* $(,)?) => {
            $(pub(crate) const $name: LS = LS(in_lib!($name));)*
        };
    }

    symbols_in_lib! {
        pri,
    }

    macro_rules! in_pri {
        ($name: ident) => {
            concatcp!(pri.0, "::", stringify!($name))
        };
    }
    macro_rules! symbols_in_pri {
        ($($name: ident),* $(,)?) => {
            #[allow(dead_code)]
            $(pub(crate) const $name: LS = LS(in_pri!($name));)*
        };
    }

    symbols_in_pri! {
        MODULE_MARKER,
        compiler_helpers,
    }

    macro_rules! bracket {
        ($($name: ident),*$(,)?) => {
            [
                $($name),*
            ]
        };
    }

    mod mains {
        #![allow(non_upper_case_globals)]

        use super::*;

        common::pri::pass_func_names_to!(symbols_in_pri, all_comma_separated);

        pub(crate) const ALL_MAINS: [LeafSymbol; 101] =
            common::pri::pass_func_names_to!(bracket, all_comma_separated);

        pub(crate) mod intrinsics {
            #![allow(non_upper_case_globals)]
            use super::*;

            #[derive(
                Clone, Copy, dm::Deref, Debug, dm::Display, Hash, PartialEq, Eq, PartialOrd, Ord,
            )]
            #[repr(transparent)]
            pub struct LeafIntrinsicSymbol(LeafSymbol);

            macro_rules! symbols_for_intrinsics {
                ($($name: ident),* $(,)?) => {
                    $(
                        #[allow(non_upper_case_globals)]
                        pub(crate) const $name: LeafIntrinsicSymbol = LeafIntrinsicSymbol(super::$name);
                    )*
                };
            }

            symbols_for_intrinsics! {
                intrinsic_assign_rotate_left,
                intrinsic_assign_rotate_right,
                intrinsic_assign_saturating_add,
                intrinsic_assign_saturating_sub,
                intrinsic_assign_exact_div,
                intrinsic_assign_bitreverse,
                intrinsic_assign_cttz_nonzero,
                intrinsic_assign_cttz,
                intrinsic_assign_ctpop,
                intrinsic_assign_ctlz_nonzero,
                intrinsic_assign_ctlz,

                intrinsic_atomic_load,
                intrinsic_atomic_store,
                intrinsic_atomic_xchg,
                intrinsic_atomic_cxchg,
                intrinsic_atomic_binary_op,
                intrinsic_atomic_fence,
            }

            pub(crate) mod atomic {
                use super::*;

                #[derive(
                    Clone, Copy, dm::Deref, Debug, dm::Display, Hash, PartialEq, Eq, PartialOrd, Ord,
                )]
                #[repr(transparent)]
                pub struct LeafAtomicIntrinsicSymbol(LeafIntrinsicSymbol);

                macro_rules! symbols_for_atomic_intrinsics {
                    ($($name: ident),* $(,)?) => {
                        $(
                            #[allow(non_upper_case_globals)]
                            pub(crate) const $name: LeafAtomicIntrinsicSymbol = LeafAtomicIntrinsicSymbol(super::$name);
                        )*
                    };
                }

                symbols_for_atomic_intrinsics! {
                    intrinsic_atomic_load,
                    intrinsic_atomic_store,
                    intrinsic_atomic_xchg,
                    intrinsic_atomic_cxchg,
                    intrinsic_atomic_binary_op,
                    intrinsic_atomic_fence,
                }
            }
        }
    }
    pub(crate) use mains::*;

    mod compiler_helpers {
        use super::*;

        macro_rules! make_pass_compiler_helpers_to_macro {
            (funcs: [$($fname:ident),+,], others: [$($o_name:ident),+,]) => {
                macro_rules! pass_compiler_helpers_to {
                    ($$macro:ident, just_funcs) => {
                        $$macro! {
                            $($fname),+
                        }
                    };
                    ($$macro:ident) => {
                        $$macro! {
                            $($fname),+,
                            $($o_name),+
                        }
                    };
                }
            };
        }

        make_pass_compiler_helpers_to_macro! {
            funcs: [
                f32_to_bits,
                f64_to_bits,

                set_place_address_typed,
                type_id_of,
                size_of,

                basic_block_location,
                switch_info,
                assertion_info,

                callee_def_static,
                callee_def_maybe_virtual,
                func_def_static,
                func_def_dyn_method,
                receiver_to_raw_ptr,
                receiver_pin_to_raw_ptr,
                receiver_self_to_raw_ptr,

                const_binary_op_of,
                const_unary_op_of,

                const_atomic_ord_of,
                const_atomic_binary_op_of,

                special_func_placeholder,
            ],
            others: [
                CH_MODULE_MARKER,

                PLACE_REF_TYPE_HOLDER,
                OPERAND_REF_TYPE_HOLDER,
                BINARY_OP_TYPE_HOLDER,
                UNARY_OP_TYPE_HOLDER,
            ]
        }

        pub(in super::super) use pass_compiler_helpers_to;

        macro_rules! in_compiler_helpers {
            ($name: ident) => {
                concatcp!(compiler_helpers.0, "::", stringify!($name))
            };
        }
        macro_rules! symbols_in_compiler_helpers {
            ($($name: ident),* $(,)?) => {
                $(
                    #[allow(non_upper_case_globals)]
                    pub(crate) const $name: LS = LS(in_compiler_helpers!($name));
                )*
            };
        }

        pass_compiler_helpers_to!(symbols_in_compiler_helpers);

        pub(crate) const ALL_HELPERS: [LS; 25] = pass_compiler_helpers_to!(bracket);
    }
    pub(crate) use compiler_helpers::*;

    impl TryFrom<String> for LeafSymbol {
        type Error = String;

        fn try_from(s: String) -> Result<Self, Self::Error> {
            ALL_MAINS
                .iter()
                .chain(ALL_HELPERS.iter())
                .find(|sym| sym.0 == s)
                .copied()
                .ok_or(s)
        }
    }
}

use sym::LeafSymbol;

#[derive(Clone, Copy, Debug, derive_more::From, derive_more::Deref)]
pub(crate) struct FunctionInfo {
    pub def_id: DefId,
}

struct TypeHolder(DefId);

impl TypeHolder {
    pub(crate) fn ty<'tcx>(&self, tcx: TyCtxt<'tcx>) -> Ty<'tcx> {
        tcx.type_of(self.0)
            .no_bound_vars()
            .expect("PRI types are not expected to have bound vars.")
    }
}

/// Provides types that are used in PRI functions along with primitive types.
pub(crate) struct PriTypes {
    place_ref: TypeHolder,
    operand_ref: TypeHolder,
    binary_op: TypeHolder,
    unary_op: TypeHolder,
}

impl PriTypes {
    pub(crate) fn place_ref<'tcx>(&self, tcx: TyCtxt<'tcx>) -> Ty<'tcx> {
        self.place_ref.ty(tcx)
    }

    pub(crate) fn operand_ref<'tcx>(&self, tcx: TyCtxt<'tcx>) -> Ty<'tcx> {
        // FIXME: Check if additional caching can be beneficial
        self.operand_ref.ty(tcx)
    }
}

macro_rules! define_pri_helper_funcs {
    ($($name: ident),*$(,)?) => {
        pub(crate) struct PriHelperFunctions {
            $(
                pub $name: FunctionInfo,
            )*
        }
    };
}

sym::pass_compiler_helpers_to!(define_pri_helper_funcs, just_funcs);

/// Lists all the PRI items, including the compiler helper items.
pub(crate) fn all_pri_items(tcx: TyCtxt) -> Vec<DefId> {
    let crate_num = find_pri_host_crate(tcx);
    let marker_id = find_pri_marker(tcx, crate_num);
    let result = collect_all_pri_items(tcx, marker_id);
    log_debug!(
        target: TAG_DISCOVERY,
        "Found {} PRI items.",
        result.len(),
    );
    result
}

/// Finds the crate that holds the PRI items.
/// # Remarks
/// Currently, it is possible to have access to the PRI either as a part of
/// the core library or as an external crate linked to the target program.
/// This returns either crate that should be searched for the PRI items.
/// When building the core library, the current crate is returned.
fn find_pri_host_crate(tcx: TyCtxt) -> CrateNum {
    log_debug!(
        target: TAG_DISCOVERY,
        "Searching for the crate hosting PRI symbols among: {:?}",
        tcx.crates(()).iter().map(|cnum| tcx.crate_name(*cnum)).collect::<Vec<_>>(),
    );

    tcx.crate_by_name(*sym::RUNTIME_LIB_CRATE)
        .or_else(|| tcx.crate_by_name(*sym::CORE_LIB_CRATE))
        .or_else(|| {
            /* NOTE: This is not expected to happen anymore.
             * Previously, we tried to build the core library with instrumentation directly.
             * However, currently, all bodies from the dependencies will be instrumented during
             * building the target program. Thus, the core library should not be built directly.
             * Anyway, if that happens, the runtime should be available as a part of it.
             */
            if tcx.crate_name(LOCAL_CRATE).as_str() == *sym::CORE_LIB_CRATE {
                Some(LOCAL_CRATE)
            } else {
                None
            }
        })
        .inspect(|cnum| {
            log_info!(
                target: TAG_DISCOVERY,
                "Selected crate {}, to search for PRI symbols.",
                tcx.crate_name(*cnum).as_str().to_string()
            );
        })
        .expect("Could not find the expected crates hosting PRI symbols.")
}

/// Finds `DefId` of the PRI module's marker.
/// # Remarks
/// The marker is a dummy static variable residing in the PRI module.
/// As we usually search through a set of definitions (and not modules), to find
/// the PRI module/items, we use this workaround rather than looking for the
/// module directly.
fn find_pri_marker(tcx: TyCtxt, crate_num: CrateNum) -> DefId {
    log_debug!(
        target: TAG_DISCOVERY,
        "Searching for the PRI module marker in crate {}.",
        tcx.crate_name(crate_num).as_str(),
    );

    if let Some(def_id) = tcx
        .diagnostic_items(crate_num)
        .name_to_id
        .get(&rustc_span::Symbol::intern(*sym::MODULE_MARKER_DIAG_NAME))
    {
        return *def_id;
    }

    let search_space: Box<dyn Iterator<Item = DefId>> = if crate_num == LOCAL_CRATE {
        Box::new(tcx.mir_keys(()).iter().map(|id| id.to_def_id()))
    } else {
        use rustc_middle::middle::exported_symbols::ExportedSymbol;
        Box::new(
            tcx.exported_symbols(crate_num)
                .iter()
                .filter_map(|(s, _)| match s {
                    ExportedSymbol::NonGeneric(def_id) | ExportedSymbol::Generic(def_id, _) => {
                        Some(*def_id)
                    }
                    _ => None,
                }),
        )
    };

    search_space
        .find_by_name(tcx, *sym::MODULE_MARKER)
        .unwrap_or_else(|| {
            panic!(
                "Could not find the PRI module marker in the given crate: {}:{}.",
                crate_num,
                tcx.crate_name(crate_num).as_str(),
            )
        })
}

/// Collects all the PRI items with respect to the marker.
/// # Returns
/// The list of `DefId`s existing in the same module or submodules of the parent
/// module of the marker.
fn collect_all_pri_items<'tcx>(tcx: TyCtxt<'tcx>, module_marker_id: DefId) -> Vec<DefId> {
    log_debug!(
        target: TAG_DISCOVERY,
        "Collecting all PRI items in the same module or sibling modules of {:?}.",
        module_marker_id,
    );
    if module_marker_id.krate == LOCAL_CRATE {
        /* NOTE: `module_children` panics for local ids,
         * and `module_children_local` doesn't return items. */
        tcx.mir_keys(())
            .iter()
            .map(|id| id.to_def_id())
            .filter_by_module_marker(tcx, module_marker_id, true)
            .collect()
    } else {
        tcx.module_children_rec(tcx.parent(module_marker_id), true)
    }
}

/// Filters out the top-level functions out of the list of all PRI items.
/// # Remarks
/// The functions are the main ones that record the program behavior,
/// i.e. the ones in [`common::pri::ProgramRuntimeInterface`].
/// These items are recognized as being on the same hierarchical level as the marker item.
/// At the moment, the items that are not selected are compiler helpers.
pub(crate) fn filter_main_funcs<'tcx>(
    tcx: TyCtxt<'tcx>,
    all_pri_items: &[DefId],
) -> HashMap<LeafSymbol, FunctionInfo> {
    let items = filter_pri_items(tcx, all_pri_items, sym::MODULE_MARKER)
        .filter(|def_id| matches!(tcx.def_kind(def_id), DefKind::Fn | DefKind::AssocFn))
        .inspect(|def_id| {
            log_debug!(
                target: TAG_DISCOVERY,
                "Found PRI function: {:?}",
                def_id,
            );
        })
        .filter_associate_with_symbol(tcx)
        .map(|(s, id)| (s, id.into()))
        .collect::<HashMap<_, _>>();
    debug_assert_eq!(
        items.len(),
        sym::ALL_MAINS.len(),
        "Some main functions are missing: {:?}",
        &HashSet::from(sym::ALL_MAINS) - &HashSet::from_iter(items.into_keys())
    );
    items
}

/// Filters out the compiler helper items out of the list of all PRI items.
pub(crate) fn filter_helper_items<'tcx>(
    tcx: TyCtxt<'tcx>,
    all_pri_items: &[DefId],
) -> HashMap<LeafSymbol, DefId> {
    filter_pri_items(tcx, all_pri_items, sym::CH_MODULE_MARKER)
        .filter_associate_with_symbol(tcx)
        .collect()
}

/// Collects the helper type holders out of the list of compiler helper PRI items.
pub(crate) fn collect_helper_types<'tcx>(helper_def_ids: &HashMap<LeafSymbol, DefId>) -> PriTypes {
    /* FIXME: The desired enums and type aliases don't show up in the exported symbols.
     * It may be because of the MIR phases that clean up/optimize/unify things,
     * the way that the library is added (using the compiled file), or
     * that enums and type aliases are not included at all in the exported_symbols.
     * As a workaround, we have defined some static variables having those desired
     * types and are accessible.
     * However, there should be some functions in TyCtxt that will list these items for us.
     * Update: Check if the problem still exists with the introduction of `module_children`.
     */

    let get_type_holder =
        |name: LeafSymbol| -> TypeHolder { TypeHolder(*helper_def_ids.get(&name).unwrap()) };

    PriTypes {
        place_ref: get_type_holder(sym::PLACE_REF_TYPE_HOLDER),
        operand_ref: get_type_holder(sym::OPERAND_REF_TYPE_HOLDER),
        binary_op: get_type_holder(sym::BINARY_OP_TYPE_HOLDER),
        unary_op: get_type_holder(sym::UNARY_OP_TYPE_HOLDER),
    }
}

/// Collects the helper functions out of the list of compiler helper PRI items.
pub(crate) fn collect_helper_funcs<'tcx>(
    helper_def_ids: &HashMap<LeafSymbol, DefId>,
) -> PriHelperFunctions {
    let get_func_info = |name: LeafSymbol| {
        helper_def_ids
            .get(&name)
            .copied()
            .unwrap_or_else(|| {
                panic!("`{}` is not exported (probably erased by compiler).", name);
            })
            .into()
    };

    macro_rules! create {
        ($($name: ident),*$(,)?) => {
            PriHelperFunctions {
                $($name: get_func_info(sym::$name)),*
            }
        };
    }

    sym::pass_compiler_helpers_to!(create, just_funcs)
}

fn filter_pri_items<'a, 'tcx: 'a>(
    tcx: TyCtxt<'tcx>,
    all_pri_items: &'a [DefId],
    module_marker: LeafSymbol,
) -> impl Iterator<Item = DefId> + 'a {
    all_pri_items
        .iter()
        .copied()
        .filter_by_sibling_module_marker_name(tcx, module_marker)
}

fn to_map_key(tcx: TyCtxt, def_id: DefId) -> String {
    let def_path = tcx.def_path(def_id);
    core::iter::once(tcx.crate_name(def_path.krate).to_string())
        .chain(def_path.data.into_iter().map(|d| d.data.to_string()))
        .skip_while(|p| p != *sym::RUNTIME_LIB_CRATE)
        .collect::<Vec<_>>()
        .join("::")
}

trait DefIdIterExt<'tcx> {
    fn find_by_name(self, tcx: TyCtxt<'tcx>, name: &str) -> Option<DefId>;

    fn filter_by_sibling_module_marker_name<'a>(
        self,
        tcx: TyCtxt<'tcx>,
        marker_name: LeafSymbol,
    ) -> Box<dyn Iterator<Item = DefId> + 'a>
    where
        Self: 'a + Clone,
        'tcx: 'a;

    fn filter_by_module_marker<'a>(
        self,
        tcx: TyCtxt<'tcx>,
        marker: DefId,
        submodules: bool,
    ) -> Box<dyn Iterator<Item = DefId> + 'a>
    where
        Self: 'a,
        'tcx: 'a;

    fn filter_associate_with_symbol<'a>(
        self,
        tcx: TyCtxt<'tcx>,
    ) -> Box<dyn Iterator<Item = (LeafSymbol, DefId)> + 'a>
    where
        Self: 'a,
        'tcx: 'a;
}

impl<'tcx, I: Iterator<Item = DefId>> DefIdIterExt<'tcx> for I {
    fn find_by_name(mut self, tcx: TyCtxt<'tcx>, name: &str) -> Option<DefId> {
        self.find(|def_id| to_map_key(tcx, *def_id) == name)
    }

    fn filter_by_sibling_module_marker_name<'a>(
        self,
        tcx: TyCtxt<'tcx>,
        marker_name: LeafSymbol,
    ) -> Box<dyn Iterator<Item = DefId> + 'a>
    where
        Self: 'a + Clone,
        'tcx: 'a,
    {
        let marker_id = self.clone().find_by_name(tcx, &marker_name).unwrap();
        self.filter_by_module_marker(tcx, marker_id, false)
    }

    fn filter_by_module_marker<'a>(
        self,
        tcx: TyCtxt<'tcx>,
        marker: DefId,
        submodules: bool,
    ) -> Box<dyn Iterator<Item = DefId> + 'a>
    where
        Self: 'a,
        'tcx: 'a,
    {
        use crate::utils::mir::TyCtxtExt;

        let marker_module = tcx.module_of(marker).collect::<Vec<_>>();
        let result = self.filter(move |def_id| {
            let module = tcx.module_of(*def_id);
            if submodules {
                module
                    .take(marker_module.len())
                    .eq_by(&marker_module, |a, b| &a == b)
            } else {
                module.eq_by(&marker_module, |a, b| &a == b)
            }
        });
        Box::new(result)
    }

    fn filter_associate_with_symbol<'a>(
        self,
        tcx: TyCtxt<'tcx>,
    ) -> Box<dyn Iterator<Item = (LeafSymbol, DefId)> + 'a>
    where
        Self: 'a,
        'tcx: 'a,
    {
        let result = self.filter_map(move |def_id| {
            (LeafSymbol::try_from(to_map_key(tcx, def_id)).map(|s| (s, def_id))).ok()
        });
        Box::new(result)
    }
}

trait TyCtxtExt<'tcx> {
    fn crate_by_name(self, name: &str) -> Option<CrateNum>;
    fn module_children_rec(self, module_id: DefId, submodules: bool) -> Vec<DefId>;
}

impl<'tcx> TyCtxtExt<'tcx> for TyCtxt<'tcx> {
    fn crate_by_name(self, name: &str) -> Option<CrateNum> {
        self.crates(())
            .iter()
            .find(|cnum| self.crate_name(**cnum).as_str() == name)
            .copied()
    }

    fn module_children_rec(self, module_id: DefId, submodules: bool) -> Vec<DefId> {
        let children = if let Some(module_id) = module_id.as_local() {
            self.module_children_local(module_id)
        } else {
            self.module_children(module_id)
        };

        children
            .iter()
            .map(|child| child.res)
            .filter(|res| res.opt_def_id().is_some_and(|id| id != module_id))
            .filter(|res| {
                // Skipping imports
                self.opt_parent(res.def_id())
                    .is_some_and(|parent| parent == module_id)
            })
            .flat_map(|res| {
                let mut result = vec![res.def_id()];
                if submodules {
                    if let Some(submodule_id) = res.mod_def_id() {
                        result.extend(self.module_children_rec(submodule_id, true));
                    }
                }
                result
            })
            .collect()
    }
}
