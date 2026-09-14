use crate::{
    app::{AppError, CommandOutput},
    cli::{GlobalOptions, SearchArgs},
    query,
};

pub fn run(args: &SearchArgs, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let (reader, _) =
        query::open_reader(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    let options = query::read_options(
        args.read.page.limit,
        args.read.page.offset,
        args.read.include_hidden,
    );
    let response = reader
        .search(&args.query, options)
        .map_err(|error| AppError::new("DB_READ", error.to_string(), 3))?;
    let data = query::search_data(&response).map_err(|error| AppError::new("OUTPUT", error, 1))?;
    let human = query::human_text(&query::format_search(&response), global)
        .map_err(|error| AppError::new("INVALID_CONFIG", error, 2))?;
    let mut output = CommandOutput::new(data, human);
    output.warnings = response.warnings.clone();
    Ok(output)
}
