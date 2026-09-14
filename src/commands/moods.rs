use crate::{
    app::{AppError, CommandOutput},
    cli::{GlobalOptions, Pagination},
    query,
};

pub fn run(args: &Pagination, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let (reader, _) =
        query::open_reader(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    let page = reader
        .moods(query::read_options(args.limit, args.offset, false))
        .map_err(|error| AppError::new("DB_READ", error.to_string(), 3))?;
    let data = query::metadata_data(&page).map_err(|error| AppError::new("OUTPUT", error, 1))?;
    let human = query::human_text(&query::format_moods(&page), global)
        .map_err(|error| AppError::new("INVALID_CONFIG", error, 2))?;
    let mut output = CommandOutput::new(data, human);
    output.warnings = page.warnings.clone();
    Ok(output)
}
