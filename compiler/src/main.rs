#![feature(rustc_private)]
#![feature(let_chains)]
#![feature(extend_one)]
#![feature(box_patterns)]
#![feature(extract_if)]
#![deny(rustc::internal)]
#![feature(iter_order_by)]
#![feature(macro_metavar_expr)]
#![feature(box_into_inner)]
#![feature(assert_matches)]
#![feature(core_intrinsics)]
#![feature(const_option)]
#![feature(trait_upcasting)]
#![feature(if_let_guard)]

mod config;
mod mir_transform;
mod passes;
mod pri_utils;
mod toolchain_build;
mod utils;
mod visit;

extern crate rustc_abi;
extern crate rustc_apfloat;
extern crate rustc_ast;
extern crate rustc_codegen_ssa;
extern crate rustc_const_eval;
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_index;
extern crate rustc_infer;
extern crate rustc_interface;
extern crate rustc_metadata;
extern crate rustc_middle;
extern crate rustc_mir_build;
extern crate rustc_mir_dataflow;
extern crate rustc_mir_transform;
extern crate rustc_monomorphize;
extern crate rustc_query_system;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_target;
extern crate rustc_trait_selection;
extern crate rustc_type_ir;
extern crate thin_vec;

use common::log_info;

use std::env;
use std::path::PathBuf;

use constants::*;

fn main() {
    init_logging();

    set_up_compiler();

    std::process::exit(run_compiler(env::args()));
}

fn init_logging() {
    use env;
    use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
    let env = env::var(constants::ENV_LOG).unwrap_or("warn".to_string());

    let off_tags = [LOG_PASS_OBJECTS_TAG, LOG_PRI_DISCOVERY_TAG, LOG_BB_JUMP_TAG]
        .map(|t| format!("{}=off", t))
        .join(",");
    let custom_filter = EnvFilter::builder().parse_lossy(format!("{},{}", off_tags, &env));

    // Create a formatting layer with optional write style based on environment variable
    let fmt_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(env::var(constants::ENV_LOG_WRITE_STYLE).map_or(true, |val| val != "never"));

    // Create a subscriber
    tracing_subscriber::registry()
        .with(custom_filter)
        .with(fmt_layer)
        .init();
}

pub fn set_up_compiler() {
    use rustc_session::{EarlyDiagCtxt, config::ErrorOutputType};

    rustc_driver::init_rustc_env_logger(&EarlyDiagCtxt::new(ErrorOutputType::default()));
    rustc_driver::install_ice_hook(URL_BUG_REPORT, |_| ());
}

pub fn run_compiler(args: impl IntoIterator<Item = String>) -> i32 {
    let config = config::load_config();

    let args = driver_args::set_up_args(args);
    log_info!("Running compiler with args: {:?}", args);

    let mut callbacks = driver_callbacks::set_up_callbacks(
        config,
        &driver_args::ArgsExt::parse_crate_options(&args),
    );

    rustc_driver::catch_with_exit_code(|| rustc_driver::run_compiler(&args, callbacks.as_mut()))
}

/// Returns `true` if the crate is ineffective for symbolic execution of the target.
/// Examples include build scripts and procedural macro crates.
fn is_ineffective_crate(opts: &driver_args::CrateOptions) -> bool {
    if opts
        .crate_name
        .as_ref()
        .is_some_and(|name| name.starts_with(CRATE_NAME_PREFIX_BUILD_SCRIPT))
    {
        return true;
    }

    if opts
        .crate_types
        .as_ref()
        .is_some_and(|types| types.len() == 1 && types[0] == driver_args::CrateType::ProcMacro)
    {
        return true;
    }

    false
}

fn is_noop_forced() -> bool {
    let Ok(value) = env::var(ENV_FORCE_NOOP) else {
        return false;
    };

    match value.to_lowercase().as_str() {
        "1" | "true" | "on" | "yes" | "y" => true,
        _ => false,
    }
}

mod driver_callbacks {
    use common::{log_debug, log_info, log_warn};

    use super::{
        config::{LeafCompilerConfig, RuntimeShimLocation},
        passes::*,
        *,
    };
    use crate::utils::chain;

