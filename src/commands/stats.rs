use capsule_core::stats::{self, NaiveDate, StatsPeriod};
use serde_json::Value;

use crate::{
    app::{AppError, CommandOutput},
    cli::{Command, GlobalOptions, Period},
    query,
};

pub fn run(args: &Command, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let Command::Stats {
        period,
        include_hidden,
    } = args
    else {
        return Err(AppError::new("INVALID_INPUT", "invalid stats arguments", 2));
    };
    run_at(*period, stats::local_today(), *include_hidden, global)
}

pub fn run_at(
    period: Period,
    today: NaiveDate,
    include_hidden: bool,
    global: &GlobalOptions,
) -> Result<CommandOutput, AppError> {
    let (reader, _) =
        query::open_reader(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    let period = match period {
        Period::Week => StatsPeriod::Week,
        Period::Month => StatsPeriod::Month,
        Period::Year => StatsPeriod::Year,
    };
    let stats =
        stats::memory_stats_for_database(reader.database_path(), period, today, include_hidden)
            .map_err(|error| AppError::new("DB_READ", error.to_string(), 3))?;
    let data = serde_json::to_value(&stats)
        .map_err(|error| AppError::new("OUTPUT", error.to_string(), 1))?;
    let human = query::human_text(&format_stats(&stats), global)
        .map_err(|error| AppError::new("INVALID_CONFIG", error, 2))?;
    Ok(CommandOutput::new(data, human))
}

fn format_stats(stats: &stats::MemoryStats) -> String {
    let mut output = format!(
        "Writing stats · {}\n{} entries · {} words · {} active days\nCurrent streak: {} days · longest: {} days",
        stats.period.name(),
        stats.total_entries,
        stats.total_words,
        stats.active_days,
        stats.current_streak_days,
        stats.longest_streak_days
    );
    if stats.total_entries == 0 {
        output.push_str("\nNo writing in this period.");
    }
    output
}

#[allow(dead_code)]
fn _json_shape(stats: &stats::MemoryStats) -> Value {
    serde_json::to_value(stats).unwrap_or(Value::Null)
}
