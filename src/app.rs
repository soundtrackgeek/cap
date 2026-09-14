use crate::{
    cli::{Cli, Command},
    contracts::CliError,
};
use serde_json::Value;

#[derive(Debug)]
pub struct CommandOutput {
    pub data: Value,
    pub human: String,
    pub quiet: Option<String>,
    pub warnings: Vec<String>,
    /// A completed save is not made retryable by a broken output stream.
    pub committed: bool,
}
impl CommandOutput {
    pub fn new(data: Value, human: impl Into<String>) -> Self {
        Self {
            data,
            human: human.into(),
            quiet: None,
            warnings: vec![],
            committed: false,
        }
    }
}

#[derive(Debug)]
pub struct AppError {
    pub detail: CliError,
    pub exit_code: i32,
    pub data: Option<Value>,
}
impl AppError {
    pub fn new(code: &str, message: impl Into<String>, exit_code: i32) -> Self {
        Self {
            detail: CliError::new(code, message, matches!(exit_code, 4 | 6)),
            exit_code,
            data: None,
        }
    }
}

pub fn execute(cli: &Cli) -> Result<CommandOutput, AppError> {
    if cli.global.dry_run
        && !matches!(
            cli.command,
            None | Some(Command::Add(_)) | Some(Command::Entry(_))
        )
    {
        return Err(AppError::new(
            "INVALID_INPUT",
            "--dry-run is supported only for entry creation.",
            2,
        ));
    }
    Err(AppError::new(
        "NOT_IMPLEMENTED",
        "This command is not connected yet in the foundation development build.",
        3,
    ))
}
