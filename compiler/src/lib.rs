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

mod config;
mod mir_transform;
mod passes;
mod pri_utils;
mod utils;
mod visit;

extern crate rustc_abi;
extern crate rustc_apfloat;
extern crate rustc_ast;
extern crate rustc_attr;
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
extern crate rustc_mir_transform;
extern crate rustc_monomorphize;
extern crate rustc_query_system;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_target;
extern crate rustc_trait_selection;
extern crate rustc_type_ir;
extern crate thin_vec;

use rustc_driver::RunCompiler;

use common::{log_debug, log_info, log_warn};
use std::path::PathBuf;

use constants::*;

pub fn set_up_compiler() {
    use rustc_session::{config::ErrorOutputType, EarlyDiagCtxt};

    rustc_driver::init_rustc_env_logger(&EarlyDiagCtxt::new(ErrorOutputType::default()));
    rustc_driver::install_ice_hook(URL_BUG_REPORT, |_| ());
}

pub fn run_compiler(args: impl Iterator<Item = String>, input_path: Option<PathBuf>) -> i32 {
    let config = config::load_config();

    let args = driver_args::set_up_args(args, input_path, &config);
    log_info!("Running compiler with args: {:?}", args);

    let mut callbacks =
        driver_callbacks::set_up_callbacks(config, driver_args::find_crate_name(&args));

    rustc_driver::catch_with_exit_code(|| RunCompiler::new(&args, callbacks.as_mut()).run())
}

fn should_do_nothing(crate_name: Option<&String>) -> bool {
    if crate_name.is_some_and(|name| name == CRATE_BUILD_SCRIPT) {
        return true;
    }

    false
}

mod driver_callbacks {
    use common::{log_debug, log_info, log_warn};

    use super::{config::LeafCompilerConfig, constants::*, passes::*};
    use crate::utils::chain;

    pub(super) fn set_up_callbacks(
        config: LeafCompilerConfig,
        crate_name: Option<String>,
    ) -> Box<Callbacks> {
        if super::should_do_nothing(crate_name.as_ref()) {
            log_info!("Leafc will work as the normal Rust compiler.");
            Box::new(NoOpPass.to_callbacks())
        } else {
            let mut passes = if config.codegen_all_mir && is_dependency_crate(crate_name.as_ref()) {
                build_dep_passes_in_codegen_all_mode()
            } else {
                build_primary_passes(&config)
            };
            passes.set_leaf_config(config);
            passes.add_config_callback(Box::new(move |rustc_config, leafc_config| {
                if leafc_config.codegen_all_mir {
                    config_codegen_all_mode(rustc_config, leafc_config);
                }
            }));
            passes
        }
    }

