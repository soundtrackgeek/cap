//! Explicit inspection and replay/removal of cap-local pending captures.

use crate::{
    app::{AppError, CommandOutput},
    cli::{GlobalOptions, RecoverAction},
    commands::{add, status},
    recovery::{RecoveryState, StateError, StateStore},
};
use capsule_core::identity;
use serde_json::json;

pub fn run(action: &RecoverAction, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    match action {
        RecoverAction::List => list(global),
        RecoverAction::Show { id } => show(id, global),
        RecoverAction::Retry { id } => add::retry_capture_id(id, global),
        RecoverAction::Discard { id, yes } => discard(id, *yes),
    }
}

fn list(_global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let state =
        StateStore::from_env().map_err(|error| AppError::new("RECOVERY_STORAGE", error, 5))?;
    let records = state
        .list_records()
        .map_err(|error| storage_error(error, 5))?;
    let items = records.iter().map(status::record_data).collect::<Vec<_>>();
    let human = if items.is_empty() {
        "No pending captures.".to_string()
    } else {
        records
            .iter()
            .map(|record| format!("{}  {}", record.capture_id, record.display_state()))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(
        json!({"items": items, "total": records.len()}),
        human,
    ))
}

fn show(id: &str, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let id = identity::validate_capture_id(id)
        .map_err(|error| AppError::new("INVALID_INPUT", error.to_string(), 2))?;
    let state =
        StateStore::from_env().map_err(|error| AppError::new("RECOVERY_STORAGE", error, 5))?;
    let record = if let Some(record) = state
        .read_pending(&id)
        .map_err(|error| storage_error(error, 5))?
    {
        Some(record)
    } else {
        state
            .read_receipt(&id)
            .map_err(|error| storage_error(error, 5))?
    };
    let Some(record) = record else {
        if state
            .read_tombstone(&id)
            .map_err(|error| storage_error(error, 5))?
            .is_some()
        {
            return Err(AppError::new(
                "CAPTURE_UNVERIFIABLE",
                format!(
                    "Capture {id} was discarded before its commit could be verified; choose a new capture ID."
                ),
                6,
            ));
        }
        if state
            .read_binding(&id)
            .map_err(|error| storage_error(error, 5))?
            .is_some()
        {
            return Err(AppError::new(
                "CAPTURE_ID_EXPIRED",
                format!("Capture {id} has expired; its durable binding prevents replay."),
                2,
            ));
        }
        return Err(AppError::new(
            "CAPTURE_NOT_FOUND",
            format!("No capture named {id}"),
            3,
        ));
    };
    let mut output = status::output_for_record(&record, global)?;
    if let Some(request) = record.request.as_ref() {
        // `recover show` is the one explicit inspection path that may reveal
        // a pending draft body.  Status/list and committed receipts continue
        // to omit it so routine diagnostics cannot leak authored content.
        output.data["draft"] = json!({
            "text": request.text,
            "contentFormat": request.content_format,
            "title": request.title,
            "summary": request.summary,
            "mood": request.mood,
            "tags": request.tags,
            "starred": request.starred,
            "pinned": request.pinned,
            "continueFromUuid": request.continue_from_uuid,
        });
        output.human = format!(
            "{}\n\n{}",
            output.human,
            cap_effects::sanitize_text(&request.text)
        );
    }
    Ok(output)
}

fn discard(id: &str, yes: bool) -> Result<CommandOutput, AppError> {
    let id = identity::validate_capture_id(id)
        .map_err(|error| AppError::new("INVALID_INPUT", error.to_string(), 2))?;
    if !yes {
        return Err(AppError::new(
            "CONFIRMATION_REQUIRED",
            "Discarding a pending capture only removes local recovery data; repeat with --yes.",
            2,
        ));
    }
    let state =
        StateStore::from_env().map_err(|error| AppError::new("RECOVERY_STORAGE", error, 5))?;
    let lock = state.lock(&id).map_err(|error| match error {
        StateError::Busy(message) => AppError::new("CAPTURE_BUSY", message, 4),
        other => storage_error(other, 5),
    })?;
    let record = state
        .read_pending(&id)
        .map_err(|error| storage_error(error, 5))?
        .ok_or_else(|| {
            AppError::new(
                "CAPTURE_NOT_FOUND",
                format!("No pending capture named {id}"),
                3,
            )
        })?;
    if record.state == RecoveryState::Committed {
        drop(lock);
        return Err(AppError::new(
            "CAPTURE_COMMITTED",
            "The journal entry is already saved; discard never deletes a Capsule entry.",
            2,
        ));
    }
    if record.explicit_capture_id {
        state
            .write_tombstone(&record)
            .map_err(|error| storage_error(error, 5))?;
    }
    state
        .remove_pending(&id)
        .map_err(|error| storage_error(error, 5))?;
    drop(lock);
    Ok(CommandOutput::new(
        json!({"captureId": id, "discarded": true, "tombstoned": record.explicit_capture_id}),
        "Pending capture discarded (journal unchanged).",
    ))
}

fn storage_error(error: StateError, exit: i32) -> AppError {
    AppError::new("RECOVERY_STORAGE", error.to_string(), exit)
}
