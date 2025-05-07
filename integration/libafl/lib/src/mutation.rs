use core::iter;
use std::{
    borrow::Cow,
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use common::conc_loop::GeneratedInputRecord;
use libafl::{
    Error, HasMetadata,
    corpus::Testcase,
    inputs::{HasMutatorBytes, Input},
    mutators::MultiMutator,
    schedulers::TestcaseScore,
};
use libafl_bolts::{Named, SerdeAny};

use derive_more as dm;
use serde::{Deserialize, Serialize};

pub struct DivergingMutator {
    orchestrator_path: PathBuf,
    orchestrator_args: Box<[String]>,
    program_path: PathBuf,
    program_args: Box<[String]>,
    work_dir: PathBuf,
    input_path: Option<PathBuf>,
    command: Option<Command>,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, SerdeAny, dm::From)]
pub struct DivergingInputScore(f64);

pub struct DivergingInputTestcaseScore;

#[derive(Debug, Default, Clone, Serialize, Deserialize, SerdeAny)]
struct DivergingMutatorMetadata {
    input_scores: HashMap<String, DivergingInputScore>,
}

impl<I, S> TestcaseScore<I, S> for DivergingInputTestcaseScore
where
    I: Input,
    S: HasMetadata,
{
    fn compute(state: &S, entry: &mut Testcase<I>) -> Result<f64, Error> {
        if let Ok(score) = entry.metadata::<DivergingInputScore>() {
            return Ok(score.0);
        }

        let key = entry
            .filename()
            .clone()
            .or_else(|| entry.input().as_ref().map(|i| i.generate_name(None)));
        let score = key.and_then(|key| {
            state
                .metadata::<DivergingMutatorMetadata>()
                .ok()
                .map(|m| &m.input_scores)
                .and_then(|m| m.get(&key))
        });

        if let Some(score) = score {
            entry.add_metadata(score.clone());
            return Ok(score.0);
        }

        Ok(0.0)
    }
}

impl DivergingMutator {
    pub fn new(
        orchestrator_path: &Path,
        orchestrator_args: Box<[String]>,
        program_path: &Path,
        program_args: Box<[String]>,
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
    S: HasMetadata,
{
    fn multi_mutate(
        &mut self,
        state: &mut S,
        input: &I,
        max_count: Option<usize>,
    ) -> Result<Vec<I>, Error> {
        let input_scores = &mut state
            .metadata_or_insert_with::<DivergingMutatorMetadata>(Default::default)
            .input_scores;
        input_scores.clear();

        let mutants = self.run_input(input)?;
        let max_count = max_count.unwrap_or(mutants.len());
        mutants
            .into_iter()
            .rev()
            .take(max_count)
            .map(|r| {
                I::from_file(r.path).inspect(|input| {
                    if let Some(score) = r.score {
                        input_scores.insert(input.generate_name(None), score.into());
                    }
                })
            })
            .try_collect::<Vec<I>>()
    }
}

impl DivergingMutator {
    fn run_input<I: Input>(&mut self, input: &I) -> Result<Vec<GeneratedInputRecord>, Error> {
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

        let mutants = serde_json::Deserializer::from_slice(output.stdout.as_slice())
            .into_iter()
            .map(|r| {
                r.map(|mut r: GeneratedInputRecord| {
                    if !r.path.is_absolute() {
                        r.path = self.work_dir.join(r.path)
                    }
                    r
                })
            })
            .try_collect::<Vec<_>>()
            .map_err(|e| Error::os_error(e.into(), "Failed to read orchestrator's output"))?;
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
            const ARG_OUTPUT_FORMAT: &str = "--output-format";
            let mut cmd = Command::new(&self.orchestrator_path);
            cmd.current_dir(&self.work_dir)
                .args([ARG_PROGRAM, &self.program_path.to_string_lossy()])
                .args([ARG_STDIN, &self.get_input_path().to_string_lossy()])
                .args([
                    ARG_OUT_DIR,
                    &self.work_dir.join("mutants").to_string_lossy(),
                ])
                .arg(ARG_SILENT)
                .args([ARG_OUTPUT_FORMAT, "jsonl"])
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
