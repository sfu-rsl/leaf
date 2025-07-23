use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct FileGenConfig {
    /// The folder to write file outputs to.
    /// Defaults to the current working directory.
    #[serde(default)]
    pub directory: Option<std::path::PathBuf>,
    /// The format to write the file outputs in.
    #[serde(default)]
    pub format: FileFormat,
    /// The prefix to use for the name of the output files.
    /// Translates to the file name if a single file gets generated.
    #[serde(default)]
    pub prefix: Option<String>,
    /// The extension to use for the name of the output files.
    #[serde(default)]
    pub extension: Option<String>,
}

impl FileGenConfig {
    pub(crate) fn dir_or_default(&self) -> PathBuf {
        self.directory.clone().unwrap_or_else(|| {
            std::env::current_dir().expect("Cannot get current working directory")
        })
    }

    pub(crate) fn ensure_dir(&self) -> io::Result<PathBuf> {
        let dir = self.dir_or_default();
        fs::create_dir_all(&dir).map(|_| dir)
    }

    /// # Remarks
    /// Prefix is dual-purpose:
    /// - If a single file is generated, it is used as the file name.
    /// - If multiple files are generated, it is used as a prefix for each file.
    /// So if you want to generate a single file, and you want to give it a default name,
    /// you use `default_prefix` and `name` as `None`.
    pub(crate) fn single_file_path(&self, default_prefix: &str, name: Option<String>) -> PathBuf {
        let filename = format!(
            "{}{}",
            self.prefix
                .as_ref()
                .map(|p| p.as_str())
                .unwrap_or(default_prefix),
            name.unwrap_or_default()
        );
        self.dir_or_default()
            .join(filename)
            .with_added_extension(self.extension_or_default())
    }

    #[tracing::instrument(level = "debug")]
    pub(crate) fn open_or_create_single(
        &self,
        default_prefix: &str,
        name: Option<String>,
        truncate: bool,
    ) -> std::io::Result<fs::File> {
        self.open_or_create_single_with_path(default_prefix, name, truncate)
            .map(|(_, f)| f)
    }

    #[tracing::instrument(level = "debug")]
    pub(crate) fn open_or_create_single_with_path(
        &self,
        default_prefix: &str,
        name: Option<String>,
        truncate: bool,
    ) -> std::io::Result<(PathBuf, fs::File)> {
        self.ensure_dir().and_then(|_| {
            let path = self.single_file_path(default_prefix, name);
            let file = fs::File::options()
                .write(true)
                .create(true)
                .truncate(truncate)
                .open(&path);
            file.map(|f| (path, f))
        })
    }

    pub(crate) fn extension_or_default(&self) -> &str {
        self.extension
            .as_ref()
            .map(AsRef::<str>::as_ref)
            .unwrap_or_else(|| self.format.default_extension())
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FileFormat {
    #[serde(alias = "txt")]
    Text,
    #[default]
    Json,
    #[serde(alias = "jsonl")]
    JsonLines,
    Binary,
}

impl FileFormat {
    pub(crate) fn default_extension(&self) -> &'static str {
        match self {
            Self::Text => "txt",
            Self::Json => "json",
            Self::JsonLines => "jsonl",
            Self::Binary => "bin",
        }
    }

    pub(crate) fn is_streamable(&self) -> bool {
        match self {
            Self::Text => true,
            Self::Json => false,
            Self::JsonLines => true,
            Self::Binary => false,
        }
    }
}

#[derive(Default)]
pub(crate) struct JsonLinesFormatter {
    depth: usize,
}

use serde_json::ser::{CompactFormatter, Formatter as JsonFormatter};

impl JsonFormatter for JsonLinesFormatter {
    fn begin_object<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        self.depth += 1;
        CompactFormatter.begin_object(writer)
    }

    fn end_object<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        self.depth -= 1;
        CompactFormatter.end_object(writer).and_then(|_| {
            if self.depth == 0 {
                writer.write(&[b'\n']).map(|_| ())
            } else {
                Ok(())
            }
        })
    }
}