    pub(super) fn set_up_callbacks(
        config: LeafCompilerConfig,
        crate_options: &driver_args::CrateOptions,
    ) -> Box<Callbacks> {
        let is_ineffective_crate = super::is_ineffective_crate(crate_options);
        if is_noop_forced() {
            log_info!("Leafc is forced to work as a normal Rust compiler.");
            Box::new(NoOpPass.to_callbacks())
        } else if (!config.codegen_all_mir || config.building_core) && is_ineffective_crate {
            log_info!(
                "Leafc will work as the normal Rust compiler as the crate is identified as ineffective."
            );
            Box::new(NoOpPass.to_callbacks())
        } else {
            let mut passes = if config.codegen_all_mir && is_ineffective_crate {
                log_info!(
                    "Setting up passes as for an ineffective primary package in codegen all mode."
                );
                build_minimal_passes_in_codegen_all_mode()
            } else if config.codegen_all_mir
                && is_dependency_crate(crate_options.crate_name.as_ref())
            {
                log_info!("Setting up passes as for a dependency in codegen all mode.");
                build_dep_passes_in_codegen_all_mode(&config)
            } else {
                log_info!("Setting up passes as for a primary package.");
                let mut passes = build_primary_passes(&config);
                passes.add_config_callback(Box::new(shim_dep::config_shim_dep));
                passes
            };
            passes.set_leaf_config(config);

            passes.add_config_callback(Box::new(move |rustc_config, leafc_config| {
                let cfg_name = leafc_config.marker_cfg_name.clone();
                if cfg_name.is_empty() {
                    return;
                }
                log_info!("Adding marker cfg to the crate config: `{cfg_name}`.");
                rustc_config
                    .crate_check_cfg
                    .push(format!("cfg({})", cfg_name));
                rustc_config.crate_cfg.push(cfg_name.clone());
            }));
            passes.add_config_callback(Box::new(move |rustc_config, _| {
                /* Forcing inlining to happen as some compiler helper functions in the PRI use
                 * generic functions from the core library which may cause infinite loops. */
                rustc_config.opts.unstable_opts.inline_mir = Some(true);
            }));
            passes.add_config_callback(Box::new(codegen_all::config_codegen_all));
            passes
        }
    }

    fn build_primary_passes(config: &LeafCompilerConfig) -> Box<Callbacks> {
        let prerequisites_pass = RuntimeExternCrateAdder::new(
            matches!(
                config.runtime_shim.location,
                RuntimeShimLocation::External { .. }
            ),
            match config.runtime_shim.location {
                RuntimeShimLocation::CoreLib => None,
                RuntimeShimLocation::External { ref crate_name, .. } => Some(crate_name.clone()),
            },
        );

        let nctfe_pass = NoOpPass;

        let passes = chain!(
            prerequisites_pass,
            <TypeInfoExporter>,
            <ProgramMapExporter>,
            <ProgramDependenceMapExporter>,
            nctfe_pass,
            Instrumentor::new(true, None /* FIXME */, config.instr_rules.clone()),
        );

        if config.codegen_all_mir {
            Box::new(
                chain!(force_codegen_all_pass(), passes,)
                    .into_logged()
                    .to_callbacks(),
            )
        } else {
            Box::new(passes.into_logged().to_callbacks())
        }
    }

    fn build_dep_passes_in_codegen_all_mode(config: &LeafCompilerConfig) -> Box<Callbacks> {
        /* In this mode, we only internalize the items in the compiled objects,
         * and do the instrumentation with the final crate. */
        Box::new(
            chain!(
                force_codegen_all_pass(),
                MonoItemInternalizer::new(config.internalization_rules.clone()),
            )
            .to_callbacks(),
        )
    }

    fn build_minimal_passes_in_codegen_all_mode() -> Box<Callbacks> {
        /* If we are using the version of the core library with internalized symbols,
         * then we have to force codegen for all items so the internalized symbols
         * will be found. */
        Box::new(chain!(force_codegen_all_pass(),).to_callbacks())
    }

    const SHOULD_CODEGEN_FLAGS: u8 = OverrideFlags::SHOULD_CODEGEN.bits();

    fn force_codegen_all_pass() -> OverrideFlagsForcePass<SHOULD_CODEGEN_FLAGS> {
        /* We must enable overriding should_codegen to force the codegen for all items.
         * Currently, overriding it equals to forcing the codegen for all items. */
        OverrideFlagsForcePass::<SHOULD_CODEGEN_FLAGS>::default()
    }

