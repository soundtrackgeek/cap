//! Durable capture status and read-only reconciliation.

use crate::{
    app::{AppError, CommandOutput},
    cli::GlobalOptions,
    commands::enrich,
    recovery::{RecoveryRecord, RecoveryState, StateError, StateStore},
};
use capsule_core::{capture::CaptureErrorCode, contracts::CaptureOutcome, identity};
use serde_json::{json, Value};

pub fn run(capture_id: &str, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let capture_id = identity::validate_capture_id(capture_id)
        .map_err(|error| AppError::new("INVALID_INPUT", error.to_string(), 2))?;
    let state =
        StateStore::from_env().map_err(|error| AppError::new("RECOVERY_STORAGE", error, 5))?;
    if let Some(record) = state
        .read_receipt(&capture_id)
        .map_err(|error| storage_error(error, 5))?
    {
        return output_for_record(&record, global);
    }
    let Some(_initial_record) = state
        .read_pending(&capture_id)
        .map_err(|error| storage_error(error, 5))?
    else {
        if state
            .read_tombstone(&capture_id)
            .map_err(|error| storage_error(error, 5))?
            .is_some()
        {
            let mut error = AppError::new(
                "CAPTURE_UNVERIFIABLE",
                format!(
                    "Capture {capture_id} was discarded before its commit could be verified; choose a new capture ID."
                ),
                6,
            );
            error.data = Some(json!({"captureId": capture_id, "saveState": "unknown"}));
            return Err(error);
        }
        if state
            .read_binding(&capture_id)
            .map_err(|error| storage_error(error, 5))?
            .is_some()
        {
            return Err(AppError::new(
                "CAPTURE_ID_EXPIRED",
                format!(
                    "Capture {capture_id} has expired; its durable binding prevents silent replay."
                ),
                2,
            ));
        }
        return Err(AppError::new(
            "CAPTURE_NOT_FOUND",
            format!("No pending or committed capture named {capture_id}"),
            3,
        ));
    };

    let _record_lock = state.lock(&capture_id).map_err(|error| match error {
        StateError::Busy(message) => AppError::new("CAPTURE_BUSY", message, 4),
        other => storage_error(other, 5),
    })?;
    // The lock closes the read/reconcile/write window.  A concurrent retry
    // may have replaced this pending record with a receipt while we waited.
    if let Some(receipt_record) = state
        .read_receipt(&capture_id)
        .map_err(|error| storage_error(error, 5))?
    {
        return output_for_record(&receipt_record, global);
    }
    let mut record = if let Some(latest) = state
        .read_pending(&capture_id)
        .map_err(|error| storage_error(error, 5))?
    {
        latest
    } else {
        if state
            .read_tombstone(&capture_id)
            .map_err(|error| storage_error(error, 5))?
            .is_some()
        {
            let mut error = AppError::new(
                "CAPTURE_UNVERIFIABLE",
                format!(
                    "Capture {capture_id} was discarded before its commit could be verified; choose a new capture ID."
                ),
                6,
            );
            error.data = Some(json!({"captureId": capture_id, "saveState": "unknown"}));
            return Err(error);
        }
        if state
            .read_binding(&capture_id)
            .map_err(|error| storage_error(error, 5))?
            .is_some()
        {
            return Err(AppError::new(
                "CAPTURE_ID_EXPIRED",
                format!(
                    "Capture {capture_id} has no recoverable record; its durable binding prevents replay."
                ),
                2,
            ));
        }
        return Err(AppError::new(
            "CAPTURE_NOT_FOUND",
            format!("No pending or committed capture named {capture_id}"),
            3,
        ));
    };

    let mut storage_warnings = Vec::new();
    // A status check may safely perform the core's read-only UUID
    // reconciliation. It never creates a backup, repairs IDs or inserts.
    if let Some(request) = record.request.as_ref() {
        match capsule_core::reconcile_capture_for_database(request) {
            Ok(status) if status.outcome == CaptureOutcome::Committed => {
                if let Some(receipt) = status.receipt {
                    record.mark_committed(receipt);
                    if let Err(error) = state.write_receipt(&record) {
                        storage_warnings.push(format!(
                            "Confirmed save, but receipt storage is unavailable: {error}"
                        ));
                        if let Err(pending_error) = state.write_pending(&record) {
                            storage_warnings.push(format!(
                                "Recovery record update also failed; do not retry until storage is restored: {pending_error}"
                            ));
                        }
                    } else {
                        match state.write_binding(&record) {
                            Ok(()) => {
                                let _ = state.remove_pending(&capture_id);
                            }
                            Err(error) => {
                                storage_warnings.push(format!(
                                    "Confirmed save, but capture-ID binding storage is unavailable: {error}"
                                ));
                                // Keep the committed pending record when the
                                // durable explicit-ID binding could not be
                                // written; status can finish it later.
                                let _ = state.write_pending(&record);
                            }
                        }
                    }
                }
            }
            Ok(status) => {
                if status.message.is_some() {
                    record.last_error = status.message;
                }
            }
            Err(error) if error.code == CaptureErrorCode::DatabaseReplaced => {
                return Err(AppError::new("DB_REPLACED", error.message, 4));
            }
            Err(error) => {
                record.last_error = Some(error.to_string());
            }
        }
    }
    let mut output = output_for_record(&record, global)?;
    output.warnings.extend(storage_warnings);
    Ok(output)
}

pub(crate) fn output_for_record(
    record: &RecoveryRecord,
    global: &GlobalOptions,
) -> Result<CommandOutput, AppError> {
    let data = record_data(record);
    let human = format!(
        "Capture {}: {} ({})",
        record.capture_id,
        record.display_state(),
        record
            .receipt
            .as_ref()
            .map(|receipt| receipt.uuid.as_str())
            .unwrap_or("not committed")
    );
    let mut output = CommandOutput::new(data, human);
    output.quiet = Some(record.display_state().to_string());
    output.committed = record.state == RecoveryState::Committed;
    if let Some(context) = record.context.as_ref() {
        output.warnings = context
            .warnings
            .iter()
            .map(|warning| cap_effects::sanitize_text(warning))
            .collect();
    }
    if global.json || global.quiet {
        // `human` is never emitted in machine modes; retaining this branch
        // documents that no content/body is added for those paths.
    }
    Ok(output)
}

pub(crate) fn record_data(record: &RecoveryRecord) -> Value {
    let context = record.context.as_ref().map(enrich::sanitized_result);
    json!({
        "captureId": record.capture_id,
        "reservedUuid": record.reserved_uuid,
        "saveState": record.display_state(),
        "receipt": record.receipt,
        "context": context,
        "updatedAt": record.updated_at.to_rfc3339(),
        "expiresAt": record.expires_at.map(|value| value.to_rfc3339()),
        "lastError": record.last_error,
    })
}

fn storage_error(error: StateError, exit: i32) -> AppError {
    AppError::new("RECOVERY_STORAGE", error.to_string(), exit)
}
