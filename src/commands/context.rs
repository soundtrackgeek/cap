use crate::{
    app::{AppError, CommandOutput},
    cli::GlobalOptions,
    query,
};

pub fn run(global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let (reader, resolved) =
        query::open_reader(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    // Pass an explicit path only when the resolver selected one explicitly or
    // through the environment.  A missing fallback config must retain the
    // shared loader's inherited-default semantics.
    let config_path = match resolved.config_source {
        capsule_core::db::PathSource::Explicit | capsule_core::db::PathSource::Environment => {
            resolved.config_path.as_deref()
        }
        _ => None,
    };
    let settings = reader
        .context_settings(config_path)
        .map_err(|error| AppError::new("CONTEXT_READ", error.to_string(), 3))?;
    let (data, warnings) = query::context_data(&settings, &resolved, global)
        .map_err(|error| AppError::new("OUTPUT", error, 1))?;
    let human = query::human_text(&query::format_context_report(&data), global)
        .map_err(|error| AppError::new("INVALID_CONFIG", error, 2))?;
    let mut output = CommandOutput::new(data, human);
    output.warnings = warnings;
    Ok(output)
}
