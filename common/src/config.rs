use std::{env, format, string::ToString};

use config::{
    Config, ConfigBuilder, ConfigError, Environment, File, FileFormat, FileStoredFormat,
    builder::DefaultState,
};

use crate::{log_debug, log_warn};

pub const CONFIG_STR: &str = "CONFIG_STR";
pub const CONFIG_STR_FORMAT: &str = "CONFIG_STR_FMT";

pub fn load_config(
    file_name: &str,
    env_prefix: &str,
    config_builder: impl FnOnce(
        ConfigBuilder<DefaultState>,
    ) -> Result<ConfigBuilder<DefaultState>, ConfigError>,
) -> Result<Config, ConfigError> {
    let mut builder =
        config_builder(Config::builder()).expect("Failed to obtain configuration builder");
    builder = builder.add_source(
        File::with_name(
            &crate::utils::search_current_ancestor_dirs_for(file_name)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| file_name.to_string()),
        )
        .required(false),
    );
    if let Some((str, format)) = Option::zip(
        env::var(format!("{env_prefix}_{CONFIG_STR}")).ok(),
        env::var(format!("{env_prefix}_{CONFIG_STR_FORMAT}")).ok(),
    ) {
        if let Ok(format) = try_parse_format(&format) {
            builder = builder.add_source(File::from_str(str.as_str(), format));
        } else {
            log_warn!("Unknown format for config string: {}", format);
        }
    }
    builder = builder.add_source(
        Environment::with_prefix(env_prefix)
            .prefix_separator("_")
            .separator("__"),
    );

    builder
        .build()
        .inspect(|c| log_debug!("Loaded raw configurations: {:?}", c))
}

fn try_parse_format(format: &str) -> Result<FileFormat, &str> {
    use FileFormat::*;
    let all_formats = [Toml, Json, Json5, Yaml, Ron, Ini];
    all_formats
        .into_iter()
        .find(|f| f.file_extensions().contains(&format))
        .ok_or(format)
}
