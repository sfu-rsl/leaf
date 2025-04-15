use core::{error::Error, fmt::Display};
use std::prelude::rust_2021::*;

use super::{log_debug, log_info, log_warn};

/// Outputs answers found for the symbolic values.
pub trait AnswersWriter {
    type Id;
    type Answer;
    type Output;
    type Error;

    fn write(
        &mut self,
        answers: impl ExactSizeIterator<Item = (Self::Id, Self::Answer)>,
    ) -> Result<Self::Output, Self::Error>;
}

/// Provides a switch for an existing [AnswersWriter].
pub struct SwitchableAnswersWriter<W: AnswersWriter> {
    enabled: bool,
    inner: W,
}

impl<W: AnswersWriter> SwitchableAnswersWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            enabled: true,
            inner,
        }
    }

    pub fn switch(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

impl<W: AnswersWriter> AnswersWriter for SwitchableAnswersWriter<W> {
    type Id = W::Id;
    type Answer = W::Answer;
    type Output = Result<W::Output, W::Error>;
    type Error = ();

    fn write(
        &mut self,
        answers: impl ExactSizeIterator<Item = (Self::Id, Self::Answer)>,
    ) -> Result<Self::Output, Self::Error> {
        if !self.enabled {
            return Err(());
        }

        Ok(self.inner.write(answers))
    }
}

mod binary {
    use core::ops::Range;
    use std::{
        format,
        io::{self, Write},
        path::PathBuf,
    };

    use super::*;

    ///
    #[derive(Debug)]
    pub enum BinaryFileAnswerError {
        /// The answer iterator did not provide all the values for the buffer.
        Incomplete,
        /// Answer at an index was not byte-typed.
        NonByte(usize),
        Io(std::io::Error),
    }

    impl Error for BinaryFileAnswerError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            match self {
                Self::Io(err) => err.source(),
                _ => None,
            }
        }
    }

    impl Display for BinaryFileAnswerError {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self {
                BinaryFileAnswerError::Incomplete => write!(f, "Incomplete set of answers"),
                BinaryFileAnswerError::NonByte(index) => {
                    write!(f, "Answer with index {} is not available as a byte", index)
                }
                BinaryFileAnswerError::Io(err) => write!(f, "{err}"),
            }
        }
    }

    /// Outputs answers found for byte-typed symbolic values over time.
    /// It takes a directory and outputs each answer in a separate file named
    /// with the format `{prefix}{counter}.{extension}`.
    /// # Remarks
    /// - Ids are the index of the byte in the output buffer.
    /// - A `None` answer means it is not byte-typed.
    pub struct BinaryFileMultiAnswersWriter {
        dir_path: PathBuf,
        counter: usize,
        prefix: String,
        extension: String,
        buffer: Vec<u8>,
        /// The output buffer will be initially filled with this buffer.
        /// Useful when the output will be used as input again.
        default_answers: Box<[u8]>,
        _phantom: core::marker::PhantomData<()>,
    }

    impl BinaryFileMultiAnswersWriter {
        pub fn new(
            dir_path: PathBuf,
            prefix: Option<String>,
            extension: String,
            default_answers: Option<&[u8]>,
        ) -> Self {
            std::fs::create_dir_all(&dir_path).unwrap();

            log_info!(
                "Setting up binary output writing to directory: {}",
                dir_path.display()
            );
            let dir_path = std::fs::canonicalize(dir_path).unwrap();

            Self::check_out_dir(&dir_path, prefix.as_ref(), &extension);

            Self {
                dir_path,
                counter: 0,
                prefix: prefix.unwrap_or_default(),
                extension,
                buffer: default_answers.map(Vec::from).unwrap_or_default(),
                default_answers: default_answers.map(Into::into).unwrap_or_default(),
                _phantom: Default::default(),
            }
        }

        fn write(&mut self, range: Range<usize>) -> Result<PathBuf, io::Error> {
            let path = self
                .dir_path
                .join(format!("{}{}", self.prefix, self.counter))
                .with_added_extension(&self.extension);
            log_debug!("Writing values to file: {}.", path.display());

            std::fs::File::create(&path)
                .and_then(|mut f| f.write(&self.buffer[range]))
                .inspect(|_| {
                    self.counter += 1;
                })
                .map(|_| path)
        }

        fn check_out_dir(dir_path: &PathBuf, file_prefix: Option<&String>, file_ext: &str) {
            if std::fs::read_dir(&dir_path)
                .unwrap()
                .filter_map(io::Result::ok)
                .filter(|e| e.metadata().is_ok_and(|t| t.is_file()))
                .any(|e| {
                    e.file_name().to_string_lossy().ends_with(file_ext)
                        && file_prefix.is_none_or(|prefix| {
                            e.file_name().to_string_lossy().starts_with(prefix)
                        })
                })
            {
                log_warn!(
                    "Output directory has some previous answers, which may may be overwritten: {}",
                    dir_path.display(),
                );
            }
        }
    }

    impl AnswersWriter for BinaryFileMultiAnswersWriter {
        type Id = usize;
        type Answer = Option<u8>;
        type Output = PathBuf;
        type Error = BinaryFileAnswerError;

        fn write(
            &mut self,
            answers: impl ExactSizeIterator<Item = (Self::Id, Self::Answer)>,
        ) -> Result<PathBuf, BinaryFileAnswerError> {
            if answers.len() > self.buffer.len() {
                self.buffer
                    .extend(core::iter::repeat_n(0, answers.len() - self.buffer.len()));
            }

            let valid_range = 0..answers.len();

            // The buffer is growing and at least as wide as the default answers.
            self.buffer[0..self.default_answers.len()].copy_from_slice(&self.default_answers);

            for (id, ans) in answers {
                let index = id.into();
                let byte_ans = ans.ok_or_else(|| BinaryFileAnswerError::NonByte(index))?;
                if !valid_range.contains(&index) {
                    return Err(BinaryFileAnswerError::Incomplete);
                }

                self.buffer[index] = byte_ans;
            }

            self.write(valid_range).map_err(BinaryFileAnswerError::Io)
        }
    }
}
pub use binary::{BinaryFileAnswerError, BinaryFileMultiAnswersWriter};
