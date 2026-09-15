//! Quick capture and its post-commit context hand-off.

use std::{
    io::{self, IsTerminal, Write},
    sync::atomic::{AtomicU64, Ordering},
    sync::{atomic::AtomicBool, Arc},
    thread::JoinHandle,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::anyhow;
use capsule_core::{
    capture::{CaptureError, CaptureErrorCode, CaptureHookPoint, CaptureHooks},
    context::{
        Cancellation, ContextDependencies, ContextRequest, ContextService, NoopContextCache,
        ReqwestHttpClient, SystemClock,
    },
    contracts::{BackupPolicy, CaptureOutcome, CaptureRequest, ContextResult},
    db::{self, ResolvedCapsule},
    identity,
};
use chrono::{Local, Utc};
use serde_json::json;

use crate::{
    app::{AppError, CommandOutput},
    cancellation,
    cli::{AddArgs, ContentFormat, GlobalOptions},
    context_cache::PersistentContextCache,
    input, preferences, query,
    recovery::{self, RecoveryRecord, RecoveryState, StateError, StateStore},
    ui::receipt::{self, ReceiptModel},
};

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Execute explicit `cap add`.
pub fn run(args: &AddArgs, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let mut stdin = io::stdin().lock();
    let stdin_is_terminal = io::stdin().is_terminal();
    let text = input::read_add(args, &mut stdin, stdin_is_terminal)?;
    capture(args, global, text, true)
}

/// Replay one locally pending request after an explicit reconciliation step.
/// The request loaded from disk owns its original UUID, timestamp and binding;
/// this helper never re-resolves those fields from current process settings.
pub fn retry_capture_id(
    capture_id: &str,
    global: &GlobalOptions,
) -> Result<CommandOutput, AppError> {
    let capture_id = identity::validate_capture_id(capture_id)
        .map_err(|error| AppError::new("INVALID_INPUT", error.to_string(), 2))?;
    let state =
        StateStore::from_env().map_err(|error| AppError::new("RECOVERY_STORAGE", error, 5))?;
    let record = state
        .read_pending(&capture_id)
        .map_err(|error| storage_error(error, 5))?
        .ok_or_else(|| {
            AppError::new(
                "CAPTURE_NOT_FOUND",
                format!("No pending capture named {capture_id}"),
                3,
            )
        })?;
    let request = record.request.clone().ok_or_else(|| {
        AppError::new(
            "RECOVERY_STORAGE",
            "pending capture is missing its request",
            5,
        )
    })?;
    if request.capture_id != capture_id {
        return Err(AppError::new(
            "RECOVERY_STORAGE",
            "pending capture request and filename IDs do not match",
            5,
        ));
    }
    capture_frozen_request(request, global)
}

/// Replay a persisted request without rebuilding it from current CLI flags.
/// The original body, normalized metadata, timestamp, reserved UUID, backup
/// policy and database binding remain authoritative; the active resolver is
/// checked only to reject a replaced/missing destination before any write.
pub fn capture_frozen_request(
    request: CaptureRequest,
    global: &GlobalOptions,
) -> Result<CommandOutput, AppError> {
    capture_internal(global, request.text.clone(), true, Some(request))
}

fn capture(
    args: &AddArgs,
    global: &GlobalOptions,
    text: String,
    explicit_add: bool,
) -> Result<CommandOutput, AppError> {
    capture_internal_with_args(args, global, text, explicit_add)
}

fn capture_internal(
    global: &GlobalOptions,
    text: String,
    explicit_add: bool,
    frozen: Option<CaptureRequest>,
) -> Result<CommandOutput, AppError> {
    capture_internal_seeded(global, text, explicit_add, frozen, None)
}

fn capture_internal_with_args(
    args: &AddArgs,
    global: &GlobalOptions,
    text: String,
    explicit_add: bool,
) -> Result<CommandOutput, AppError> {
    capture_internal_seeded(global, text, explicit_add, None, Some(args))
}

fn capture_internal_seeded(
    global: &GlobalOptions,
    text: String,
    explicit_add: bool,
    frozen: Option<CaptureRequest>,
    args: Option<&AddArgs>,
) -> Result<CommandOutput, AppError> {
    validate_presentation_flags(global)?;
    let created_at = Local::now().fixed_offset();
    let resolved = query::resolve(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    if !resolved.capabilities.supports_write() {
        let reason = resolved
            .capabilities
            .write_support_reasons
            .first()
            .cloned()
            .unwrap_or_else(|| "database schema does not support capture writes".to_string());
        return Err(AppError::new("UNSUPPORTED_SCHEMA", reason, 3));
    }

    // Read-only entry readers intentionally refuse ID repair. Capture instead
    // lets the shared mutation path perform supported repairs after backup.
    let context_settings = load_context_settings(&resolved)?;
    let default_backup_policy = backup_policy(&resolved);
    let frozen_request = frozen.is_some();
    let mut request = if let Some(request) = frozen {
        identity::validate_capture_id(&request.capture_id)
            .map_err(|error| AppError::new("INVALID_INPUT", error.to_string(), 2))?;
        identity::validate_reserved_uuid(&request.reserved_uuid)
            .map_err(|error| AppError::new("INVALID_INPUT", error.to_string(), 2))?;
        if !request_matches_resolved(&request, &resolved) {
            return Err(AppError::new(
                "DB_REPLACED",
                "the pending capture is bound to a different or replaced database",
                4,
            ));
        }
        if request.backup_policy.is_none() {
            return Err(AppError::new(
                "RECOVERY_STORAGE",
                "pending capture is missing its frozen backup policy",
                5,
            ));
        }
        request
    } else {
        let args = args.expect("normal captures provide CLI arguments");
        let capture_id = match args.capture_id.as_deref() {
            Some(value) => identity::validate_capture_id(value)
                .map_err(|error| AppError::new("INVALID_INPUT", error.to_string(), 2))?,
            None => new_capture_id(),
        };
        let reserved_uuid = crate::entry_id::new_entry_uuid(&resolved.database_path)
            .map_err(|error| AppError::new("DB_READ", error.to_string(), 3))?;
        let mut request = CaptureRequest::new(
            text,
            capture_id,
            reserved_uuid,
            resolved.database_path.clone(),
            created_at,
        );
        request.content_format = match args.format.unwrap_or(ContentFormat::Markdown) {
            ContentFormat::Markdown => "markdown".to_string(),
            ContentFormat::Plain => "plain".to_string(),
        };
        request.title = args.title.clone();
        request.summary = args.summary.clone();
        request.mood = args.mood.clone();
        request.tags = args.tags.clone();
        request.starred = args.star;
        request.pinned = args.pin;
        request.continue_from_uuid = match args.continue_from.as_deref() {
            Some(alias) => {
                let parent = capsule_core::JournalReader::open(resolved.database_path.clone())
                    .and_then(|reader| reader.get(alias, true))
                    .map_err(|error| AppError::new("INVALID_CONTINUATION", error.to_string(), 2))?;
                Some(parent.uuid)
            }
            None => None,
        };
        request.database_identity = resolved.database_identity.clone();
        request.backup_policy = Some(default_backup_policy.clone());
        request
    };
    let capture_id = request.capture_id.clone();

    if global.dry_run {
        return dry_run(&request, &resolved, &context_settings, global);
    }

    let state =
        StateStore::from_env().map_err(|error| AppError::new("RECOVERY_STORAGE", error, 5))?;
    let fingerprint = recovery::request_fingerprint(&request)
        .map_err(|error| AppError::new("INVALID_INPUT", error, 2))?;
    let explicit_capture_id = if frozen_request {
        true
    } else {
        explicit_add && args.is_some_and(|value| value.capture_id.is_some())
    };

    // A receipt/binding is checked before any new state can be written. This
    // is the caller-controlled idempotency path; ordinary repeated text has
    // a newly generated capture ID and intentionally creates another entry.
    if let Some(existing) = state
        .read_receipt(&capture_id)
        .map_err(|error| storage_error(error, 5))?
    {
        return existing_result(&existing, &request, &fingerprint, global);
    }
    if let Some(binding) = state
        .read_binding(&capture_id)
        .map_err(|error| storage_error(error, 5))?
    {
        if !binding_matches_request(&binding, &request) {
            return Err(AppError::new(
                "DB_REPLACED",
                format!("capture ID {capture_id} is bound to a different database"),
                4,
            ));
        }
        if binding.request_fingerprint != fingerprint {
            return Err(AppError::new(
                "CAPTURE_ID_CONFLICT",
                format!("capture ID {capture_id} is already bound to different content"),
                2,
            ));
        }
        // A missing receipt with a durable binding means its 30-day receipt
        // expired. Never silently create a duplicate under that ID.
        return Err(AppError::new(
            "CAPTURE_ID_EXPIRED",
            format!("capture ID {capture_id} has expired; use cap status or a new capture ID"),
            2,
        ));
    }
    if let Some(tombstone) = state
        .read_tombstone(&capture_id)
        .map_err(|error| storage_error(error, 5))?
    {
        return tombstone_reuse_error(&tombstone, &request, &fingerprint);
    }

    let _record_lock = state.lock(&capture_id).map_err(|error| match error {
        StateError::Busy(message) => AppError::new("CAPTURE_BUSY", message, 4),
        other => storage_error(other, 5),
    })?;

    // Re-check while holding the record lock so two processes racing the same
    // caller-controlled ID cannot both reserve/write a request.
    if let Some(existing) = state
        .read_receipt(&capture_id)
        .map_err(|error| storage_error(error, 5))?
    {
        return existing_result(&existing, &request, &fingerprint, global);
    }
    if let Some(binding) = state
        .read_binding(&capture_id)
        .map_err(|error| storage_error(error, 5))?
    {
        if !binding_matches_request(&binding, &request) {
            return Err(AppError::new(
                "DB_REPLACED",
                format!("capture ID {capture_id} is bound to a different database"),
                4,
            ));
        }
        if binding.request_fingerprint != fingerprint {
            return Err(AppError::new(
                "CAPTURE_ID_CONFLICT",
                format!("capture ID {capture_id} is already bound to different content"),
                2,
            ));
        }
        return Err(AppError::new(
            "CAPTURE_ID_EXPIRED",
            format!("capture ID {capture_id} has expired; use cap status or a new capture ID"),
            2,
        ));
    }
    if let Some(tombstone) = state
        .read_tombstone(&capture_id)
        .map_err(|error| storage_error(error, 5))?
    {
        return tombstone_reuse_error(&tombstone, &request, &fingerprint);
    }
    let mut record = if let Some(existing) = state
        .read_pending(&capture_id)
        .map_err(|error| storage_error(error, 5))?
    {
        if !record_binding_matches(&existing, &request) {
            return Err(AppError::new(
                "DB_REPLACED",
                format!("capture ID {capture_id} is bound to a different database"),
                4,
            ));
        }
        if existing.request_fingerprint != fingerprint {
            return Err(AppError::new(
                "CAPTURE_ID_CONFLICT",
                format!("capture ID {capture_id} is already pending for different content"),
                2,
            ));
        }
        if existing.state == RecoveryState::Committed {
            return existing_result(&existing, &request, &fingerprint, global);
        }
        // A crashed/unknown request owns its original timestamp and UUID.
        request = existing.request.clone().ok_or_else(|| {
            AppError::new(
                "RECOVERY_STORAGE",
                "pending capture is missing its request",
                5,
            )
        })?;
        existing
    } else {
        let record =
            RecoveryRecord::pending(request.clone(), fingerprint.clone(), explicit_capture_id);
        #[cfg(feature = "test-hooks")]
        crate::test_hooks::stage("before_pending")
            .map_err(|error| AppError::new("TEST_HOOK", error.to_string(), 1))?;
        state
            .write_pending(&record)
            .map_err(|error| storage_error(error, 5))?;
        record
    };

    // An explicit recovery retry submits an editable writer draft. Freeze it
    // under the existing record lock before any shared-core mutation, so a
    // later writer session cannot edit a request that may have been committed.
    if record.is_editable_writer_draft() {
        record.writer_draft = None;
        record.updated_at = Utc::now();
        state
            .write_pending(&record)
            .map_err(|error| storage_error(error, 5))?;
    }

    let _ = cancellation::install();
    if cancellation::requested() {
        return cancelled_before_commit(&mut record, &state);
    }
    let hooks = CancellationHooks;
    let capture_result = {
        let indicator = WorkingIndicator::start(global);
        let result = capsule_core::capture_entry_with_hooks_for_database(request.clone(), &hooks);
        indicator.finish();
        result
    };
    let receipt = match capture_result {
        Ok(receipt) => receipt,
        Err(error) if error.outcome == CaptureOutcome::Committed && error.receipt.is_some() => {
            error.receipt.expect("committed error carries a receipt")
        }
        Err(error) => return handle_capture_error(error, &mut record, &state),
    };

    #[cfg(feature = "test-hooks")]
    crate::test_hooks::stage("after_commit")
        .map_err(|error| AppError::new("TEST_HOOK", error.to_string(), 1))?;

    record.mark_committed(receipt.clone());
    let mut warnings = Vec::new();
    if let Err(error) = state.write_receipt(&record) {
        // Keep the confirmed receipt in pending state for reconciliation. A
        // local receipt failure can never turn the journal commit into a retry.
        warnings.push(format!(
            "Receipt storage is unavailable; use cap status --capture-id {}: {error}",
            capture_id
        ));
        let _ = state.write_pending(&record);
    } else {
        let binding_durable = match state.write_binding(&record) {
            Ok(()) => true,
            Err(error) => {
                warnings.push(format!("Capture ID binding could not be retained: {error}"));
                false
            }
        };
        // A committed receipt without its explicit-ID binding remains in the
        // pending store so a later status/recovery pass can finish the
        // protection.  Removing it would allow a duplicate after expiry.
        if binding_durable {
            let _ = state.remove_pending(&capture_id);
        } else {
            let _ = state.write_pending(&record);
        }
    }

    // The per-record lock only protects reservation/commit and the first
    // durable receipt transition.  Provider work and terminal rendering must
    // not hold it for the full context deadline.
    drop(_record_lock);

    // Human acknowledgement is deliberately flushed before optional provider
    // work. JSON/quiet invocations remain silent until their final envelope.
    immediate_ack(&receipt, global);

    let context = if global.no_context {
        Some(ContextResult::default())
    } else {
        let context = capture_context(&request, &resolved, &context_settings, global);
        match context {
            Ok(result) => Some(result),
            Err(error) => {
                warnings.push(format!("Context unavailable after save: {error}"));
                Some(ContextResult::default())
            }
        }
    };
    record.context = context.clone();
    record.updated_at = Utc::now();
    if record.state == RecoveryState::Committed {
        match state.lock(&capture_id) {
            Ok(_final_lock) => {
                if let Err(error) = state.write_receipt(&record) {
                    warnings.push(format!("Final receipt update failed: {error}"));
                    let _ = state.write_pending(&record);
                }
            }
            Err(error) => warnings.push(format!("Final receipt lock unavailable: {error}")),
        }
    }

    let mut model = ReceiptModel::from_parts(&request, &receipt, context.as_ref());
    if !cancellation::requested() {
        if let Some(identity) = request.database_identity.as_ref() {
            match crate::insights::MemoryStateStore::from_env() {
                Ok(store) => {
                    let milestones = store.milestone_receipt_for_committed(
                        &request.database_path,
                        identity,
                        Local::now().date_naive(),
                        &receipt.uuid,
                        false,
                    );
                    model.milestones = milestones.glints;
                    warnings.extend(milestones.warnings);
                }
                Err(error) => warnings.push(format!("Milestones unavailable: {error}")),
            }
        }
    }
    let (human, effect_warning) = render_receipt(&request, &model, global);
    if let Some(warning) = effect_warning {
        warnings.push(warning);
    }
    let data = serde_json::to_value(&model)
        .map_err(|error| AppError::new("OUTPUT", error.to_string(), 1))?;
    let mut output = CommandOutput::new(data, human);
    output.quiet = Some(receipt.uuid.clone());
    output.warnings = warnings;
    if let Some(context) = context {
        output.warnings.extend(
            context
                .warnings
                .into_iter()
                .map(|warning| cap_effects::sanitize_text(&warning)),
        );
    }
    output.committed = true;
    Ok(output)
}

fn dry_run(
    request: &CaptureRequest,
    resolved: &ResolvedCapsule,
    settings: &capsule_core::location::ContextSettings,
    global: &GlobalOptions,
) -> Result<CommandOutput, AppError> {
    let mut policy = settings.policy.clone();
    if global.no_context {
        policy.auto_capture = false;
        policy.allow_network = false;
        policy.allow_cache = false;
    } else if global.offline {
        policy.allow_network = false;
    }
    let data = json!({
        "dryRun": true,
        "saveState": "not_committed",
        "captureId": request.capture_id,
        "reservedUuid": request.reserved_uuid,
        "text": request.text,
        "createdAt": request.created_at.to_rfc3339(),
        "wordCount": request.text.split_whitespace().count(),
        "contentFormat": request.content_format,
        "title": request.title,
        "summary": request.summary,
        "mood": request.mood,
        "tags": request.tags,
        "starred": request.starred,
        "pinned": request.pinned,
        "continueFromUuid": request.continue_from_uuid,
        "databasePath": db::path_to_string(&resolved.database_path),
        "databaseIdentity": resolved.database_identity,
        "schema": resolved.capabilities.schema,
        "contextPolicy": policy,
        "configValid": resolved.config_valid && settings.valid,
    });
    let human = format!(
        "Dry run: {} words; no backup, entry, context, cache or network work performed.",
        request.text.split_whitespace().count()
    );
    Ok(CommandOutput::new(data, human))
}

fn load_context_settings(
    resolved: &ResolvedCapsule,
) -> Result<capsule_core::location::ContextSettings, AppError> {
    let config_path = match resolved.config_source {
        db::PathSource::Explicit | db::PathSource::Environment => resolved.config_path.as_deref(),
        _ => None,
    };
    capsule_core::location::load_context_settings(&resolved.database_path, config_path)
        .map_err(|error| AppError::new("CONTEXT_READ", error.to_string(), 3))
}

fn backup_policy(resolved: &ResolvedCapsule) -> BackupPolicy {
    BackupPolicy::new(
        resolved.backup_directory.clone(),
        resolved
            .settings
            .backup_retention_count
            .unwrap_or(db::DEFAULT_BACKUP_RETENTION_COUNT),
    )
}

fn validate_presentation_flags(global: &GlobalOptions) -> Result<(), AppError> {
    if let Some(theme) = global.theme.as_deref() {
        if crate::ui::themes::Theme::parse(theme).is_none() {
            return Err(AppError::new(
                "INVALID_CONFIG",
                format!(
                    "Unknown theme `{}`. Choose one of: aurora, neon, c64, amber, paper.",
                    cap_effects::sanitize_text(theme)
                ),
                2,
            ));
        }
    }
    Ok(())
}

fn request_matches_resolved(request: &CaptureRequest, resolved: &ResolvedCapsule) -> bool {
    let left = request
        .database_path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    let right = resolved
        .database_path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    left == right
        && matches!(
            (&request.database_identity, &resolved.database_identity),
            (Some(left), Some(right)) if left.same_file(right)
        )
}

fn capture_context(
    request: &CaptureRequest,
    resolved: &ResolvedCapsule,
    settings: &capsule_core::location::ContextSettings,
    global: &GlobalOptions,
) -> Result<ContextResult, String> {
    let mut policy = settings.policy.clone();
    policy.database_path = resolved.database_path.clone();
    if global.offline {
        policy.allow_network = false;
    }
    let mut context_request =
        ContextRequest::capture(&resolved.database_path, &request.reserved_uuid, policy);
    context_request.database_identity = request.database_identity.clone();
    context_request.config_valid = resolved.config_valid && settings.valid;
    let cache: Arc<dyn capsule_core::context::ContextCache> = if global.no_context {
        Arc::new(NoopContextCache)
    } else {
        let identity = request
            .database_identity
            .clone()
            .ok_or_else(|| "database identity is unavailable".to_string())?;
        Arc::new(PersistentContextCache::at_path(
            StateStore::from_env()
                .map_err(|error| error.to_string())?
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
    service
        .capture(&context_request)
        .map_err(|error| error.to_string())
}

fn render_receipt(
    request: &CaptureRequest,
    model: &ReceiptModel,
    global: &GlobalOptions,
) -> (String, Option<String>) {
    let capabilities = cap_effects::TerminalCapabilities::detect();
    let presentation =
        preferences::resolve_from_store(global, &capabilities).unwrap_or_else(|_| {
            preferences::resolve_defaults(global, &capabilities)
                .expect("presentation flags validated before save")
        });
    let theme = presentation.theme;
    let output = presentation.output;
    let color = output.color();
    let animate = matches!(
        output,
        cap_effects::ResolvedOutputMode::Human {
            motion: cap_effects::MotionMode::Full,
            color: effect_color,
        } if capabilities.is_tty && effect_color != cap_effects::ColorMode::Plain
    );
    if animate && presentation.icon_mode != preferences::IconMode::Ascii {
        let mut stdout = io::stdout().lock();
        match receipt::animate_seal(
            &mut stdout,
            model,
            theme,
            color,
            &capabilities,
            cancellation::requested,
        ) {
            Ok(result) if !result.cancelled => {
                return (
                    receipt::render_with_preferences(
                        Some(request),
                        model,
                        &presentation,
                        capabilities.compact_width(),
                        false,
                    ),
                    None,
                );
            }
            Ok(_) => {
                // Ctrl+C after commit stops decoration only.  The static
                // receipt remains the authoritative user-visible result.
            }
            Err(error) => {
                return (
                    receipt::render_with_preferences(
                        Some(request),
                        model,
                        &presentation,
                        capabilities.compact_width(),
                        true,
                    ),
                    Some(format!("Seal effect unavailable after save: {error}")),
                );
            }
        }
    }
    (
        receipt::render_with_preferences(
            Some(request),
            model,
            &presentation,
            capabilities.compact_width(),
            true,
        ),
        None,
    )
}

fn existing_result(
    record: &RecoveryRecord,
    request: &CaptureRequest,
    fingerprint: &str,
    global: &GlobalOptions,
) -> Result<CommandOutput, AppError> {
    if !record_binding_matches(record, request) {
        return Err(AppError::new(
            "DB_REPLACED",
            format!(
                "capture ID {} is bound to a different database",
                record.capture_id
            ),
            4,
        ));
    }
    if record.request_fingerprint != fingerprint {
        return Err(AppError::new(
            "CAPTURE_ID_CONFLICT",
            format!(
                "capture ID {} is already bound to different content",
                record.capture_id
            ),
            2,
        ));
    }
    let Some(receipt) = record.receipt.as_ref() else {
        return Err(AppError::new(
            "CAPTURE_PENDING",
            format!(
                "capture {} is pending; use cap recover retry {}",
                record.capture_id, record.capture_id
            ),
            4,
        ));
    };
    let model = ReceiptModel::from_record(record).ok_or_else(|| {
        AppError::new(
            "RECOVERY_STORAGE",
            "committed receipt metadata is incomplete",
            5,
        )
    })?;
    let human = render_saved_receipt(&model, global);
    let data = serde_json::to_value(&model)
        .map_err(|error| AppError::new("OUTPUT", error.to_string(), 1))?;
    let mut output = CommandOutput::new(data, human);
    output.quiet = Some(receipt.uuid.clone());
    output.committed = true;
    if let Some(context) = record.context.as_ref() {
        output.warnings = context
            .warnings
            .iter()
            .map(|warning| cap_effects::sanitize_text(warning))
            .collect();
    }
    Ok(output)
}

fn record_binding_matches(record: &RecoveryRecord, request: &CaptureRequest) -> bool {
    let left = record
        .database_path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    let right = request
        .database_path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    if left != right {
        return false;
    }
    match (&record.database_identity, &request.database_identity) {
        (Some(left), Some(right)) => left.same_file(right),
        _ => false,
    }
}

fn binding_matches_request(binding: &recovery::CaptureBinding, request: &CaptureRequest) -> bool {
    let left = binding
        .database_path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    let right = request
        .database_path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    left == right
        && matches!(
            (&binding.database_identity, &request.database_identity),
            (Some(left), Some(right)) if left.same_file(right)
        )
}

fn tombstone_reuse_error(
    tombstone: &recovery::CaptureTombstone,
    request: &CaptureRequest,
    fingerprint: &str,
) -> Result<CommandOutput, AppError> {
    if !tombstone_matches_request(tombstone, request) {
        return Err(AppError::new(
            "DB_REPLACED",
            format!(
                "capture ID {} is bound to a different database",
                tombstone.capture_id
            ),
            4,
        ));
    }
    if tombstone.request_fingerprint != fingerprint {
        return Err(AppError::new(
            "CAPTURE_ID_CONFLICT",
            format!(
                "capture ID {} is already reserved for different content",
                tombstone.capture_id
            ),
            2,
        ));
    }
    let mut error = AppError::new(
        "CAPTURE_UNVERIFIABLE",
        format!(
            "capture ID {} was discarded before its commit could be verified; choose a new capture ID",
            tombstone.capture_id
        ),
        6,
    );
    error.data = Some(json!({
        "captureId": tombstone.capture_id,
        "saveState": "unknown",
        "reservedUuid": tombstone.reserved_uuid,
    }));
    Err(error)
}

fn tombstone_matches_request(
    tombstone: &recovery::CaptureTombstone,
    request: &CaptureRequest,
) -> bool {
    let left = tombstone
        .database_path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    let right = request
        .database_path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    left == right
        && matches!(
            (&tombstone.database_identity, &request.database_identity),
            (Some(left), Some(right)) if left.same_file(right)
        )
}

fn render_saved_receipt(model: &ReceiptModel, global: &GlobalOptions) -> String {
    // A confirmed receipt intentionally has no body text. Keep its final
    // status readable without reconstructing or inventing authored content.
    let capabilities = cap_effects::TerminalCapabilities::detect();
    let presentation = preferences::resolve_from_store(global, &capabilities)
        .or_else(|_| preferences::resolve_defaults(global, &capabilities));
    match presentation {
        Ok(style) => receipt::render_with_preferences(
            None,
            model,
            &style,
            capabilities.compact_width(),
            true,
        ),
        Err(_) => receipt::render_saved(
            model,
            crate::ui::themes::Theme::Paper,
            cap_effects::ColorMode::Plain,
            capabilities.compact_width(),
        ),
    }
}

fn handle_capture_error(
    error: CaptureError,
    record: &mut RecoveryRecord,
    state: &StateStore,
) -> Result<CommandOutput, AppError> {
    if error.outcome == CaptureOutcome::Unknown {
        record.mark_unknown(error.message.clone());
        let storage_warning = state.write_pending(record).err().map(|storage| {
            format!(
                " Recovery state could not be updated; preserve the original pending record and run status again: {storage}"
            )
        });
        let mut app_error = AppError::new(
            "COMMIT_UNKNOWN",
            format!(
                "Commit result is unknown for capture {}. Run cap status --capture-id {} before retrying.",
                record.capture_id, record.capture_id
            ) + storage_warning.as_deref().unwrap_or(""),
            6,
        );
        app_error.data = Some(json!({
            "captureId": record.capture_id,
            "saveState": "unknown",
            "reservedUuid": record.reserved_uuid,
            "recoveryWriteError": storage_warning,
        }));
        return Err(app_error);
    }
    record.last_error = Some(error.message.clone());
    record.updated_at = Utc::now();
    let _ = state.write_pending(record);
    if cancellation::requested() {
        return Err(AppError {
            detail: crate::contracts::CliError::new(
                "CANCELLED",
                "Capture cancelled before commit",
                false,
            ),
            exit_code: 130,
            data: Some(json!({"captureId": record.capture_id, "saveState": "not_committed"})),
        });
    }
    let (code, exit) = match error.code {
        CaptureErrorCode::DatabaseBusy => ("DB_BUSY", 4),
        CaptureErrorCode::BackupFailed => ("BACKUP_FAILED", 5),
        CaptureErrorCode::DatabaseReplaced => ("DB_REPLACED", 4),
        CaptureErrorCode::IdentityConflict => ("CAPTURE_ID_CONFLICT", 2),
        CaptureErrorCode::InvalidInput => ("INVALID_INPUT", 2),
        CaptureErrorCode::CommitUnknown => ("COMMIT_UNKNOWN", 6),
        CaptureErrorCode::CommittedReceiptUnavailable => ("COMMIT_UNKNOWN", 6),
    };
    let message = if error.code == CaptureErrorCode::BackupFailed {
        format!(
            "{}. Entry was not saved. Retry with: cap recover retry {}",
            error.message, record.capture_id
        )
    } else {
        error.message
    };
    let mut app_error = AppError::new(code, message, exit);
    app_error.data = Some(json!({
        "captureId": record.capture_id,
        "saveState": "not_committed",
        "reservedUuid": record.reserved_uuid,
    }));
    Err(app_error)
}

fn cancelled_before_commit(
    record: &mut RecoveryRecord,
    state: &StateStore,
) -> Result<CommandOutput, AppError> {
    record.last_error = Some("capture cancelled before commit".to_string());
    record.updated_at = Utc::now();
    state
        .write_pending(record)
        .map_err(|error| storage_error(error, 5))?;
    Err(AppError {
        detail: crate::contracts::CliError::new(
            "CANCELLED",
            "Capture cancelled before commit",
            false,
        ),
        exit_code: 130,
        data: Some(json!({"captureId": record.capture_id, "saveState": "not_committed"})),
    })
}

struct CancellationHooks;

impl CaptureHooks for CancellationHooks {
    fn checkpoint(&self, point: CaptureHookPoint) -> anyhow::Result<()> {
        #[cfg(feature = "test-hooks")]
        crate::test_hooks::checkpoint(point)?;
        if !matches!(
            point,
            CaptureHookPoint::DuringCommit | CaptureHookPoint::AfterCommit
        ) && cancellation::requested()
        {
            return Err(anyhow!("capture cancelled at {point:?}"));
        }
        Ok(())
    }
}

struct SignalCancellation;

impl Cancellation for SignalCancellation {
    fn is_cancelled(&self) -> bool {
        cancellation::requested()
    }
}

fn immediate_ack(receipt: &capsule_core::contracts::CommitReceipt, global: &GlobalOptions) {
    if global.json || global.quiet {
        return;
    }
    let mut stdout = io::stdout().lock();
    if writeln!(stdout, "Saved {}", receipt.uuid).is_ok() {
        let _ = stdout.flush();
    }
}

/// A pre-commit-only, output-owned working indicator.  It waits 150 ms before
/// touching the terminal, so short saves and every redirected/machine mode are
/// silent.  The caller always stops and joins it before emitting either an
/// error or the immediate `Saved` acknowledgement.
struct WorkingIndicator {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl WorkingIndicator {
    fn start(global: &GlobalOptions) -> Self {
        let capabilities = cap_effects::TerminalCapabilities::detect();
        let presentation = preferences::resolve_from_store(global, &capabilities).ok();
        let enabled = capabilities.is_tty
            && presentation.is_some_and(|value| {
                matches!(
                    value.output,
                    cap_effects::ResolvedOutputMode::Human {
                        motion: cap_effects::MotionMode::Full,
                        ..
                    }
                )
            });
        let stop = Arc::new(AtomicBool::new(false));
        if !enabled {
            return Self { stop, worker: None };
        }
        let thread_stop = Arc::clone(&stop);
        let width = capabilities.compact_width();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            if thread_stop.load(Ordering::SeqCst) {
                return;
            }
            let mut stdout = io::stdout().lock();
            let mut guard = match cap_effects::TerminalGuard::new(
                &mut stdout,
                cap_effects::GuardOptions::default(),
            ) {
                Ok(guard) => guard,
                Err(_) => return,
            };
            let mut frame = 0usize;
            while !thread_stop.load(Ordering::SeqCst) {
                let glyph = ["·", "✦", "·", "✧"][frame % 4];
                let message = format!("{glyph} Saving to Capsule");
                let line = cap_effects::layout_text(&message, width, false)
                    .plain_lines()
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| "Saving to Capsule".to_string());
                let _ = write!(guard.writer(), "\r\x1b[2K{line}");
                let _ = guard.writer().flush();
                frame = frame.wrapping_add(1);
                std::thread::sleep(std::time::Duration::from_millis(80));
            }
            let _ = guard.restore();
            let _ = write!(guard.writer(), "\r\x1b[2K");
            let _ = guard.writer().flush();
        });
        Self {
            stop,
            worker: Some(worker),
        }
    }

    fn finish(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn storage_error(error: StateError, exit: i32) -> AppError {
    AppError::new("RECOVERY_STORAGE", error.to_string(), exit)
}

fn new_capture_id() -> String {
    format!(
        "cap_{}_{:x}_{}",
        unix_nanos(),
        std::process::id(),
        ID_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn generated_capture_ids_are_valid() {
        let capture = new_capture_id();
        assert!(identity::validate_capture_id(&capture).is_ok());
    }

    #[test]
    fn default_positional_words_use_input_normalization() {
        let words = [
            OsString::from("write-the-entry-here"),
            OsString::from("two  spaces"),
        ];
        assert_eq!(
            input::from_words(&words).unwrap(),
            "write-the-entry-here two  spaces"
        );
    }
}
