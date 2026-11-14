use std::env;
use std::format;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::string::ToString;
use std::vec;
use std::vec::Vec;

const ENV_LEAFC: &str = "LEAFC";

pub const RUST_FLAGS: [&str; 2] = ["-Awarnings", "-Coverflow-checks=off"];
pub const ENV_RUST_BACKTRACE: &str = "RUST_BACKTRACE";

const ENV_LEAFC_LOG: &str = "LEAFC_LOG";
const ENV_LEAFC_RUNTIME_SHIM_EXTERNAL_CRATE_NAME: &str =
    "LEAFC_RUNTIME_SHIM__LOCATION__EXTERNAL__CRATE_NAME";
const ENV_CARGO_TARGET_DIR: &str = "CARGO_TARGET_DIR";
const ENV_RUSTC: &str = "RUSTC";
const ENV_RUSTFLAGS: &str = "RUSTFLAGS";
pub const ENV_LEAFC_RUNTIME_SHIM_EXTERNAL_SEARCH: &str =
    "LEAFC_RUNTIME_SHIM__LOCATION__EXTERNAL__SEARCH_PATH";
pub const ENV_LEAFC_CODEGEN_ALL_MIR: &str = "LEAFC_CODEGEN_ALL_MIR";
// Source: github.com/rust-lang/cargo/blob/master/crates/cargo-util/src/paths.rs
const ENV_DYLIB_PATH: &str = if cfg!(windows) {
    "PATH"
} else if cfg!(target_os = "macos") {
    "DYLD_FALLBACK_LIBRARY_PATH"
} else if cfg!(target_os = "aix") {
    "LIBPATH"
} else if cfg!(target_os = "haiku") {
    "LIBRARY_PATH"
} else {
    "LD_LIBRARY_PATH"
};

const LOG_CONFIG: &str = "info";
const FILE_RUNTIME_DYLIB: &str = "libleafrt.so";

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|path| path.join("compiler").join("Cargo.toml").is_file())
        .expect("Failed to locate the workspace root")
        .to_path_buf()
}

pub fn create_temp_dir() -> PathBuf {
    use std::time::SystemTime;
    let mut path = env::temp_dir();
    path.push(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string(),
    );
    fs::create_dir_all(&path).unwrap();
    path
}

pub fn path_in_proj_root(name: &str) -> PathBuf {
    workspace_root().join(name)
}

pub fn set_leafc_env(cmd: &mut Command) -> &mut Command {
    cmd.get_envs()
        .filter_map(|(key, _)| key.to_str())
        .filter(|key| key.starts_with("CARGO_"))
        .map(str::to_string)
        .collect::<Vec<_>>()
        .into_iter()
        .for_each(|key| {
            cmd.env_remove(key);
        });
    cmd.env(ENV_LEAFC_LOG, LOG_CONFIG)
        .env(ENV_LEAFC_RUNTIME_SHIM_EXTERNAL_CRATE_NAME, "leaf")
        .env(ENV_LEAFC_RUNTIME_SHIM_EXTERNAL_SEARCH, "compiler")
        .env(ENV_LEAFC_CODEGEN_ALL_MIR, "false")
        .env(ENV_RUST_BACKTRACE, "1")
}

pub fn set_out_log_files(cmd: &mut Command, work_dir: &Path) {
    cmd.stdout(fs::File::create(work_dir.join("stdout.log")).unwrap())
        .stderr(fs::File::create(work_dir.join("stderr.log")).unwrap());
}

pub fn compile_sample_crate(source_dir: &str, leafc: &Path) {
    let output_dir = create_temp_dir();
    let mut cmd = Command::new("cargo");
    set_leafc_env(&mut cmd);

    cmd.arg("build")
        .env(ENV_CARGO_TARGET_DIR, &output_dir)
        .env(ENV_RUSTC, leafc.as_os_str())
        .env(ENV_RUSTFLAGS, RUST_FLAGS.join(" "))
        .env(ENV_LEAFC_RUNTIME_SHIM_EXTERNAL_SEARCH, "sysroot")
        .env(ENV_LEAFC_CODEGEN_ALL_MIR, "true")
        .current_dir(path_in_proj_root(source_dir));

    set_out_log_files(&mut cmd, &output_dir);

    let status = cmd
        .status()
        .expect("Failed to spawn and wait for command termination");
    assert!(
        status.success(),
        "Failed to compile {} with exit code: {}, output available in {}",
        source_dir,
        status.code().unwrap_or(-1),
        output_dir.display()
    );

    let _ = fs::remove_dir_all(output_dir);
}