    fn is_dependency_crate(crate_name: Option<&String>) -> bool {
        let from_cargo = rustc_session::utils::was_invoked_from_cargo();
        let is_primary = env::var("CARGO_PRIMARY_PACKAGE").is_ok();

        log_debug!(
            "Checking if crate `{}` is a dependency. From cargo: {}, Primary Package: {}",
            crate_name.map(|s| s.as_str()).unwrap_or("UNKNOWN"),
            from_cargo,
            is_primary,
        );

        from_cargo && !is_primary
    }

    mod shim_dep {
        use rustc_session::{
            config::{ExternEntry, ExternLocation, Externs},
            search_paths::{PathKind, SearchPath},
            utils::CanonicalizedPath,
        };

        use std::{collections::BTreeMap, iter, path::Path};

        use super::*;
        use crate::utils::file::*;

        const DIR_DEPS: &str = "deps";

        const DIR_RUNTIME_SHIM_LIB: &str = "runtime_shim";
        const FILE_RUNTIME_SHIM_LIB: &str = "libleafrtsh.rlib";

        pub(super) fn config_shim_dep(
            rustc_config: &mut rustc_interface::Config,
            leafc_config: &mut LeafCompilerConfig,
        ) {
            match &leafc_config.runtime_shim.location {
                RuntimeShimLocation::CoreLib => {
                    log_info!("Expecting the runtime shim as a part of core library")
                    // Nothing to do.
                }
                RuntimeShimLocation::External { .. } => {
                    log_info!("Adding the runtime shim as an external dependency");
                    add_shim_as_external(rustc_config, leafc_config)
                }
            }
        }

