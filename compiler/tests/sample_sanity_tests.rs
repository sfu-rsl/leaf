use std::fs;
use std::path::Path;

use common::testing::sanity_tests::{
    compile_and_run_sample, compile_sample, compile_sample_crate, path_in_proj_root,
};
use macros::{gen_sample_crate_tests, gen_sample_tests_rs};

const PATH_LEAFC: &str = env!("CARGO_BIN_EXE_leafc");

const FILENAME_TYPES: &str = common::type_info::rw::FILENAME_DB;

#[gen_sample_tests_rs(
    folder = "samples",
    ignore_files = [".sanity_test.ignore"],
    backend = noop
)]
fn test_sample_compile_rs(fic: &str, backend: &str) {
    let result = compile_sample(
        &path_in_proj_root(fic),
        Path::new(PATH_LEAFC),
        Some(backend),
    );

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

#[gen_sample_crate_tests(
    folder = "samples/crates",
    ignore_files = [".sanity_test.crates.ignore"]
)]
fn test_crates_compile_toml(source_dir: &str) {
    compile_sample_crate(source_dir, Path::new(env!("CARGO_BIN_EXE_leafc")));
}

#[gen_sample_tests_rs(
    folder = "samples",
    ignore_files = [
        ".sanity_test.ignore",
        ".sanity_test.runtime.ignore"
    ]
)]
fn test_sample_sanity_rs(fic: &str) {
    compile_and_run_sample(&path_in_proj_root(fic), Path::new(PATH_LEAFC), None);
}