pub fn set_runtime_library_path(cmd: &mut Command, runtime_dir: &Path) {
    let mut paths = vec![runtime_dir.to_path_buf()];
    if let Some(existing_paths) = env::var_os(ENV_DYLIB_PATH) {
        paths.extend(env::split_paths(&existing_paths));
    }
    let paths = env::join_paths(paths).expect("Failed to construct runtime library search path");
    cmd.env(ENV_DYLIB_PATH, paths);
}

pub fn compile_sample(src_file: &Path, leafc: &Path, backend: Option<&str>) -> CompilationResult {
    let work_dir = create_temp_dir();
    if let Some(backend) = backend {
        link_test_runtime(backend, &work_dir);
    }
    let mut cmd = Command::new(leafc);

    const EXE_NAME: &str = "program";
    cmd.current_dir(&work_dir)
        .args(RUST_FLAGS)
        .arg("--edition=2021")
        .arg("-L")
        .arg(format!("native={}", work_dir.display()));

    let output = work_dir.join(EXE_NAME);
    cmd.args(["-o", output.to_str().unwrap()]);

    set_leafc_env(&mut cmd);
    set_out_log_files(&mut cmd, &work_dir);
    cmd.arg(src_file);

    cmd.status()
        .map(|status| CompilationResult {
            status,
            work_dir,
            output,
        })
        .expect("Failed to spawn and wait for command termination")
}

pub fn compile_and_run_sample(src_file: &Path, leafc: &Path, backend: Option<&str>) {
    let compilation_result = compile_sample(src_file, leafc, backend);
    assert!(
        compilation_result.status.success(),
        "Failed to compile {}",
        src_file.display()
    );

    let work_dir = compilation_result.work_dir;
    let mut command = Command::new(compilation_result.output);
    command.current_dir(&work_dir).env(ENV_RUST_BACKTRACE, "1");
    set_runtime_library_path(&mut command, &work_dir);

    let execution_status = command.status().unwrap_or_else(|error| {
        panic!(
            "Failed to spawn and wait for the compiled binary: {error}, output available in: {}",
            work_dir.display()
        )
    });
    assert!(
        execution_status.success(),
        "Failed to execute {} with exit code: {}, output available in {}",
        src_file.display(),
        execution_status.code().unwrap_or(-1),
        work_dir.display()
    );

    let _ = fs::remove_dir_all(work_dir);
}

fn link_test_runtime(backend: &str, work_dir: &Path) {
    let runtime_name = format!(
        "{}leafrt_{backend}{}",
        env::consts::DLL_PREFIX,
        env::consts::DLL_SUFFIX
    );
    let search_paths = env::var_os(ENV_DYLIB_PATH)
        .map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default();
    let runtime = search_paths
        .into_iter()
        .map(|path| path.join(&runtime_name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| {
            panic!(
                "Runtime artifact {runtime_name} for backend {backend} was not found in Cargo's {ENV_DYLIB_PATH} search paths"
            )
        });
    let link = work_dir.join(FILE_RUNTIME_DYLIB);
    link_runtime(&runtime, &link);
}

fn link_runtime(runtime: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(runtime, link).unwrap_or_else(|error| {
        panic!(
            "Failed to link test runtime {} as {}: {error}",
            runtime.display(),
            link.display()
        )
    });
    #[cfg(not(unix))]
    panic!("Test runtime linking is only supported on Unix");
}

pub struct CompilationResult {
    pub status: ExitStatus,
    pub work_dir: PathBuf,
    pub output: PathBuf,
}

pub fn run_sample_sanity(fic: &str, backend: &str) {
    compile_and_run_sample(&path_in_proj_root(fic), &path_leafc(), Some(backend));
}

fn path_leafc() -> PathBuf {
    let workspace_root = workspace_root();
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("target"));
    let target_dir = if target_dir.is_absolute() {
        target_dir
    } else {
        workspace_root.join(target_dir)
    };
    let path = env::var_os(ENV_LEAFC)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            target_dir
                .join("debug")
                .join(format!("leafc{}", env::consts::EXE_SUFFIX))
        });
    assert!(
        path.is_file(),
        "leafc was not found at {}; build it first or set {ENV_LEAFC}",
        path.display()
    );
    path
}
