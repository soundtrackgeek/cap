use capsule_core::stats::{self, NaiveDate};

use crate::{
    app::{AppError, CommandOutput},
    cli::{Command, GlobalOptions},
    query,
    ui::garden as garden_ui,
};

pub fn run(args: &Command, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let Command::Garden { include_hidden } = args else {
        return Err(AppError::new(
            "INVALID_INPUT",
            "invalid garden arguments",
            2,
        ));
    };
    run_at(stats::local_today(), *include_hidden, global)
}

pub fn run_at(
    today: NaiveDate,
    include_hidden: bool,
    global: &GlobalOptions,
) -> Result<CommandOutput, AppError> {
    let (reader, _) =
        query::open_reader(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    let garden = stats::memory_garden_for_database(reader.database_path(), today, include_hidden)
        .map_err(|error| AppError::new("DB_READ", error.to_string(), 3))?;
    let data = serde_json::to_value(&garden)
        .map_err(|error| AppError::new("OUTPUT", error.to_string(), 1))?;
    let human = query::human_text(&garden_ui::format_garden(&garden), global)
        .map_err(|error| AppError::new("INVALID_CONFIG", error, 2))?;
    Ok(CommandOutput::new(data, human))
}
