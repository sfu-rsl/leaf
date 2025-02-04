use ignore::WalkBuilder;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::ExitStatus;

use macros::{gen_tests_rs, gen_tests_toml};

const ENV_LEAFC_LOG: &str = "LEAFC_LOG";
const ENV_LEAFC_RUNTIME_SHIM_EXTERNAL_CRATE_NAME: &str =
    "LEAFC_RUNTIME_SHIM__LOCATION__EXTERNAL__CRATE_NAME";
const ENV_LEAFC_RUNTIME_SHIM_EXTERNAL_SEARCH: &str =
    "LEAFC_RUNTIME_SHIM__LOCATION__EXTERNAL__SEARCH_PATH";
const ENV_LEAFC_CODEGEN_ALL_MIR: &str = "LEAFC_CODEGEN_ALL_MIR";
const ENV_RUSTC: &str = "RUSTC";
const ENV_RUST_FLAGS: &str = "RUSTFLAGS";
const ENV_RUST_BACKTRACE: &str = "RUST_BACKTRACE";
const LOG_CONFIG: &str = "info";

// Disable warnings to avoid polluting the output
const RUST_FLAGS: [&str; 2] = ["-Awarnings", "-Coverflow-checks=off"];

const PATH_LEAFC: &str = env!("CARGO_BIN_EXE_leafc");

const DIR_SAMPLES_ROOT: &str = "samples";
const FILENAME_TYPES: &str = "types.json";
const FILENAME_EXECUTION_IGNORE: &str = ".test_execution_rs.ignore";
const FILENAME_COMPILE_IGNORE: &str = ".test_compile_rs.ignore";

#[gen_tests_rs("folder=samples,ignore_files=[.test_compile_rs.ignore]")]
fn test_compile_rs(fic: &str) {
    let result = run_compilation(&path_in_proj_root(fic));

    assert!(
        result.status.success(),
        "Failed to compile {} with exit code: {}, output available in {}",
        fic,
        result.status.code().unwrap_or(-1),
        result.work_dir.display()
    );
    assert!(
        result
            .output
            .parent()
            .unwrap()
            .join(FILENAME_TYPES)
            .exists(),
        "Types file not found, output available in {}",
        result.work_dir.display()
    );

    let _ = fs::remove_dir_all(result.work_dir);
}

#[gen_tests_toml("samples/crates")]
fn test_compile_toml(source_dir: &str) {
    let output_dir = create_temp_dir();

    let mut cmd = Command::new("cargo");
    set_leafc_env(&mut cmd);

    cmd.arg("build")
        .env("CARGO_TARGET_DIR", &output_dir)
        .env(ENV_RUSTC, env!("CARGO_BIN_EXE_leafc"))
        .env(ENV_RUST_FLAGS, RUST_FLAGS.join(" "))
        .env(ENV_LEAFC_RUNTIME_SHIM_EXTERNAL_SEARCH, "sysroot")
        .env(ENV_LEAFC_CODEGEN_ALL_MIR, "true")
        .current_dir(path_in_proj_root(source_dir));

    set_out_log_files(&mut cmd, &output_dir);

    let status = cmd
        .status()
        .expect("Failed to spawn and wait for command termination");

    if !status.success() {
        panic!(
            "Failed to compile {} with exit code: {}, output available in {}",
            source_dir,
            status.code().unwrap_or(-1),
            output_dir.display()
        );
    } else {
        let _ = fs::remove_dir_all(output_dir);
    }
}

#[gen_tests_rs("folder=samples,ignore_files=[.test_compile_rs.ignore,.test_execution_rs.ignore]")]
fn test_execution_rs(fic: &str) {
    let compilation_result = run_compilation(&path_in_proj_root(fic));
    assert!(
        compilation_result.status.success(),
        "Failed to compile {}",
        fic
    );

    let work_dir = compilation_result.work_dir;
    let execution_status = Command::new(compilation_result.output)
        .current_dir(&work_dir)
        .env(ENV_RUST_BACKTRACE, "1")
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "Failed to spawn and wait for the compiled binary: {}, output available in: {}",
                e.to_string(),
                work_dir.display()
            )
        });

    assert!(
        execution_status.success(),
        "Failed to execute {} with exit code: {}, output available in: {}",
        fic,
        execution_status.code().unwrap_or(-1),
        work_dir.display()
    );

    let _ = fs::remove_dir_all(work_dir);
}

fn create_temp_dir() -> std::path::PathBuf {
    use std::time::SystemTime;
    let mut path = std::env::temp_dir();
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

fn path_in_proj_root(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(name)
}

fn set_leafc_env(cmd: &mut Command) -> &mut Command {
    cmd.get_envs()
        .filter_map(|(k, _)| k.to_str())
        .filter(|k| k.starts_with("CARGO_"))
        .map(|k| k.to_string())
        .collect::<Vec<_>>()
        .into_iter()
        .for_each(|k| {
            cmd.env_remove(k);
        });
    cmd.env(ENV_LEAFC_LOG, LOG_CONFIG)
        .env(ENV_LEAFC_RUNTIME_SHIM_EXTERNAL_CRATE_NAME, "leaf")
        .env(ENV_LEAFC_RUNTIME_SHIM_EXTERNAL_SEARCH, "compiler")
        .env(ENV_LEAFC_CODEGEN_ALL_MIR, "false")
        .env(ENV_RUST_BACKTRACE, "1")
}

fn set_out_log_files(cmd: &mut Command, work_dir: &Path) {
    cmd.stdout(fs::File::create(work_dir.join("stdout.log")).unwrap())
        .stderr(fs::File::create(work_dir.join("stderr.log")).unwrap());
}

fn run_compilation(src_file: &Path) -> CompilationResult {
    let work_dir = create_temp_dir();
    let mut cmd = Command::new(PATH_LEAFC);

    const EXE_NAME: &str = "program";
    cmd.current_dir(&work_dir)
        .args(RUST_FLAGS)
        .arg("--edition=2021");

    let out_file = work_dir.join(EXE_NAME);
    cmd.args(["-o", out_file.to_str().unwrap()]);

    set_leafc_env(&mut cmd);
    set_out_log_files(&mut cmd, &work_dir);

    cmd.arg(src_file);

    cmd.status()
        .map(|status| CompilationResult {
            status,
            work_dir,
            output: out_file,
        })
        .expect("Failed to spawn and wait for command termination")
}

struct CompilationResult {
    status: ExitStatus,
    work_dir: PathBuf,
    output: PathBuf,
}
