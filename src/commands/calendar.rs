use cap_effects::TerminalCapabilities;
use capsule_core::stats::{self, NaiveDate};

use crate::{
    app::{AppError, CommandOutput},
    cli::{Command, GlobalOptions},
    query,
    ui::calendar as calendar_ui,
};

pub fn run(args: &Command, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let Command::Calendar {
        month,
        include_hidden,
    } = args
    else {
        return Err(AppError::new(
            "INVALID_INPUT",
            "invalid calendar arguments",
            2,
        ));
    };
    let today = stats::local_today();
    let month = parse_month(month.as_deref(), today)?;
    run_at(month, *include_hidden, global)
}

pub fn run_at(
    month: NaiveDate,
    include_hidden: bool,
    global: &GlobalOptions,
) -> Result<CommandOutput, AppError> {
    let (reader, _) =
        query::open_reader(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    let calendar =
        stats::memory_calendar_for_database(reader.database_path(), month, include_hidden)
            .map_err(|error| AppError::new("DB_READ", error.to_string(), 3))?;
    let data = serde_json::to_value(&calendar)
        .map_err(|error| AppError::new("OUTPUT", error.to_string(), 1))?;
    let width = TerminalCapabilities::detect().compact_width();
    let human = query::human_text(&calendar_ui::format_calendar(&calendar, width), global)
        .map_err(|error| AppError::new("INVALID_CONFIG", error, 2))?;
    Ok(CommandOutput::new(data, human))
}

fn parse_month(value: Option<&str>, today: NaiveDate) -> Result<NaiveDate, AppError> {
    let value = value.unwrap_or("");
    if value.is_empty() {
        return NaiveDate::parse_from_str(&format!("{}-01", today.format("%Y-%m")), "%Y-%m-%d")
            .map_err(|_| AppError::new("INVALID_INPUT", "invalid current month", 2));
    }
    if value.len() != 7 {
        return Err(AppError::new(
            "INVALID_INPUT",
            "--month must use YYYY-MM format",
            2,
        ));
    }
    NaiveDate::parse_from_str(&format!("{value}-01"), "%Y-%m-%d")
        .map_err(|_| AppError::new("INVALID_INPUT", "--month must use a valid YYYY-MM date", 2))
}
