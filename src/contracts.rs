use serde::{Deserialize, Serialize};

pub const OUTPUT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CliError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl CliError {
    pub fn new(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OutputEnvelope<T> {
    pub schema_version: u32,
    pub ok: bool,
    pub command: String,
    pub data: Option<T>,
    pub warnings: Vec<String>,
    pub error: Option<CliError>,
}

impl<T> OutputEnvelope<T> {
    pub fn success(command: impl Into<String>, data: T) -> Self {
        Self {
            schema_version: OUTPUT_SCHEMA_VERSION,
            ok: true,
            command: command.into(),
            data: Some(data),
            warnings: vec![],
            error: None,
        }
    }
    pub fn failure(command: impl Into<String>, error: CliError) -> Self {
        Self {
            schema_version: OUTPUT_SCHEMA_VERSION,
            ok: false,
            command: command.into(),
            data: None,
            warnings: vec![],
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SaveState {
    NotCommitted,
    Committed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub limit: u32,
    pub offset: u64,
    pub has_more: bool,
}
