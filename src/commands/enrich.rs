//! Explicit missing-context enrichment for an existing entry.

use std::sync::Arc;

use crate::{
    app::{AppError, CommandOutput},
    cli::GlobalOptions,
    context_cache::PersistentContextCache,
    query,
    recovery::StateStore,
    ui::weather,
};
use capsule_core::{
    context::{
        Cancellation, ContextDependencies, ContextRequest, ContextService, NoopContextCache,
        ReqwestHttpClient, SystemClock,
    },
    contracts::ContextResult,
    db::{self, PathSource},
    JournalReader,
};
use serde_json::json;

pub fn run(identifier: &str, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let resolved = query::resolve(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    let reader = JournalReader::open(resolved.database_path.clone())
        .map_err(|error| AppError::new("DB_READ", error.to_string(), 3))?;
    let entry = reader
        .get(identifier, true)
        .map_err(|error| AppError::new("ENTRY_NOT_FOUND", error.to_string(), 3))?;
    let config_path = match resolved.config_source {
        PathSource::Explicit | PathSource::Environment => resolved.config_path.as_deref(),
        _ => None,
    };
    let settings = reader
        .context_settings(config_path)
        .map_err(|error| AppError::new("CONTEXT_READ", error.to_string(), 3))?;
    let identity = resolved
        .database_identity
        .clone()
        .ok_or_else(|| AppError::new("DB_REPLACED", "Database identity is unavailable", 3))?;
    let mut policy = settings.policy.clone();
    if global.no_context {
        policy.auto_capture = false;
        policy.allow_network = false;
        policy.allow_cache = false;
    } else if global.offline {
        policy.allow_network = false;
    }
    let backup_policy = capsule_core::contracts::BackupPolicy::new(
        resolved.backup_directory.clone(),
        resolved
            .settings
            .backup_retention_count
            .unwrap_or(db::DEFAULT_BACKUP_RETENTION_COUNT),
    );
    let mut request = ContextRequest::enrich(&resolved.database_path, &entry.uuid, policy);
    request.database_identity = Some(identity.clone());
    request.config_valid = resolved.config_valid && settings.valid;
    request.backup_policy = Some(backup_policy);
    let cache: Arc<dyn capsule_core::context::ContextCache> = if global.no_context {
        Arc::new(NoopContextCache)
    } else {
        Arc::new(PersistentContextCache::at_path(
            StateStore::from_env()
                .map_err(|error| AppError::new("RECOVERY_STORAGE", error, 5))?
                .cache_dir()
                .join("weather.json"),
            &resolved.database_path,
            identity,
        ))
    };
    let service = ContextService::new(ContextDependencies {
        http: Arc::new(ReqwestHttpClient::default()),
        clock: Arc::new(SystemClock::default()),
        cache,
        cancellation: Arc::new(SignalCancellation),
    });
    let result = service
        .enrich(&request)
        .map_err(|error| AppError::new("CONTEXT", error.to_string(), 1))?;
    let data = context_data(&entry.uuid, &result);
    let human = format!(
        "{}\n{}",
        entry.uuid,
        weather::render_stamp(Some(&result), true)
    );
    let mut output = CommandOutput::new(data, human);
    output.quiet = Some(entry.uuid.clone());
    output.warnings = result
        .warnings
        .iter()
        .map(|warning| cap_effects::sanitize_text(warning))
        .collect();
    output.committed = true;
    Ok(output)
}

fn context_data(uuid: &str, result: &ContextResult) -> serde_json::Value {
    let result = sanitized_result(result);
    json!({
        "entryUuid": cap_effects::sanitize_text(uuid),
        "saveState": "committed",
        "locationStatus": result.location_status,
        "weatherStatus": result.weather_status,
        "location": result.location,
        "weather": result.weather,
        "persistedAt": result.persisted_at.map(|value| value.to_rfc3339()),
        "warnings": result.warnings,
    })
}

pub(crate) fn sanitized_result(result: &ContextResult) -> ContextResult {
    let mut result = result.clone();
    if let Some(location) = result.location.as_mut() {
        location.place_name = location
            .place_name
            .take()
            .map(|value| cap_effects::sanitize_text(&value));
        location.place_details = location
            .place_details
            .take()
            .map(|value| cap_effects::sanitize_text(&value));
        location.source = location
            .source
            .take()
            .map(|value| cap_effects::sanitize_text(&value));
    }
    if let Some(weather) = result.weather.as_mut() {
        weather.provider = weather
            .provider
            .take()
            .map(|value| cap_effects::sanitize_text(&value));
        weather.condition = weather
            .condition
            .take()
            .map(|value| cap_effects::sanitize_text(&value));
        weather.icon = weather
            .icon
            .take()
            .map(|value| cap_effects::sanitize_text(&value));
    }
    result.warnings = result
        .warnings
        .into_iter()
        .map(|warning| cap_effects::sanitize_text(&warning))
        .collect();
    result
}

struct SignalCancellation;
impl Cancellation for SignalCancellation {
    fn is_cancelled(&self) -> bool {
        crate::cancellation::requested()
    }
}
