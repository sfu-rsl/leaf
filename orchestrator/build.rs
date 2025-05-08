use std::path::{Path, PathBuf};

use common::building::utils::workspace_path;

const PATH_REACHABILITY_ANALYZER: [&str; 3] = ["scripts", "analyzers", "Reachability.dl"];

const ENV_REACHABILITY_ANALYZER_PATH: &str = "LEAFO_TOOL_REACHABILITY";

fn main() {
    let workspace_dir = workspace_path();

    set_tool_path(
        ENV_REACHABILITY_ANALYZER_PATH,
        &reachability::provide(&workspace_dir),
    );
}

mod reachability {

    use std::{fs, process::Command};

    use common::building::utils::output_dir;

    use super::*;

    pub(super) fn provide(workspace_dir: &Path) -> PathBuf {
        const NAME_WRAPPER: &str = "calc_reachability";

        let program_path = workspace_dir.join(PathBuf::from_iter(PATH_REACHABILITY_ANALYZER));
        println!("cargo:rerun-if-changed={}", program_path.display());
        let compiled_program_path = compile_souffle_program(&program_path);

        let wrapper_path = program_path.parent().unwrap().join(NAME_WRAPPER);
        println!("cargo:rerun-if-changed={}", wrapper_path.display());
        let out_wrapper_path = compiled_program_path.parent().unwrap().join(NAME_WRAPPER);
        fs::copy(wrapper_path, &out_wrapper_path).expect("Failed to copy the wrapper script");
        out_wrapper_path
    }

    fn compile_souffle_program(program_path: &Path) -> PathBuf {
        let exe_path = output_dir().join(program_path.with_extension("").file_name().unwrap());
        println!("{exe_path:?} {}", output_dir().display());
        Command::new("souffle")
            .current_dir(output_dir())
            .args([&program_path.display().to_string()])
            .args(["-o", &exe_path.display().to_string()])
            .args([
                "-j",
                &std::thread::available_parallelism()
                    .map(Into::into)
                    .unwrap_or(1)
                    .to_string(),
            ])
            .status()
            .expect("Failed to execute Souffle. Make sure it is installed and available in PATH");
        exe_path
    }
}

fn set_tool_path(env_name: &str, path: &Path) {
    println!("cargo:rustc-env={env_name}={}", path.display())
}
