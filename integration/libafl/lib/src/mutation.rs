use core::iter;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use libafl::{
    Error,
    inputs::{HasMutatorBytes, Input},
    mutators::MultiMutator,
};
use libafl_bolts::Named;

pub struct DivergingMutator {
    orchestrator_path: PathBuf,
    orchestrator_args: Vec<String>,
    program_path: PathBuf,
    program_args: Vec<String>,
    work_dir: PathBuf,
    input_path: Option<PathBuf>,
    command: Option<Command>,
}

impl DivergingMutator {
    pub fn new(
        orchestrator_path: &Path,
        orchestrator_args: Vec<String>,
        program_path: &Path,
        program_args: Vec<String>,
        work_dir: &Path,
    ) -> Self {
        Self {
            orchestrator_path: orchestrator_path.to_path_buf(),
            orchestrator_args,
            program_path: program_path.canonicalize().expect("Invalid program path"),
            program_args,
            work_dir: work_dir.canonicalize().expect("Invalid work directory"),
            input_path: None,
            command: None,
        }
    }
}

impl Named for DivergingMutator {
    fn name(&self) -> &Cow<'static, str> {
        &Cow::Borrowed("LeafDivergingInputMutator")
    }
}

impl<I, S> MultiMutator<I, S> for DivergingMutator
where
    I: Input + HasMutatorBytes,
{
    fn multi_mutate(
        &mut self,
        _state: &mut S,
        input: &I,
        max_count: Option<usize>,
    ) -> Result<Vec<I>, Error> {
        let mutant_paths = self.run_input(input)?;
        let max_count = max_count.unwrap_or(mutant_paths.len());
        mutant_paths
            .into_iter()
            .rev()
            .take(max_count)
            .map(|p| I::from_file(p))
            .try_collect::<Vec<I>>()
    }
}

impl DivergingMutator {
    fn run_input<I: Input>(&mut self, input: &I) -> Result<Vec<PathBuf>, Error> {
        input.to_file(self.get_input_path())?;

        let child = self.get_command().spawn()?;
        let output = child.wait_with_output()?;
        output.status.exit_ok().map_err(|e| {
            Error::unknown(format!(
                "Orchestrator did not exit successfully: {:?} StdErr: {}",
                e,
                String::from_utf8_lossy(output.stderr.as_slice())
            ))
        })?;

        let mutants = String::from_utf8_lossy(output.stdout.as_slice())
            .lines()
            .map(PathBuf::from)
            .map(|p| {
                if p.is_absolute() {
                    p
                } else {
                    self.work_dir.join(p)
                }
            })
            .collect::<Vec<_>>();
        log::debug!("Generated {} diverging inputs", mutants.len());
        Ok(mutants)
    }

    fn get_input_path(&mut self) -> &Path {
        self.input_path.get_or_insert_with(|| {
            debug_assert!(self.work_dir.is_absolute() && self.program_path.is_absolute());
            self.work_dir.join("current_input")
        })
    }

    fn get_command(&mut self) -> &mut Command {
        if self.command.is_none() {
            const ARG_PROGRAM: &str = "--program";
            const ARG_STDIN: &str = "--stdin";
            const ARG_OUT_DIR: &str = "--outdir";
            const ARG_SILENT: &str = "--silent";
            let mut cmd = Command::new(&self.orchestrator_path);
            cmd.current_dir(&self.work_dir)
                .args([ARG_PROGRAM, &self.program_path.to_string_lossy()])
                .args([ARG_STDIN, &self.get_input_path().to_string_lossy()])
                .args([
                    ARG_OUT_DIR,
                    &self.work_dir.join("mutants").to_string_lossy(),
                ])
                .arg(ARG_SILENT)
                .args(self.orchestrator_args.iter())
                .args(iter::once("--").chain(self.program_args.iter().map(String::as_str)))
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            log::debug!("Command to use for diverging input generation: {:?}", cmd);

            self.command = Some(cmd);
        }

        self.command.as_mut().unwrap()
    }
}
