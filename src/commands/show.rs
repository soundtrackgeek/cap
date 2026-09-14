use crate::{
    app::{AppError, CommandOutput},
    cli::{GlobalOptions, ShowArgs},
    query,
};

pub fn run(args: &ShowArgs, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let (reader, _) =
        query::open_reader(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    let entry = reader
        .get(&args.identifier, args.include_hidden)
        .map_err(|error| AppError::new("ENTRY_NOT_FOUND", error.to_string(), 3))?;
    let data = serde_json::to_value(&entry)
        .map_err(|error| AppError::new("OUTPUT", error.to_string(), 1))?;
    let human = query::human_text(&query::format_entry(&entry), global)
        .map_err(|error| AppError::new("INVALID_CONFIG", error, 2))?;
    Ok(CommandOutput::new(data, human))
}
