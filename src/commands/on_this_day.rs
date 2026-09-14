use capsule_core::stats::{self, NaiveDate};
use serde_json::json;

use crate::{
    app::{AppError, CommandOutput},
    cli::{GlobalOptions, ReadArgs},
    query,
    ui::unseal,
};

pub fn run(args: &ReadArgs, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    run_at(
        stats::local_today(),
        args.page.limit,
        args.page.offset,
        args.include_hidden,
        global,
    )
}

pub fn run_at(
    date: NaiveDate,
    limit: u32,
    offset: u64,
    include_hidden: bool,
    global: &GlobalOptions,
) -> Result<CommandOutput, AppError> {
    let (reader, _) =
        query::open_reader(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    let page = stats::on_this_day_page_for_database(
        reader.database_path(),
        date,
        include_hidden,
        limit,
        offset,
    )
    .map_err(|error| AppError::new("DB_READ", error.to_string(), 3))?;
    let items = page.entries.clone();
    let data = json!({
        "date": date.to_string(),
        "items": items,
        "total": page.total,
        "limit": page.limit,
        "offset": page.offset,
        "hasMore": page.has_more,
    });
    let human = query::human_text(
        &unseal::format_on_this_day(&date.to_string(), &items),
        global,
    )
    .map_err(|error| AppError::new("INVALID_CONFIG", error, 2))?;
    Ok(CommandOutput::new(data, human))
}
