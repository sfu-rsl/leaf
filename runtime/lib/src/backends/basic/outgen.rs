use std::{assert_matches::debug_assert_matches, collections::HashMap};

use common::{
    answers::{
        AnswersWriter, BinaryFileAnswerError, BinaryFileMultiAnswersWriter, SwitchableAnswersWriter,
    },
    log_warn,
};

use crate::{
    abs::IntType,
    utils::file::{FileFormat, FileGenConfig},
};

use super::{config::OutputConfig, expr::prelude::*};

pub(super) struct BasicOutputGenerator {
    writers: Vec<Box<dyn BasicAnswersWriter>>,
}

impl BasicOutputGenerator {
    pub(super) fn new(configs: &[OutputConfig]) -> Self {
        let mut writers = vec![Box::new(LoggingAnswersWriter) as Box<dyn BasicAnswersWriter>];

        writers.extend(configs.iter().map(|c| match c {
            OutputConfig::File(file_config) => match file_config.format {
                FileFormat::Json | FileFormat::JsonLines => todo!("Not implemented yet"),
                FileFormat::Binary => Box::new(BinaryFileAnswersWriter::new(file_config)),
            },
        } as Box<dyn BasicAnswersWriter>));

        Self { writers }
    }

    pub(super) fn generate(&mut self, answers: &HashMap<u32, ValueRef>) {
        for writer in self.writers.iter_mut() {
            writer.write(answers);
        }
    }
}

trait BasicAnswersWriter {
    fn write(&mut self, answers: &HashMap<u32, ValueRef>);
}

struct LoggingAnswersWriter;

impl BasicAnswersWriter for LoggingAnswersWriter {
    fn write(&mut self, answers: &HashMap<u32, ValueRef>) {
        crate::outgen::log_json(answers.iter());
    }
}

/// A wrapper to convert [Value]s obtained from the solver to bytes.
struct BinaryFileAnswersWriter {
    inner: SwitchableAnswersWriter<BinaryFileMultiAnswersWriter>,
}

impl BinaryFileAnswersWriter {
    fn new(config: &FileGenConfig) -> Self {
        debug_assert_matches!(config.format, FileFormat::Binary);

        let dir_path = config.ensure_dir().unwrap();

        Self {
            inner: SwitchableAnswersWriter::new(BinaryFileMultiAnswersWriter::new(
                dir_path,
                config.prefix.clone(),
                config.format.default_extension().to_owned(),
                Default::default(),
            )),
        }
    }
}

impl BasicAnswersWriter for BinaryFileAnswersWriter {
    fn write(&mut self, answers: &HashMap<u32, ValueRef>) {
        let Ok(result) = self.inner.write(answers.iter().map(|(id, v)| {
            (
                (id - 1) as usize,
                TryInto::<u8>::try_into(AsRef::<Value>::as_ref(&v)).ok(),
            )
        })) else {
            return;
        };

        if let Err(err) = result {
            match err {
                BinaryFileAnswerError::Incomplete => {
                    log_warn!("Unexpected answers format. Not all symbolic values are present.");
                }
                BinaryFileAnswerError::NonByte(index) => {
                    let id = index as u32 + 1;
                    log_warn!("Value is not a byte: {} -> {}", id, answers[&id]);
                    log_warn!(
                        "Not all values are bytes. Disabling binary file answers writing for this execution."
                    );
                    self.inner.switch(false);
                }
                BinaryFileAnswerError::Io(error) => {
                    panic!("Could not write output: {error}")
                }
            }
        }
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
