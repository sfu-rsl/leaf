use std::{
    assert_matches::debug_assert_matches,
    collections::HashMap,
    io::{self, Write},
    path::PathBuf,
};

use common::{log_debug, log_info, log_warn};

use crate::{
    abs::IntType,
    outgen,
    utils::file::{FileFormat, FileGenConfig},
};

use super::{config::OutputConfig, expr::prelude::*};

pub(super) struct BasicOutputGenerator {
    writers: Vec<Box<dyn AnswersWriter>>,
}

impl BasicOutputGenerator {
    pub(super) fn new(configs: &[OutputConfig]) -> Self {
        let mut writers = vec![Box::new(LoggingAnswersWriter) as Box<dyn AnswersWriter>];

        writers.extend(configs.iter().map(|c| match c {
            OutputConfig::File(file_config) => match file_config.format {
                FileFormat::Json => todo!("Not implemented yet"),
                FileFormat::Binary => Box::new(BinaryFileAnswersWriter::new(file_config)),
            },
        } as Box<dyn AnswersWriter>));

        Self { writers }
    }

    pub(super) fn generate(&mut self, answers: &HashMap<u32, ValueRef>) {
        for writer in self.writers.iter_mut() {
            writer.write(answers);
        }
    }
}

trait AnswersWriter {
    fn write(&mut self, answers: &HashMap<u32, ValueRef>);
}

struct LoggingAnswersWriter;

impl AnswersWriter for LoggingAnswersWriter {
    fn write(&mut self, answers: &HashMap<u32, ValueRef>) {
        outgen::log_json(answers.iter());
    }
}

struct BinaryFileAnswersWriter {
    dir_path: PathBuf,
    counter: usize,
    enabled: bool,
    prefix: String,
    extension: String,
}

impl BinaryFileAnswersWriter {
    fn new(config: &FileGenConfig) -> Self {
        debug_assert_matches!(config.format, FileFormat::Binary);

        let dir_path = config.ensure_dir().unwrap();

        log_info!(
            "Setting up binary output writing to directory: {}",
            dir_path.display()
        );
        let dir_path = std::fs::canonicalize(dir_path).unwrap();

        let file_ext = config.format.default_extension();
        Self::check_out_dir(&dir_path, config.prefix.as_ref(), &file_ext);

        Self {
            dir_path,
            counter: 0,
            enabled: true,
            prefix: config.prefix.clone().unwrap_or_default(),
            extension: file_ext.to_owned(),
        }
    }

    fn write(&mut self, values: &[u8]) {
        let path = self
            .dir_path
            .join(format!("{}{}", self.prefix, self.counter))
            .with_added_extension(&self.extension);
        log_debug!("Writing values to file: {}.", path.display());
        let mut file = std::fs::File::create(path).unwrap();
        file.write(&values).unwrap();
        self.counter += 1;
    }

    fn check_out_dir(dir_path: &PathBuf, file_prefix: Option<&String>, file_ext: &str) {
        if std::fs::read_dir(&dir_path)
            .unwrap()
            .filter_map(io::Result::ok)
            .filter(|e| e.metadata().is_ok_and(|t| t.is_file()))
            .any(|e| {
                e.file_name().to_string_lossy().ends_with(file_ext)
                    && file_prefix
                        .is_none_or(|prefix| e.file_name().to_string_lossy().starts_with(prefix))
            })
        {
            log_warn!(
                "Output directory has some previous answers, which may may be overwritten: {}",
                dir_path.display(),
            );
        }
    }
}

impl AnswersWriter for BinaryFileAnswersWriter {
    fn write(&mut self, answers: &HashMap<u32, ValueRef>) {
        if !self.enabled {
            return;
        }

        let mut keys = answers.keys().collect::<Vec<_>>();
        keys.sort();

        let is_complete = *keys[0] == 1 && *keys[keys.len() - 1] == keys.len() as u32;
        if !is_complete {
            log_warn!("Unexpected answers format. Not all symbolic values are present.");
            return;
        }

        let bytes = keys
            .iter()
            .map(move |k| (k, answers.get(k).unwrap()))
            .filter_map(|(id, v)| {
                v.as_ref()
                    .try_into()
                    .inspect_err(|_| log_warn!("Value is not a byte: {} -> {}", id, v))
                    .ok()
            })
            .collect::<Vec<_>>();

        if bytes.len() != keys.len() {
            log_warn!(
                "Not all values are bytes. Disabling binary file answers writing for this execution."
            );
            self.enabled = false;
            return;
        }

        self.write(&bytes)
    }
}

impl TryFrom<&Value> for u8 {
    type Error = ();

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        match value {
            Value::Concrete(ConcreteValue::Const(ConstValue::Int {
                bit_rep,
                ty:
                    IntType {
                        bit_size: 8,
                        is_signed: false,
                    },
            })) => Ok((bit_rep.0 & 0xFF) as u8),
            _ => Err(()),
        }
    }
}
