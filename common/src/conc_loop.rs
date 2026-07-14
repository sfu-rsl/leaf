use std::{path::PathBuf, prelude::rust_2024::*};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GeneratedInputRecord {
    pub path: PathBuf,
    pub score: Option<f64>,
}