        fn add_shim_as_external(
            rustc_config: &mut rustc_interface::Config,
            leafc_config: &mut LeafCompilerConfig,
        ) {
            use crate::config::RuntimeShimExternalLocation::*;
            let RuntimeShimLocation::External { search_path, .. } =
                &leafc_config.runtime_shim.location
            else {
                unreachable!()
            };
            let location = match search_path {
                Sysroot | CrateDeps => {
                    log_debug!("Expecting the runtime shim in the search paths of the crate.");
                    if search_path == &Sysroot && leafc_config.codegen_all_mir != true {
                        log_warn!(concat!(
                            "The runtime shim is expected to be in the sysroot",
                            "which is meant for codegen_all_mir mode. ",
                            "The mode will be enabled."
                        ));
                        leafc_config.codegen_all_mir = true;
                    }
                    ExternLocation::FoundInLibrarySearchDirectories
                }
                _ => {
                    let rlib_path = match search_path {
                        Compiler => find_dependency_path(DIR_RUNTIME_SHIM_LIB, [])
                            .join(FILE_RUNTIME_SHIM_LIB),
                        Exact(path) => PathBuf::from(path),
                        _ => unreachable!(),
                    };
                    let search_path = make_search_path_for_transitive_deps(&rlib_path);

                    log_debug!(
                        "Runtime shim path: {} found at: {}",
                        rlib_path.display(),
                        search_path.dir.display()
                    );

                    rustc_config.opts.search_paths.push(search_path);
                    ExternLocation::ExactPaths(
                        core::iter::once(CanonicalizedPath::new(rlib_path)).collect(),
                    )
                }
            };

            let extern_entry = ExternEntry {
                add_prelude: true,
                force: true,
                is_private_dep: false,
                nounused_dep: false,
                location,
            };
            rustc_config.opts.externs = Externs::new(
                // We prepend to let the users force the location if they want to.
                iter::once((CRATE_RUNTIME_SHIM.to_owned(), extern_entry))
                    .chain(
                        rustc_config
                            .opts
                            .externs
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone())),
                    )
                    .collect::<BTreeMap<_, _>>(),
            );
        }

        fn make_search_path_for_transitive_deps(lib_file_path: &Path) -> SearchPath {
            SearchPath::new(
                PathKind::Dependency,
                // The `deps` folder next to the library file
                lib_file_path.parent().unwrap().join(DIR_DEPS),
            )
        }
    }

    mod codegen_all {

        use super::*;

        pub(super) fn config_codegen_all(
            rustc_config: &mut rustc_interface::Config,
            leafc_config: &mut LeafCompilerConfig,
        ) {
            if !leafc_config.codegen_all_mir {
                return;
            }

            rustc_config.opts.unstable_opts.always_encode_mir = true;
            rustc_config
                .opts
                .cli_forced_codegen_units
                .replace(1)
                .inspect(|old| {
                    if *old != 1 {
                        log_warn!(
                            concat!(
                                "Forcing codegen units to 1 because of compilation mode. ",
                                "The requested value was: {:?}",
                            ),
                            old,
                        );
                    }
                });

            let is_building_core = leafc_config.building_core
                || rustc_config
                    .crate_cfg
                    .iter()
                    .any(|cfg| cfg == CONFIG_CORE_BUILD);

            if !is_building_core {
                sysroot::check_sysroot(rustc_config, leafc_config);
            }
        }
    }

    mod sysroot {
        use super::*;

        use std::path::Path;

        use common::log_error;
        use toolchain_build::{self, is_sysroot_compatible, try_find_compatible_toolchain};

        pub(super) fn check_sysroot(
            rustc_config: &mut rustc_interface::Config,
            leafc_config: &LeafCompilerConfig,
        ) {
            let current_sysroot = rustc_config.opts.sysroot.path();
            if is_sysroot_compatible(&current_sysroot, None) {
                return;
            }

            if !leafc_config.override_sysroot {
                log_warn!(concat!(
                    "Codegen all MIR is enabled, while the sysroot does not seem to be built for leaf. ",
                    "It is necessary to use a sysroot with MIR for all libraries included.",
                    "Unless you are doing special build scenarios, this may lead to unexpected errors.",
                ));
                return;
            }

            log_debug!("The current sysroot is probably not compatible for codegen all MIR.");

            let sysroot = try_find_compatible_toolchain(&current_sysroot)
                .unwrap_or_else(|| build_toolchain(&current_sysroot, rustc_config));
            log_info!(
                "Overriding the sysroot with the one found at: {}",
                sysroot.display()
            );
            rustc_config.opts.sysroot = rustc_session::config::Sysroot::new(Some(sysroot));
        }

        fn build_toolchain(
            current_sysroot: &Path,
            rustc_config: &rustc_interface::Config,
        ) -> PathBuf {
            log_info!(
                "Building a compatible toolchain based on the current sysroot: {}",
                current_sysroot.display()
            );

            let result: PathBuf = toolchain_build::build_toolchain(
                current_sysroot,
                rustc_config.opts.target_triple.tuple(),
                rustc_config.output_dir.as_deref(),
            )
            .unwrap_or_else(|e| {
                log_error!("Failed to build the toolchain: {}", e);
                std::process::exit(1);
            });
            assert!(is_sysroot_compatible(current_sysroot, Some(&result)));
            result
        }
    }
}

pub mod constants {
    use const_format::concatcp;

    pub(super) const CRATE_NAME_PREFIX_BUILD_SCRIPT: &str = "build_script_";

    // The instrumented code is going to call the shim.
    pub(super) const CRATE_RUNTIME_SHIM: &str = "leafrtsh";

    pub(crate) const CONFIG_ENV_PREFIX: &str = "LEAFC";

    pub(super) const CONFIG_CORE_BUILD: &str = "core_build";

    pub(super) const URL_BUG_REPORT: &str = "https://github.com/sfu-rsl/leaf/issues/new";

    pub const ENV_LOG: &str = concatcp!(CONFIG_ENV_PREFIX, "_LOG");
    pub const ENV_LOG_WRITE_STYLE: &str = concatcp!(CONFIG_ENV_PREFIX, "_LOG_STYLE");

    pub const ENV_FORCE_NOOP: &str = concatcp!(CONFIG_ENV_PREFIX, "_FORCE_NOOP");

    pub const LOG_PASS_OBJECTS_TAG: &str = super::passes::logger::TAG_OBJECTS;
    pub const LOG_PRI_DISCOVERY_TAG: &str = super::pri_utils::TAG_DISCOVERY;
    pub const LOG_BB_JUMP_TAG: &str = super::mir_transform::TAG_BB_JUMP;

    pub const TOOL_LEAF: &str = "leaf_attr";

    pub const ENV_RUSTUP_TOOLCHAIN: &str = "RUSTUP_TOOLCHAIN";
}

mod driver_args {
    pub(super) use rustc_session::config::CrateType;

