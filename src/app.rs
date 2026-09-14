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
    match &cli.command {
        None => crate::commands::add::run_default(None, &cli.global),
        Some(Command::Entry(words)) => crate::commands::add::run_default(Some(words), &cli.global),
        Some(Command::Add(args)) => crate::commands::add::run(args, &cli.global),
        Some(Command::Theme { action }) => crate::commands::theme::run(action, &cli.global),
        Some(Command::Config { action }) => crate::commands::config::run(action, &cli.global),
        Some(Command::Completions { shell }) => {
            crate::commands::completions::run(shell, &cli.global)
        }
        Some(Command::Fx { name }) => run_fx(name.as_deref(), &cli.global),
        Some(Command::Show(args)) => crate::commands::show::run(args, &cli.global),
        Some(Command::Today(args)) => crate::commands::today::run(args, &cli.global),
        Some(Command::Recent(args)) => crate::commands::recent::run(args, &cli.global),
        Some(Command::Search(args)) => crate::commands::search::run(args, &cli.global),
        Some(Command::Tags(args)) => crate::commands::tags::run(args, &cli.global),
        Some(Command::Moods(args)) => crate::commands::moods::run(args, &cli.global),
        Some(Command::Context) => crate::commands::context::run(&cli.global),
        Some(Command::Doctor) => crate::commands::doctor::run(&cli.global),
        Some(command @ Command::Recall { .. }) => {
            crate::commands::recall::run(command, &cli.global)
        }
        Some(Command::OnThisDay(args)) => crate::commands::on_this_day::run(args, &cli.global),
        Some(command @ Command::Calendar { .. }) => {
            crate::commands::calendar::run(command, &cli.global)
        }
        Some(command @ Command::Stats { .. }) => crate::commands::stats::run(command, &cli.global),
        Some(command @ Command::Garden { .. }) => {
            crate::commands::garden::run(command, &cli.global)
        }
        Some(Command::Status { capture_id }) => {
            crate::commands::status::run(capture_id, &cli.global)
        }
        Some(Command::Recover { action }) => crate::commands::recover::run(action, &cli.global),
        Some(Command::Enrich { identifier }) => {
            crate::commands::enrich::run(identifier, &cli.global)
        }
        _ => Err(AppError::new(
            "NOT_IMPLEMENTED",
            "This command is not connected yet in the foundation development build.",
            3,
        )),
    }
}

fn run_fx(
    name: Option<&str>,
    global: &crate::cli::GlobalOptions,
) -> Result<CommandOutput, AppError> {
    use cap_effects::{MotionMode, ResolvedOutputMode, TerminalCapabilities};
    let capabilities = TerminalCapabilities::detect();
    let presentation = crate::preferences::resolve_defaults(global, &capabilities)
        .map_err(|error| AppError::new("INVALID_CONFIG", error.to_string(), 2))?;
    if !matches!(
        presentation.output,
        ResolvedOutputMode::Human {
            motion: MotionMode::Full,
            ..
        }
    ) {
        return crate::commands::fx::run_with_capabilities(name, global, &capabilities);
    }
    if let Err(error) = crate::cancellation::install() {
        let mut static_options = global.clone();
        static_options.motion = Some(crate::cli::MotionMode::Off);
        let mut result =
            crate::commands::fx::run_with_capabilities(name, &static_options, &capabilities)?;
        result.warnings.push(format!(
            "Animation disabled because interruption handling is unavailable: {error}"
        ));
        return Ok(result);
    }
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "[synthetic] FX demo")
        .and_then(|()| stdout.flush())
        .map_err(|error| AppError::new("EFFECT_IO", error.to_string(), 1))?;
    crate::commands::fx::run_with_writer_and_cancel(
        name,
        global,
        &capabilities,
        &mut stdout,
        crate::cancellation::requested,
    )
}
