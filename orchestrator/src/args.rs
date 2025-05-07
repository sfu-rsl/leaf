use std::{default, path::PathBuf};

use clap::{Args, ValueEnum};

fn parse_env_pair(s: &str) -> Result<(String, String), String> {
    let mut split = s.splitn(2, '=');
    split
        .next()
        .zip(split.next())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .ok_or_else(|| "Expected format: KEY=VALUE".to_owned())
}

#[derive(ValueEnum, Clone, Copy, Debug, Default)]
pub enum OutputFormat {
    #[default]
    Csv,
    #[value(alias("jsonl"))]
    JsonLines,
}

#[derive(Args, Debug)]
pub struct CommonArgs {
    /// Leaf-instrumented program to run
    #[arg(short, long)]
    pub program: PathBuf,
    /// Path to the file to be given to the program on stdin
    #[arg(long)]
    pub stdin: Option<PathBuf>,
    /// Environment variables to pass to the program
    #[arg(long, value_parser=parse_env_pair, number_of_values=1)]
    pub env: Vec<(String, String)>,
    /// Argument to pass to the program
    #[arg(last = true)]
    pub args: Vec<String>,
    /// Path to the directory to write new (diverging) inputs to
    #[arg(short, long)]
    pub outdir: PathBuf,
    /// The format for the standard output of the orchestrator
    #[arg(long)]
    pub output_format: Option<OutputFormat>,
    /// Whether to return failure exit code in case the program does not finish successfully
    #[arg(short, long, action)]
    pub silent: bool,
}