    use super::{utils::file::*, *};

    use std::path::{Path, PathBuf};
    use std::{env, fs, iter};

    const CODEGEN_LINK_ARG: &str = "link-arg";

    const FILE_RUNTIME_DYLIB_DEFAULT: &str = FILE_RUNTIME_DYLIB_BASIC_LATE_INIT;
    #[allow(dead_code)]
    const FILE_RUNTIME_DYLIB_BASIC: &str = "libleafrt_basic.so";
    #[allow(dead_code)]
    const FILE_RUNTIME_DYLIB_BASIC_LATE_INIT: &str = "libleafrt_basic_li.so";
    const FILE_RUNTIME_DYLIB_NOOP: &str = "libleafrt_noop.so";
    #[allow(dead_code)]
    const FILE_RUNTIME_DYLIB: &str = "libleafrt.so";

    const DIR_RUNTIME_DYLIB_DEFAULT: &str = DIR_RUNTIME_DYLIB_BASIC_LATE_INIT;
    #[allow(dead_code)]
    const DIR_RUNTIME_DYLIB_BASIC: &str = "runtime_basic";
    #[allow(dead_code)]
    const DIR_RUNTIME_DYLIB_BASIC_LATE_INIT: &str = "runtime_basic_li";
    #[allow(dead_code)]
    const DIR_RUNTIME_DYLIB_NOOP: &str = "runtime_noop";

    const LIB_RUNTIME: &str = "leafrt";

    const OPT_CODEGEN: &str = "-C";
    const OPT_CRATE_NAME: &str = "--crate-name";
    const OPT_CRATE_TYPE: &str = "--crate-type";
    const OPT_LINK_NATIVE: &str = "-l";
    const OPT_SEARCH_PATH: &str = "-L";

    const SEARCH_KIND_NATIVE: &str = "native";

    const MAX_RETRY: usize = 5;

    // FIXME: #467
    pub(super) struct CrateOptions {
        pub crate_name: Option<String>,
        pub crate_types: Option<Vec<CrateType>>,
    }

    pub(super) trait ArgsExt {
        fn set_if_absent(&mut self, key: &str, get_value: impl FnOnce() -> String);

        fn add_pair(&mut self, key: &str, value: String);

        fn parse_crate_options(&self) -> CrateOptions;
    }

    impl<T: AsRef<Vec<String>> + AsMut<Vec<String>>> ArgsExt for T {
        fn set_if_absent(&mut self, key: &str, get_value: impl FnOnce() -> String) {
            if !self.as_mut().iter().any(|arg| arg.starts_with(key)) {
                self.add_pair(key, get_value());
            }
        }

        fn add_pair(&mut self, key: &str, value: String) {
            self.as_mut().push(key.to_owned());
            self.as_mut().push(value);
        }

        fn parse_crate_options(&self) -> CrateOptions {
            let find_crate_name = || -> Option<String> {
                let index = self
                    .as_ref()
                    .iter()
                    .rposition(|arg| arg == OPT_CRATE_NAME)?
                    + 1;
                self.as_ref().get(index).cloned()
            };

            let find_crate_types = || -> Option<Vec<CrateType>> {
                let index = self.as_ref().iter().position(|arg| arg == OPT_CRATE_TYPE)? + 1;
                let types = rustc_session::config::parse_crate_types_from_list(vec![
                    self.as_ref().get(index).cloned()?,
                ]);
                types.ok()
            };

            CrateOptions {
                crate_name: find_crate_name(),
                crate_types: find_crate_types(),
            }
        }
    }

    pub(super) fn set_up_args(given_args: impl IntoIterator<Item = String>) -> Vec<String> {
        let mut args = given_args.into_iter().collect::<Vec<_>>();

        let crate_options = args.parse_crate_options();

        /* Add linking to the runtime dynamic library for all commands.
         * As some crates might be built with the version of the core library
         * with embedded runtime shim, we add it anyway. It won't be effective
         * if there is no linking required.
         * Related to #462. */
        // FIXME: Use the parsed config instead.
        set_up_runtime_dylib(&mut args, &crate_options);

        if is_ineffective_crate(&crate_options) {
            return args;
        }

        args
    }

