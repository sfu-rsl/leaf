use std::error::Error;
use std::fs;
use std::path::PathBuf;

use clap::Parser;
use common::type_info::{TypesData, rw};

#[derive(Parser)]
#[command(name = "leaf-typedb-merge")]
struct Args {
    #[arg(short, long)]
    out_file: PathBuf,
    #[arg(short, long, required = true, num_args = 1..)]
    input: Vec<PathBuf>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let input_files = expand_inputs(&args.input)?;
    let databases = input_files
        .into_iter()
        .map(|path| {
            rw::read_types_db_from(&path).map_err(|error| {
                format!("Failed to read type database `{}`: {error}", path.display())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let TypesData {
        all_types,
        core_types,
        metadata,
    } = rw::merge_types_dbs(databases)?;

    rw::write_types_db_to(all_types.values(), core_types, metadata, &args.out_file)?;
    Ok(())
}

fn expand_inputs(inputs: &[PathBuf]) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    for input in inputs {
        if input.is_file() {
            files.push(input.clone());
            continue;
        }
        if !input.is_dir() {
            return Err(
                format!("Type database input `{}` is not readable", input.display()).into(),
            );
        }

        let mut directory_files = fs::read_dir(input)
            .map_err(|error| {
                format!(
                    "Failed to read input directory `{}`: {error}",
                    input.display()
                )
            })?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|path| {
                path.is_file()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(rw::is_stable_db_file_name)
            })
            .collect::<Vec<_>>();
        directory_files.sort();
        if directory_files.is_empty() {
            return Err(format!(
                "Input directory `{}` contains no stable type databases",
                input.display()
            )
            .into());
        }
        files.extend(directory_files);
    }
    Ok(files)
}
