use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const ENV_LEAFC: &str = "LEAFC";
const ENV_LEAFC_BUILDING_CORE: &str = "LEAFC_BUILDING_CORE";
const ENV_LEAF_WORKSPACE: &str = "LEAF_WORKSPACE";
const ENV_OUT_DIR: &str = "OUT_DIR";
const ENV_RUSTUP_TOOLCHAIN: &str = "RUSTUP_TOOLCHAIN";
const ENV_TARGET: &str = "TARGET";
const ENV_TOOLCHAIN_MARKER: &str = "LEAFS_TOOLCHAIN_MARKER_FILE";
const ENV_WORK_DIR: &str = "WORK_DIR";
const FILE_TOOLCHAIN_MARKER: &str = ".leafc_toolchain";
const TARGETS: [&str; 2] = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"];

#[test]
fn toolchain_builder_runs_successfully() {
    let test_dir = env::temp_dir().join(format!(
        "leaf-toolchain-builder-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before the Unix epoch")
            .as_nanos()
    ));
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("compiler package has a workspace parent")
        .join("scripts")
        .join("toolchain_builder")
        .join("build");
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let leaf_workspace = manifest_dir
        .parent()
        .expect("compiler package has a workspace parent");
    let sysroot = Command::new("rustc")
        .arg("--print=sysroot")
        .output()
        .expect("failed to query the active Rust sysroot");
    assert!(sysroot.status.success(), "rustc --print=sysroot failed");

    for target in TARGETS {
        let target_dir = test_dir.join(target);
        let work_dir = target_dir.join("work");
        let out_dir = target_dir.join("out");
        fs::create_dir_all(&work_dir).expect("failed to create toolchain builder work directory");
        fs::create_dir_all(&out_dir).expect("failed to create toolchain builder output directory");

        let output = Command::new(&script)
            .current_dir(&work_dir)
            .env(ENV_WORK_DIR, &work_dir)
            .env(ENV_OUT_DIR, &out_dir)
            .env(ENV_LEAFC, env!("CARGO_BIN_EXE_leafc"))
            .env(ENV_LEAFC_BUILDING_CORE, "true")
            .env(ENV_LEAF_WORKSPACE, leaf_workspace)
            .env(
                ENV_RUSTUP_TOOLCHAIN,
                String::from_utf8_lossy(&sysroot.stdout).trim(),
            )
            .env(ENV_TARGET, target)
            .env(ENV_TOOLCHAIN_MARKER, FILE_TOOLCHAIN_MARKER)
            .env("LEAFS_ADD_LEAF_AS_DEP", "true")
            .output()
            .expect("failed to execute the toolchain builder");

        assert!(
            output.status.success(),
            "toolchain builder failed for {target} with {}:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );

        let toolchain_output =
            String::from_utf8(output.stdout).expect("toolchain builder output was not UTF-8");
        let toolchain = toolchain_output
            .lines()
            .last()
            .expect("toolchain builder did not report an output path");
        let toolchain = PathBuf::from(toolchain);
        assert!(
            toolchain.is_dir(),
            "toolchain output for {target} does not exist: {}",
            toolchain.display()
        );
        assert!(
            toolchain.join(FILE_TOOLCHAIN_MARKER).is_file(),
            "toolchain marker file for {target} was not created: {}",
            toolchain.display()
        );
    }

    let _ = fs::remove_dir_all(&test_dir);
}