    fn set_up_runtime_dylib(args: &mut Vec<String>, opts: &CrateOptions) {
        let use_noop_runtime = is_ineffective_crate(opts);

        ensure_runtime_dylib_exists(use_noop_runtime);
        let runtime_dylib_dir = find_runtime_dylib_dir(use_noop_runtime)
            .to_string_lossy()
            .to_string();
        // Add the runtime dynamic library as a dynamic dependency.
        /* NOTE: As long as the shim is getting compiled along with the program,
         * adding it explicitly should not be necessary (is expected to be
         * realized by the compiler). */
        args.add_pair(OPT_LINK_NATIVE, format!("dylib={}", LIB_RUNTIME));
        /* Add the RPATH header to the binary,
         * so there will be a default path to look for the library and including
         * it in `LD_LIBRARY_PATH` won't be necessary. */
        args.add_pair(
            OPT_CODEGEN,
            format!("{CODEGEN_LINK_ARG}=-Wl,-rpath={}", runtime_dylib_dir),
        );
        // Also include it in the search path for Rust.
        args.add_pair(
            OPT_SEARCH_PATH,
            format!("{SEARCH_KIND_NATIVE}={}", runtime_dylib_dir),
        );
    }

    fn ensure_runtime_dylib_exists(use_noop_runtime: bool) {
        ensure_runtime_dylib_dir_exist(use_noop_runtime);
        let runtime_dylib_dir = PathBuf::from(find_runtime_dylib_dir(use_noop_runtime));

        fn sym_link_exists(sym_path: &Path) -> bool {
            fs::symlink_metadata(sym_path).is_ok()
        }

        let sym_dylib_path = runtime_dylib_dir.join(FILE_RUNTIME_DYLIB);
        if sym_link_exists(&sym_dylib_path) && sym_dylib_path.exists() {
            return;
        }

        let physical_dylib_path = if use_noop_runtime {
            find_dependency_path(FILE_RUNTIME_DYLIB_NOOP, iter::empty())
        } else {
            find_dependency_path(FILE_RUNTIME_DYLIB_DEFAULT, iter::empty())
        };

        // NOTE: Parallel execution of the compiler may cause race conditions.
        // FIXME: Come up with a better solution.
        retry(MAX_RETRY, std::time::Duration::from_secs(1), || {
            if sym_link_exists(&sym_dylib_path) {
                if sym_dylib_path.exists() {
                    return Ok(());
                } else {
                    // Invalid symbolic link.
                    fs::remove_file(&sym_dylib_path)?;
                }
            }

            #[cfg(unix)]
            let result = std::os::unix::fs::symlink(&physical_dylib_path, &sym_dylib_path);
            #[cfg(windows)]
            let result = std::os::windows::fs::symlink_file(&physical_dylib_path, &sym_dylib_path);
            result
        })
        .expect("Could not create a symlink to the fallback runtime dylib.");
    }

    fn ensure_runtime_dylib_dir_exist(use_noop_runtime: bool) {
        let runtime_dylib_folder = get_runtime_dylib_folder(use_noop_runtime);
        // FIXME: Come up with a better solution.
        retry(MAX_RETRY, std::time::Duration::from_secs(1), || {
            if try_find_dependency_path(runtime_dylib_folder, iter::empty()).is_none() {
                let runtime_dylib_dir = env::current_exe()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join(runtime_dylib_folder);
                std::fs::create_dir(&runtime_dylib_dir)
            } else {
                Ok(())
            }
        })
        .expect("Could not create a symlink to the fallback runtime dylib.");
    }

    fn find_runtime_dylib_dir(use_noop_runtime: bool) -> PathBuf {
        find_dependency_path(get_runtime_dylib_folder(use_noop_runtime), iter::empty())
    }

    fn get_runtime_dylib_folder(use_noop_runtime: bool) -> &'static str {
        if use_noop_runtime {
            DIR_RUNTIME_DYLIB_NOOP
        } else {
            DIR_RUNTIME_DYLIB_DEFAULT
        }
    }

    fn retry<T, E>(
        times: usize,
        sleep_dur: std::time::Duration,
        mut f: impl FnMut() -> Result<T, E>,
    ) -> Result<T, E> {
        let mut result = f();
        for _ in 0..times {
            if result.is_ok() {
                break;
            } else {
                std::thread::sleep(sleep_dur);
            }
            result = f();
        }
        result
    }
}
