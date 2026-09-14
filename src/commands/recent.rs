use crate::{
    app::{AppError, CommandOutput},
    cli::{GlobalOptions, ReadArgs},
    query,
};

pub fn run(args: &ReadArgs, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let (reader, _) =
        query::open_reader(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    let options = query::read_options(args.page.limit, args.page.offset, args.include_hidden);
    let page = reader
        .recent(options)
        .map_err(|error| AppError::new("DB_READ", error.to_string(), 3))?;
    let data = query::page_data(&page).map_err(|error| AppError::new("OUTPUT", error, 1))?;
    let human = query::human_text(&query::format_entries(&page), global)
        .map_err(|error| AppError::new("INVALID_CONFIG", error, 2))?;
    Ok(CommandOutput::new(data, human))
}
