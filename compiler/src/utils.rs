#[derive(Default)]
pub(crate) struct Chain<A, B> {
    pub first: A,
    pub second: B,
}

/* NOTE: The trailing comma is mandatory for this macro.
 * Also note that a kind of expression is path, so it is important to
 * distinguish them in the pattern.
 */
macro_rules! chain {
    (<$head:path>, $($tail:tt)*) => {
        crate::utils::chain!((<$head>::default()), $($tail)*)
    };
    ($single:expr,) => {
        $single
    };
    ($head:expr, $($tail:tt)+) => {
        crate::utils::Chain {
            first: $head,
            second: crate::utils::chain!($($tail)+)
        }
    };
}

pub(crate) use chain;

pub(crate) mod mir {
    use rustc_hir::{def::DefKind, definitions::DisambiguatedDefPathData};
    use rustc_middle::ty::{TyCtxt, TypingMode};
    use rustc_span::def_id::{DefId, LocalDefId};

    pub(crate) trait TyCtxtExt<'tcx> {
        fn is_llvm_intrinsic(self, def_id: DefId) -> bool;
        fn module_of(self, def_id: DefId) -> impl Iterator<Item = DisambiguatedDefPathData>;
        fn typing_mode_for_body(self, def_id: LocalDefId) -> TypingMode<'tcx>;
    }

    impl<'tcx> TyCtxtExt<'tcx> for TyCtxt<'tcx> {
        fn is_llvm_intrinsic(self, def_id: DefId) -> bool {
            // Grabbed from `is_call_from_compiler_builtins_to_upstream_monomorphization`#5aea140.
            if let Some(name) = self.codegen_fn_attrs(def_id).link_name {
                name.as_str().starts_with("llvm.")
            } else {
                false
            }
        }

        fn module_of(self, def_id: DefId) -> impl Iterator<Item = DisambiguatedDefPathData> {
            use rustc_hir::definitions::DefPathData;
            self.def_path(def_id)
                .data
                .into_iter()
                .take_while(|p| matches!(p.data, DefPathData::TypeNs(_)))
        }

        // A safe wrapper around `opaque_types_defined_by`
        fn typing_mode_for_body(self, item: LocalDefId) -> TypingMode<'tcx> {
            let kind = self.def_kind(item);
            match kind {
                DefKind::AssocFn
                | DefKind::Fn
                | DefKind::Static { .. }
                | DefKind::Const
                | DefKind::AssocConst
                | DefKind::AnonConst
                | DefKind::Closure
                | DefKind::InlineConst => {
                    if self.hir().maybe_body_owned_by(item).is_some() {
                        TypingMode::analysis_in_body(self, item)
                    } else {
                        TypingMode::non_body_analysis()
                    }
                }
                DefKind::OpaqueTy
                | DefKind::TyAlias
                | DefKind::AssocTy
                | DefKind::Mod
                | DefKind::Struct
                | DefKind::Union
                | DefKind::Enum
                | DefKind::Variant
                | DefKind::Trait
                | DefKind::ForeignTy
                | DefKind::TraitAlias
                | DefKind::TyParam
                | DefKind::ConstParam
                | DefKind::Ctor(_, _)
                | DefKind::Macro(_)
                | DefKind::ExternCrate
                | DefKind::Use
                | DefKind::ForeignMod
                | DefKind::Field
                | DefKind::LifetimeParam
                | DefKind::GlobalAsm
                | DefKind::Impl { .. }
                | DefKind::SyntheticCoroutineBody => TypingMode::non_body_analysis(),
            }
        }
    }
}

pub(super) mod file {
    use std::{
        env,
        path::{Path, PathBuf},
    };

    use common::log_debug;

    // The deps folder when building the project. Set by the build script.
    const DIR_DEPS: &str = env!("DEPS_DIR");

    pub(crate) fn find_dependency_path<'a>(
        name: &'static str,
        priority_dirs: impl IntoIterator<Item = &'a Path>,
    ) -> PathBuf {
        try_find_dependency_path(name, priority_dirs)
            .unwrap_or_else(|| panic!("Unable to find the dependency with name: {}", name))
    }

    // Tries to find a dependency of the compiler by searching in the given directories.
    // Order of search is:
    // 1. Priority directories
    // 2. Current working directory
    // 3. Directory of the executable
    pub(crate) fn try_find_dependency_path<'a>(
        name: impl AsRef<Path> + Clone,
        priority_dirs: impl IntoIterator<Item = &'a Path>,
    ) -> Option<PathBuf> {
        let try_dir = |path: &Path| {
            log_debug!(
                "Trying dir in search of `{}`: {:?}",
                name.as_ref().display(),
                path
            );
            common::utils::try_join_path(path, name.clone())
        };

        let try_priority_dirs = || priority_dirs.into_iter().find_map(try_dir);
        let try_deps = || try_dir(DIR_DEPS.as_ref());
        let try_cwd = || env::current_dir().ok().and_then(|p| try_dir(&p));
        let try_exe_path = || {
            env::current_exe()
                .ok()
                .and_then(|p| p.ancestors().skip(1).find_map(try_dir))
        };

        None.or_else(try_priority_dirs)
            .or_else(try_deps)
            .or_else(try_cwd)
            .or_else(try_exe_path)
    }

    pub(crate) trait TyCtxtFileExt<'tcx> {
        fn output_dir(self) -> PathBuf;
    }

    impl<'tcx> TyCtxtFileExt<'tcx> for rustc_middle::ty::TyCtxt<'tcx> {
        fn output_dir(self) -> PathBuf {
            self.output_filenames(())
                .with_extension("")
                .parent()
                .unwrap()
                .to_path_buf()
        }
    }
}