    fn build_primary_passes(config: &LeafCompilerConfig) -> Box<Callbacks> {
        let prerequisites_pass = RuntimeExternCrateAdder::new(
            config.runtime_shim.crate_name.clone(),
            config.runtime_shim.as_external,
        );

        let passes = chain!(
            prerequisites_pass,
            <LeafToolAdder>,
            <TypeExporter>,
            Instrumentor::new(true, None /* FIXME */),
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

    fn build_dep_passes_in_codegen_all_mode() -> Box<Callbacks> {
        /* In this mode, we only internalize the items in the compiled objects,
         * and do the instrumentation with the final crate. */
        Box::new(
            chain!(
                force_codegen_all_pass(),
                <MonoItemInternalizer>,
            )
            .to_callbacks(),
        )
    }

    const SHOULD_CODEGEN_FLAGS: u8 = OverrideFlags::SHOULD_CODEGEN.bits();

    fn force_codegen_all_pass() -> OverrideFlagsForcePass<SHOULD_CODEGEN_FLAGS> {
        /* We must enable overriding should_codegen to force the codegen for all items.
         * Currently, overriding it equals to forcing the codegen for all items. */
        OverrideFlagsForcePass::<SHOULD_CODEGEN_FLAGS>::default()
    }

    fn config_codegen_all_mode(
        rustc_config: &mut rustc_interface::Config,
        leafc_config: &mut LeafCompilerConfig,
    ) {
        assert!(
            leafc_config.codegen_all_mir,
            "This function is meant for codegen all mode."
        );
        rustc_config.opts.unstable_opts.always_encode_mir = true;
        rustc_config
            .opts
            .cli_forced_codegen_units
            .replace(1)
            .inspect(|old| {
                log_warn!(
                    concat!(
                        "Forcing codegen units to 1 because of compilation mode. ",
                        "The requested value was: {:?}",
                    ),
                    old,
                );
            });
        if rustc_config.opts.maybe_sysroot.is_none() {
            let is_building_core = leafc_config.building_core
                || rustc_config
                    .crate_cfg
                    .iter()
                    .any(|cfg| cfg == CONFIG_CORE_BUILD);
            if !is_building_core {
                log_warn!(concat!(
                    "Codegen all MIR is enabled, but the sysroot is not set. ",
                    "It is necessary to use a sysroot with MIR for all libraries included.",
                    "Unless you are building the core library, this may cause issues.",
                ));
            }
        }
    }

    fn is_dependency_crate(crate_name: Option<&String>) -> bool {
        let from_cargo = rustc_session::utils::was_invoked_from_cargo();
        let is_primary = std::env::var("CARGO_PRIMARY_PACKAGE").is_ok();

        log_debug!(
            "Checking if crate `{}` is a dependency. From cargo: {}, Primary Package: {}",
            crate_name.map(|s| s.as_str()).unwrap_or("UNKNOWN"),
            from_cargo,
            is_primary,
        );

        from_cargo && !is_primary
    }
}

pub mod constants {
    use const_format::concatcp;

    pub(super) const CRATE_BUILD_SCRIPT: &str = "build_script_build";

    // The instrumented code is going to call the shim.
    pub(super) const CRATE_RUNTIME: &str = "leafrtsh";

    pub(crate) const CONFIG_ENV_PREFIX: &str = "LEAFC";

    pub(super) const CONFIG_CORE_BUILD: &str = "core_build";

    pub(super) const URL_BUG_REPORT: &str = "https://github.com/sfu-rsl/leaf/issues/new";

    pub(super) const LEAF_AUG_MOD_NAME: &str = "__leaf_augmentation";

    pub const LOG_ENV: &str = concatcp!(CONFIG_ENV_PREFIX, "_LOG");
    pub const LOG_WRITE_STYLE_ENV: &str = concatcp!(CONFIG_ENV_PREFIX, "_LOG_STYLE");

    pub const LOG_PASS_OBJECTS_TAG: &str = super::passes::logger::TAG_OBJECTS;
    pub const LOG_PRI_DISCOVERY_TAG: &str = super::pri_utils::TAG_DISCOVERY;

    pub const TOOL_LEAF: &str = "leaf_attr";
}

mod driver_args {
    use super::*;

    use std::path::{Path, PathBuf};
    use std::{env, fs, iter};

    const CMD_RUSTC: &str = "rustc";
    const CMD_RUSTUP: &str = "rustup";

    const CODEGEN_LINK_ARG: &str = "link-arg";

    const DIR_DEPS: &str = "deps";

    const ENV_RUSTUP_HOME: &str = "RUSTUP_HOME";
    const ENV_SYSROOT: &str = "RUST_SYSROOT";
    const ENV_TOOLCHAIN: &str = "RUSTUP_TOOLCHAIN";

    const FILE_RUNTIME_SHIM_LIB: &str = "libleafrtsh.rlib";

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

    const OPT_EXTERN: &str = "--extern";
    const OPT_CODEGEN: &str = "-C";
    const OPT_CRATE_NAME: &str = "--crate-name";
    const OPT_LINK_NATIVE: &str = "-l";
    const OPT_PRINT_SYSROOT: &str = "--print=sysroot";
    const OPT_SYSROOT: &str = "--sysroot";
    const OPT_SEARCH_PATH: &str = "-L";
    const OPT_UNSTABLE: &str = "-Zunstable-options";

    const PATH_SHIM_LIB_LOCATION: &str = env!("SHIM_LIB_LOCATION"); // Set by the build script.

    const SEARCH_KIND_TRANS_DEP: &str = "dependency";
    const SEARCH_KIND_NATIVE: &str = "native";

    const SUFFIX_OVERRIDE: &str = "(override)";

    const MAX_RETRY: usize = 5;

    macro_rules! read_var {
        ($name:expr) => {{ env::var($name).ok() }};
    }

    trait ArgsExt {
        fn set_if_absent(&mut self, key: &str, get_value: impl FnOnce() -> String);

        fn add_pair(&mut self, key: &str, value: String);
    }

    impl<T: AsMut<Vec<String>>> ArgsExt for T {
        fn set_if_absent(&mut self, key: &str, get_value: impl FnOnce() -> String) {
            if !self.as_mut().iter().any(|arg| arg.starts_with(key)) {
                self.add_pair(key, get_value());
            }
        }

        fn add_pair(&mut self, key: &str, value: String) {
            self.as_mut().push(key.to_owned());
            self.as_mut().push(value);
        }
    }

    pub(super) fn set_up_args(
        given_args: impl Iterator<Item = String>,
        input_path: Option<PathBuf>,
        config: &crate::config::LeafCompilerConfig,
    ) -> Vec<String> {
        // Although the driver throws out the first argument, we set the correct value for it.
        let given_args = std::iter::once(
            env::current_exe()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        )
        .chain(given_args);
        let mut args = given_args.collect::<Vec<_>>();

        if should_do_nothing(find_crate_name(&args).as_ref()) {
            return args;
        }

        if config.set_sysroot {
            args.set_if_absent(OPT_SYSROOT, find_sysroot);
        }

        args.push(OPT_UNSTABLE.to_owned());

        if config.runtime_shim.as_external {
            // Add the runtime shim library as a direct external dependency.
            let shim_lib_path = find_shim_lib_path();
            args.add_pair(OPT_EXTERN, format!("{}={}", CRATE_RUNTIME, shim_lib_path));
            // Add the runtime shim library dependencies into the search path.
            args.add_pair(
                OPT_SEARCH_PATH,
                format!(
                    "{SEARCH_KIND_TRANS_DEP}={}",
                    find_shim_lib_deps_path(&shim_lib_path)
                ),
            );
        }

        set_up_runtime_dylib(&mut args);

        if let Some(input_path) = input_path {
            args.push(input_path.to_string_lossy().into_owned());
        }

        args
    }

    fn set_up_runtime_dylib(args: &mut Vec<String>) {
        // FIXME: Add better support for setting the runtime flavor.
        // NOTE: If the compiled target is either a build script or a proc-macro crate type, we should use the noop runtime library.
        let args_str = args.join(" ");
        let use_noop_runtime = args_str.contains(&"--crate-name build_script_build".to_string())
            || args_str.contains(&"feature=\\\"proc-macro\\\"".to_string())
            || args_str.contains(&"--crate-type proc-macro ".to_string());

        ensure_runtime_dylib_exists(use_noop_runtime);
        let runtime_dylib_dir = find_runtime_dylib_dir(use_noop_runtime);
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

    pub(super) fn find_crate_name(args: &[String]) -> Option<String> {
        let index = args.iter().rposition(|arg| arg == OPT_CRATE_NAME)? + 1;
        args.get(index).cloned()
    }

    fn find_sysroot() -> String {
        let try_rustc = || {
            use std::process::Command;
            // Find a nightly toolchain if available.
            // NOTE: It is possible to prioritize the overridden toolchain even if not nightly.
            let toolchain_arg = Command::new(CMD_RUSTUP)
                .args(&["toolchain", "list"])
                .output()
                .ok()
                .filter(|out| out.status.success())
                .and_then(|out| {
                    let lines = std::str::from_utf8(&out.stdout)
                        .ok()?
                        .lines()
                        .filter(|l| l.starts_with("nightly"))
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    Some(lines)
                })
                .and_then(|toolchains| {
                    toolchains
                        .iter()
                        .find_map(|t| t.rfind(SUFFIX_OVERRIDE).map(|i| t[..i].to_owned()))
                        .or(toolchains.first().cloned())
                })
                .map(|t| format!("+{}", t.trim()))
                .unwrap_or_else(|| {
                    log_warn!("Unable to find a nightly toolchain. Using the default one.");
                    Default::default()
                });

            Command::new(CMD_RUSTC)
                .arg(toolchain_arg)
                .arg(OPT_PRINT_SYSROOT)
                .output()
                .ok()
                .filter(|out| {
                    if out.status.success() {
                        true
                    } else {
                        log_debug!("Rustc print sysroot was not successful: {:?}", out);
                        false
                    }
                })
                .map(|out| std::str::from_utf8(&out.stdout).unwrap().trim().to_owned())
        };

        let try_toolchain_env = || {
            read_var!(ENV_RUSTUP_HOME)
                .zip(read_var!(ENV_TOOLCHAIN))
                .map(|(home, toolchain)| format!("{home}/toolchains/{toolchain}"))
        };

        let try_sysroot_env = || read_var!(ENV_SYSROOT);

        // NOTE: Check the priority of these variables in the original compiler.
        try_rustc()
            .or_else(try_toolchain_env)
            .or_else(try_sysroot_env)
            .expect("Unable to find sysroot.")
    }

    fn find_shim_lib_path() -> String {
        find_dependency_path(
            FILE_RUNTIME_SHIM_LIB,
            iter::once(Path::new(PATH_SHIM_LIB_LOCATION)),
        )
    }

    fn find_shim_lib_deps_path(lib_file_path: &str) -> String {
        // Select the `deps` folder next to the lib file.
        find_dependency_path(
            DIR_DEPS,
            iter::once(Path::new(lib_file_path).parent().unwrap()),
        )
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

    fn find_runtime_dylib_dir(use_noop_runtime: bool) -> String {
        find_dependency_path(get_runtime_dylib_folder(use_noop_runtime), iter::empty())
    }

    fn get_runtime_dylib_folder(use_noop_runtime: bool) -> &'static str {
        if use_noop_runtime {
            DIR_RUNTIME_DYLIB_NOOP
        } else {
            DIR_RUNTIME_DYLIB_DEFAULT
        }
    }

    fn find_dependency_path<'a>(
        name: &'static str,
        priority_dirs: impl Iterator<Item = &'a Path>,
    ) -> String {
        try_find_dependency_path(name, priority_dirs)
            .unwrap_or_else(|| panic!("Unable to find the dependency with name: {}", name))
    }

    fn try_find_dependency_path<'a>(
        name: &str,
        mut priority_dirs: impl Iterator<Item = &'a Path>,
    ) -> Option<String> {
        let try_dir = |path: &Path| {
            log_debug!("Trying dir in search of `{}`: {:?}", name, path);
            common::utils::try_join_path(path, name)
        };

        let try_priority_dirs = || priority_dirs.find_map(try_dir);
        let try_cwd = || env::current_dir().ok().and_then(|p| try_dir(&p));
        let try_exe_path = || {
            env::current_exe()
                .ok()
                .and_then(|p| p.ancestors().skip(1).find_map(try_dir))
        };

        None.or_else(try_priority_dirs)
            .or_else(try_cwd)
            .or_else(try_exe_path)
            .map(|path| path.to_string_lossy().to_string())
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
