use capsule_core::stats::{self, MemoryQuery};
use serde_json::json;

use crate::{
    app::{AppError, CommandOutput},
    cli::{Command, GlobalOptions},
    insights::{self, MemoryStateStore},
    query,
    ui::unseal,
};

pub fn run(args: &Command, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let Command::Recall {
        tag,
        include_hidden,
    } = args
    else {
        return Err(AppError::new(
            "INVALID_INPUT",
            "invalid recall arguments",
            2,
        ));
    };
    run_at(
        tag.as_deref(),
        *include_hidden,
        stats::local_today(),
        global,
    )
}

pub fn run_at(
    tag: Option<&str>,
    include_hidden: bool,
    _today: capsule_core::stats::NaiveDate,
    global: &GlobalOptions,
) -> Result<CommandOutput, AppError> {
    run_at_with_seed(tag, include_hidden, _today, insights::system_seed(), global)
}

pub fn run_at_with_seed(
    tag: Option<&str>,
    include_hidden: bool,
    _today: capsule_core::stats::NaiveDate,
    seed: u64,
    global: &GlobalOptions,
) -> Result<CommandOutput, AppError> {
    let (reader, resolved) =
        query::open_reader(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    let seed_for_candidates = seed;
    let candidates = stats::recall_candidates_for_database(
        reader.database_path(),
        MemoryQuery {
            include_hidden,
            tag: tag.map(str::to_owned),
            ..MemoryQuery::default()
        },
        seed_for_candidates,
        64,
    )
    .map_err(|error| AppError::new("DB_READ", error.to_string(), 3))?;
    let identity = resolved
        .database_identity
        .clone()
        .unwrap_or_else(|| capsule_core::db::FileIdentity::for_path(reader.database_path()));
    let mut warnings = Vec::new();
    let selected = match MemoryStateStore::from_env() {
        Ok(store) => match store.choose_recall(&identity, &candidates, seed) {
            Ok(selected) => selected,
            Err(error) => {
                warnings.push(format!("Recall history unavailable: {error}"));
                stats::choose_recall(&candidates, None, seed).cloned()
            }
        },
        Err(error) => {
            warnings.push(format!("Recall history unavailable: {error}"));
            stats::choose_recall(&candidates, None, seed).cloned()
        }
    };
    let data = json!({
        "entry": selected,
        "candidateCount": candidates.len(),
        "empty": selected.is_none(),
    });
    let human = query::human_text(&unseal::format_recall(selected.as_ref()), global)
        .map_err(|error| AppError::new("INVALID_CONFIG", error, 2))?;
    let mut output = CommandOutput::new(data, human);
    output.warnings = warnings;
    Ok(output)
}
