use crate::{
    app::{AppError, CommandOutput},
    cli::GlobalOptions,
    query,
};

pub fn run(global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let (data, warnings) =
        query::doctor_data(global).map_err(|error| AppError::new("DOCTOR", error, 3))?;
    let human = query::human_text(&query::format_doctor_report(&data), global)
        .map_err(|error| AppError::new("INVALID_CONFIG", error, 2))?;
    let mut output = CommandOutput::new(data, human);
    output.warnings = warnings;
    Ok(output)
}