pub(crate) mod rules {

    pub(crate) trait Predicate<I: ?Sized> {
        fn accept(&self, i: &I) -> bool;
    }

    pub(crate) trait ToPredicate<I: ?Sized> {
        type Predicate: Predicate<I>;

        fn to_predicate(&self) -> Self::Predicate;
    }

    impl<'b, I: ?Sized, F: ?Sized + for<'a> Fn(&'a I) -> bool, T: Deref<Target = F>> Predicate<I>
        for T
    {
        fn accept(&self, i: &I) -> bool {
            self(i)
        }
    }

    use std::ops::Deref;

    use crate::config::rules::*;

    impl<T> InclusionRules<T> {
        pub(crate) fn filter(self, f: impl Fn(&T) -> bool) -> Self {
            InclusionRules {
                include: self.include.into_iter().filter(&f).collect(),
                exclude: self.exclude.into_iter().filter(&f).collect(),
            }
        }

        pub(crate) fn to_baked<I: ?Sized>(&self) -> InclusionPredicate<T::Predicate>
        where
            T: ToPredicate<I>,
        {
            fn to_any<I: ?Sized, T: ToPredicate<I>>(filters: &[T]) -> LogicFormula<T::Predicate> {
                let predicates = filters
                    .iter()
                    .map(|f| f.to_predicate())
                    .map(LogicFormula::Atom)
                    .collect::<Vec<_>>();
                if predicates.is_empty() {
                    LogicFormula::Empty {}
                } else {
                    LogicFormula::Any(AnyFormula { of: predicates })
                }
            }

            let include = to_any(&self.include);
            let exclude = to_any(&self.exclude);

            InclusionPredicate { include, exclude }
        }
    }

    pub(crate) struct InclusionPredicate<P> {
        include: LogicFormula<P>,
        exclude: LogicFormula<P>,
    }

    impl<P> InclusionPredicate<P> {
        pub(crate) fn accept<I: ?Sized>(&self, i: &I) -> Option<bool>
        where
            P: Predicate<I>,
        {
            if self.include.accept(i) {
                Some(true)
            } else if self.exclude.accept(i) {
                Some(false)
            } else {
                None
            }
        }
    }

    impl<I: ?Sized, T: Predicate<I>> Predicate<I> for LogicFormula<T> {
        fn accept(&self, i: &I) -> bool {
            match self {
                LogicFormula::Empty {} => false,
                LogicFormula::Atom(f) => f.accept(i),
                LogicFormula::Not(NotFormula { of }) => !of.accept(i),
                LogicFormula::Any(AnyFormula { of }) => of.iter().any(|f| f.accept(i)),
                LogicFormula::All(AllFormula { of }) => of.iter().all(|f| f.accept(i)),
            }
        }
    }

    impl<I: ?Sized, T: ToPredicate<I>> ToPredicate<I> for LogicFormula<T> {
        type Predicate = LogicFormula<T::Predicate>;

        fn to_predicate(&self) -> Self::Predicate {
            match self {
                LogicFormula::Empty {} => LogicFormula::Empty {},
                LogicFormula::Atom(f) => LogicFormula::Atom(f.to_predicate()),
                LogicFormula::Not(NotFormula { of }) => LogicFormula::Not(NotFormula {
                    of: Box::new(of.to_predicate()),
                }),
                LogicFormula::Any(AnyFormula { of }) => LogicFormula::Any(AnyFormula {
                    of: of.iter().map(ToPredicate::to_predicate).collect::<Vec<_>>(),
                }),
                LogicFormula::All(AllFormula { of }) => LogicFormula::All(AllFormula {
                    of: of.iter().map(ToPredicate::to_predicate).collect::<Vec<_>>(),
                }),
            }
        }
    }

    pub(crate) struct RegexWrapper(regex_lite::Regex);
    impl Predicate<str> for RegexWrapper {
        fn accept(&self, i: &str) -> bool {
            self.0.is_match(i)
        }
    }

    impl ToPredicate<str> for PatternMatch {
        type Predicate = RegexWrapper;

        fn to_predicate(&self) -> Self::Predicate {
            RegexWrapper(regex_lite::Regex::new(self).expect("Invalid regex pattern"))
        }
    }
}
