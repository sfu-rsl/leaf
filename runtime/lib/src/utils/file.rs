use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct FileGenConfig {
    /// The folder to write file outputs to.
    /// Defaults to the current working directory.
    pub directory: Option<std::path::PathBuf>,
    /// The format to write the file outputs in.
    pub format: FileFormat,
    /// The prefix to use for the name of the output files.
    /// Translates to the file name if a single file gets generated.
    pub prefix: Option<String>,
    /// The extension to use for the name of the output files.
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

    pub(crate) fn single_file_path(&self, default_filename: &str) -> PathBuf {
        let filename = self
            .prefix
            .as_ref()
            .map(AsRef::<Path>::as_ref)
            .unwrap_or(default_filename.as_ref());
        self.dir_or_default()
            .join(filename)
            .with_added_extension(self.extension_or_default())
    }

    pub(crate) fn open_or_create_single(
        &self,
        default_filename: &str,
    ) -> std::io::Result<fs::File> {
        self.ensure_dir().and_then(|_| {
            fs::File::options()
                .write(true)
                .create(true)
                .open(self.single_file_path(default_filename))
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
    #[default]
    Json,
    Binary,
}

impl FileFormat {
    pub(crate) fn default_extension(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Binary => "bin",
        }
    }
}
