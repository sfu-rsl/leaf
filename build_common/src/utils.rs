use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const DIR_DEPS: &str = "deps";

pub fn workspace_path() -> PathBuf {
    let output = Command::new(env!("CARGO"))
        .arg("locate-project")
        .arg("--workspace")
        .arg("--message-format=plain")
        .output()
        .unwrap()
        .stdout;
    Path::new(std::str::from_utf8(&output).unwrap().trim())
        .parent()
        .unwrap()
        .to_path_buf()
}

pub fn recreate_dir(dir: &Path) {
    if dir.exists() {
        fs::remove_dir_all(dir).unwrap_or_else(|_| panic!("Failed to remove {:?}", dir));
    }
    fs::create_dir_all(dir).unwrap_or_else(|_| panic!("Failed to create {:?}", dir));
}

pub fn artifacts_path() -> PathBuf {
    /* At the moment, no cleaner way is found to get the artifacts path.
     * This is based on the default structure: target/profile/build/<crate>/out */
    PathBuf::from(env::var("OUT_DIR").unwrap())
        .ancestors()
        .skip(3) // profile/build/<crate>/out -> profile
        .next()
        .unwrap()
        .to_path_buf()
}

pub fn deps_path() -> PathBuf {
    artifacts_path().join(DIR_DEPS)
}

pub fn get_current_sysroot() -> PathBuf {
    let output = Command::new(env::var("RUSTC").unwrap())
        .args(["--print", "sysroot"])
        .output()
        .unwrap();
    output.status.exit_ok().unwrap();
    PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
}

// FIXME: Duplicate code
pub fn retry<T, E>(
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
