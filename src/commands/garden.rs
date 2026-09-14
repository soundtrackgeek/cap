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
    let presentation = crate::ui::memory::MemoryPresentation::resolve(global)?;
    let (reader, _) =
        query::open_reader(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    let garden = stats::memory_garden_for_database(reader.database_path(), today, include_hidden)
        .map_err(|error| AppError::new("DB_READ", error.to_string(), 3))?;
    let data = serde_json::to_value(&garden)
        .map_err(|error| AppError::new("OUTPUT", error.to_string(), 1))?;
    let human = presentation.present(
        |elapsed, width, config| {
            garden_ui::garden_frame_with_icons(
                &garden,
                width,
                config,
                elapsed,
                presentation.ascii(),
            )
        },
        std::time::Duration::from_millis(400),
    )?;
    Ok(CommandOutput::new(data, human))
}
