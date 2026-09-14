//! Draft-backed interactive writer command.
//!
//! The command performs all validation before creating a local pending draft,
//! then keeps the per-capture recovery lock for the entire editing session.
//! Saving clears the editable marker while that lock is held and only then
//! delegates the immutable request to `commands::add::capture_frozen_request`.

use std::{
    io::{self, IsTerminal},
    time::{Duration, Instant},
};

use capsule_core::{
    contracts::CaptureRequest,
    db::{self, ResolvedCapsule, WriterPreferences},
};
use crossterm::event;
use serde_json::json;

use crate::{
    app::{AppError, CommandOutput},
    cli::{GlobalOptions, WriteArgs},
    commands::add,
    preferences::{self, EffectivePresentation},
    query,
    recovery::{StateError, StateStore},
    writer::{
        draft::{self, DraftError, DraftLease},
        editor,
        input::{WriterAction, WriterMachine},
        terminal,
    },
};

const AUTOSAVE_IDLE: Duration = Duration::from_millis(500);
const AMBIENT_FRAME: Duration = Duration::from_millis(67); // <= 15 fps

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InteractiveOutcome {
    Save,
    Exit,
    Interrupted,
}

/// Execute `cap write` after validating terminal, settings and destination
/// state. No local draft is touched until all of those checks succeed.
pub fn run(args: &WriteArgs, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    preflight_terminal(global)?;
    let capabilities = cap_effects::TerminalCapabilities::detect();
    let presentation = preferences::resolve_from_store(global, &capabilities)
        .map_err(|error| AppError::new("INVALID_CONFIG", error.to_string(), 2))?;
    let resolved = query::resolve(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    validate_resolved(&resolved)?;
    let target = effective_writer_preferences(&resolved, &presentation);
    if args.editor && presentation.editor_executable.is_none() {
        return Err(AppError::new(
            "EDITOR_UNAVAILABLE",
            "No editor executable is configured; set editor.executable first.",
            2,
        ));
    }
    crate::cancellation::install().map_err(|error| {
        AppError::new(
            "WRITER_INPUT",
            format!("Unable to install interruption handling before writer start: {error}"),
            1,
        )
    })?;
    let state =
        StateStore::from_env().map_err(|error| AppError::new("RECOVERY_STORAGE", error, 5))?;

    let mut lease = acquire_lease(&state, &resolved)?;
    if args.editor {
        return run_editor(&mut lease, &resolved, &presentation, &target, global);
    }
    run_interactive(
        &mut lease,
        &resolved,
        &presentation,
        &target,
        &capabilities,
        global,
    )
}

/// Entry point used by the no-argument TTY router in `app`.
pub fn run_default(global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    run(&WriteArgs { editor: false }, global)
}

fn preflight_terminal(global: &GlobalOptions) -> Result<(), AppError> {
    if global.json || global.quiet {
        return Err(AppError::new(
            "INVALID_INPUT",
            "cap write is interactive and cannot be combined with --json or --quiet",
            2,
        ));
    }
    if global.dry_run {
        return Err(AppError::new(
            "INVALID_INPUT",
            "--dry-run is supported only for quick capture (cap add)",
            2,
        ));
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(AppError::new(
            "INVALID_INPUT",
            "cap write requires an interactive terminal on stdin and stdout; use cap add --stdin for piped text",
            2,
        ));
    }
    Ok(())
}

fn validate_resolved(resolved: &ResolvedCapsule) -> Result<(), AppError> {
    if !resolved.settings_valid {
        return Err(AppError::new(
            "INVALID_CONFIG",
            resolved
                .settings_error
                .clone()
                .unwrap_or_else(|| "Capsule path settings are invalid".to_string()),
            2,
        ));
    }
    if !resolved.capabilities.supports_write() {
        let reason = resolved
            .capabilities
            .write_support_reasons
            .first()
            .cloned()
            .unwrap_or_else(|| "database schema does not support capture writes".to_string());
        return Err(AppError::new("UNSUPPORTED_SCHEMA", reason, 3));
    }
    if resolved.database_identity.is_none() {
        return Err(AppError::new(
            "DB_READ",
            "the active database has no stable filesystem identity",
            3,
        ));
    }
    Ok(())
}

fn effective_writer_preferences(
    resolved: &ResolvedCapsule,
    presentation: &EffectivePresentation,
) -> WriterPreferences {
    let mut writer = resolved.settings.writer.clone();
    if let Some(target) = presentation.writer_target_override {
        // Capsule's shared setting clamps to 100,000. The cap-local override
        // is separately validated by Preferences and then narrowed to the same
        // shared bound before it can block a save.
        writer.word_target = (target as usize).clamp(1, db::MAX_WORD_TARGET);
    }
    writer
}

fn target_display(
    target: &WriterPreferences,
    presentation: &EffectivePresentation,
) -> Option<usize> {
    (target.word_target_enabled || presentation.writer_target_override.is_some())
        .then_some(target.word_target)
}

fn acquire_lease(state: &StateStore, resolved: &ResolvedCapsule) -> Result<DraftLease, AppError> {
    let existing = draft::editable_draft(state).map_err(map_draft_error)?;
    if let Some(record) = existing {
        let request = record.request.as_ref().ok_or_else(|| {
            AppError::new(
                "RECOVERY_STORAGE",
                "editable writer draft is missing its request",
                5,
            )
        })?;
        if !draft::request_matches_resolved(request, resolved) {
            return Err(AppError::new(
                "DB_REPLACED",
                "the saved writer draft is bound to a different or replaced database; refusing to redirect it",
                4,
            ));
        }
        match choose_existing_draft(&record) {
            Ok(ResumeChoice::Resume) => {
                return DraftLease::from_record(state.clone(), record, resolved)
                    .map_err(map_draft_error)
            }
            Ok(ResumeChoice::Discard) => {
                let mut old = DraftLease::from_record(state.clone(), record, resolved)
                    .map_err(map_draft_error)?;
                old.discard().map_err(map_draft_error)?;
                old.release();
            }
            Ok(ResumeChoice::Quit) => {
                return Err(AppError::new(
                    "WRITER_EXIT",
                    "writer closed without changing the saved draft",
                    130,
                ));
            }
            Err(error) => return Err(error),
        }
    }

    let lease = DraftLease::new(state.clone(), resolved).map_err(map_draft_error)?;
    draft::ensure_uuid_unclaimed(lease.request()).map_err(map_draft_error)?;
    Ok(lease)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResumeChoice {
    Resume,
    Discard,
    Quit,
}

fn choose_existing_draft(
    record: &crate::recovery::RecoveryRecord,
) -> Result<ResumeChoice, AppError> {
    let request = record.request.as_ref().ok_or_else(|| {
        AppError::new(
            "RECOVERY_STORAGE",
            "editable writer draft is missing its request",
            5,
        )
    })?;
    let words = request.text.split_whitespace().count();
    let destination = cap_effects::sanitize_text(&request.database_path.to_string_lossy());
    println!(
        "Recoverable writer draft: {words} words, destination {destination}.\nResume [r], discard [d], or quit [q]?"
    );
    let mut line = String::new();
    loop {
        line.clear();
        io::stdin()
            .read_line(&mut line)
            .map_err(|error| AppError::new("WRITER_INPUT", error.to_string(), 1))?;
        match line.trim().to_ascii_lowercase().as_str() {
            "r" | "resume" => return Ok(ResumeChoice::Resume),
            "d" | "discard" => return Ok(ResumeChoice::Discard),
            "q" | "quit" | "" => return Ok(ResumeChoice::Quit),
            _ => println!("Choose resume [r], discard [d], or quit [q]."),
        }
    }
}

fn run_editor(
    lease: &mut DraftLease,
    resolved: &ResolvedCapsule,
    presentation: &EffectivePresentation,
    target: &WriterPreferences,
    global: &GlobalOptions,
) -> Result<CommandOutput, AppError> {
    if crate::cancellation::requested() {
        return Err(AppError::new(
            "CANCELLED",
            "writer interrupted before editor launch",
            130,
        ));
    }
    let initial = lease.request().text.clone();
    let edited = match editor::edit(presentation, &initial) {
        Ok(document) => document,
        Err(mut error) => {
            // A failed child may still have written a useful nonempty UTF-8
            // body. Persist that body while the lease is held before returning
            // the failure; this prevents a nonzero editor exit from losing
            // the only copy of the user's work.
            if let Some(recovered) = error.recovered_text().map(str::to_owned) {
                lease.request_mut().map_err(map_draft_error)?.text = recovered.clone();
                if let Err(persist_error) = lease.persist_text(&recovered) {
                    let mut app_error = map_draft_error(persist_error);
                    if let Some(path) = error.retained_path() {
                        app_error.detail.message.push_str(&format!(
                            " The recovered editor file was retained at {}.",
                            path.display()
                        ));
                    }
                    return Err(app_error);
                }
                if editor::remove_retained_file(&mut error) {
                    if let editor::EditorError::Failed { output_error, .. } = &mut error {
                        *output_error = Some(
                            "recovered editor output was saved as an editable draft".to_string(),
                        );
                    }
                }
            }
            return Err(editor_error(error));
        }
    };
    lease.request_mut().map_err(map_draft_error)?.text = edited.text().to_owned();
    // Persist before target/destination checks so a blocked save or a changed
    // database leaves the edited nonempty body recoverable as an editable
    // draft.
    let persisted = editor::persist_document(edited, |text| lease.persist_text(text));
    if let Err((error, edited)) = persisted {
        let mut app_error = map_draft_error(error);
        app_error.detail.message.push_str(&format!(
            " The editor file was retained at {}.",
            edited.path().display()
        ));
        return Err(app_error);
    }
    if crate::cancellation::requested() {
        return Err(AppError::new(
            "CANCELLED",
            "writer interrupted before capture; the edited draft remains recoverable",
            130,
        ));
    }
    validate_active_destination(lease.request(), global)?;
    validate_save(lease.request(), resolved, target)?;
    let request = lease.prepare_submission().map_err(map_draft_error)?;
    lease.release();
    add::capture_frozen_request(request, global)
}

fn run_interactive(
    lease: &mut DraftLease,
    resolved: &ResolvedCapsule,
    presentation: &EffectivePresentation,
    target: &WriterPreferences,
    capabilities: &cap_effects::TerminalCapabilities,
    global: &GlobalOptions,
) -> Result<CommandOutput, AppError> {
    let mut machine = WriterMachine::new(
        lease.request().text.clone(),
        capabilities.width as u16,
        terminal_height(capabilities),
    );
    let mut output = io::stdout().lock();
    let mut terminal_session = terminal::TerminalSession::new(&mut output)
        .map_err(|error| AppError::new("WRITER_TERMINAL", error.to_string(), 1))?;
    let mut last_edit = Instant::now();
    let mut last_ambient = Instant::now();
    let ambient_started = Instant::now();
    let mut notice: Option<String> = None;
    let loop_result: Result<InteractiveOutcome, AppError> = (|| {
        terminal::render_frame_with_notice(
            terminal_session.writer(),
            &machine,
            lease.request(),
            presentation,
            target_display(target, presentation),
            None,
            None,
        )
        .map_err(|error| AppError::new("WRITER_TERMINAL", error.to_string(), 1))?;

        loop {
            if crate::cancellation::requested() {
                break Ok(InteractiveOutcome::Interrupted);
            }
            if event::poll(Duration::from_millis(50))
                .map_err(|error| AppError::new("WRITER_INPUT", error.to_string(), 1))?
            {
                if let Some(input) = terminal::read_input_event()
                    .map_err(|error| AppError::new("WRITER_INPUT", error.to_string(), 1))?
                {
                    let before = machine.text().to_owned();
                    let action = match machine.apply(input) {
                        Ok(action) => action,
                        Err(error) => {
                            notice = Some(error.to_string());
                            WriterAction::Continue
                        }
                    };
                    if machine.text() != before {
                        last_edit = Instant::now();
                        notice = None;
                    }
                    match action {
                        WriterAction::Save => {
                            sync_machine_request(lease, &machine)?;
                            let request = lease.request().clone();
                            match validate_active_destination(&request, global)
                                .and_then(|_| validate_save(&request, resolved, target))
                            {
                                Ok(()) => break Ok(InteractiveOutcome::Save),
                                Err(error) => {
                                    notice = Some(error.detail.message);
                                }
                            }
                        }
                        WriterAction::Exit => break Ok(InteractiveOutcome::Exit),
                        WriterAction::Interrupt => break Ok(InteractiveOutcome::Interrupted),
                        WriterAction::Continue => {}
                    }
                    terminal::render_frame_with_notice(
                        terminal_session.writer(),
                        &machine,
                        lease.request(),
                        presentation,
                        target_display(target, presentation),
                        None,
                        notice.as_deref(),
                    )
                    .map_err(|error| AppError::new("WRITER_TERMINAL", error.to_string(), 1))?;
                }
            }

            if machine.dirty() && last_edit.elapsed() >= AUTOSAVE_IDLE {
                lease
                    .persist_text(machine.text())
                    .map_err(map_draft_error)?;
                machine.mark_persisted();
                notice = None;
            }

            let ambient_enabled = presentation.output.motion() == cap_effects::MotionMode::Full
                && presentation.output.color() != cap_effects::ColorMode::Plain
                && capabilities.is_tty
                && capabilities.ansi_support
                && machine.focused()
                && last_edit.elapsed() >= AUTOSAVE_IDLE;
            if ambient_enabled && last_ambient.elapsed() >= AMBIENT_FRAME {
                let phase = (ambient_started.elapsed().as_secs_f64() * 0.18) % 1.0;
                last_ambient = Instant::now();
                terminal::render_frame_with_notice(
                    terminal_session.writer(),
                    &machine,
                    lease.request(),
                    presentation,
                    target_display(target, presentation),
                    Some(phase),
                    notice.as_deref(),
                )
                .map_err(|error| AppError::new("WRITER_TERMINAL", error.to_string(), 1))?;
            }
        }
    })();

    // Restore terminal modes before any DB work or final receipt rendering.
    let restore_result = terminal_session.restore();
    drop(terminal_session);
    drop(output);

    let outcome = match loop_result {
        Ok(outcome) => outcome,
        Err(mut error) => {
            if let Err(persist_error) = lease.persist_text(machine.text()) {
                error.detail.message.push_str(&format!(
                    " Latest draft could not be retained: {persist_error}."
                ));
            }
            if let Err(restore_error) = restore_result {
                error
                    .detail
                    .message
                    .push_str(&format!(" Terminal restoration failed: {restore_error}."));
            }
            return Err(error);
        }
    };
    if let Err(error) = restore_result {
        let mut app_error = AppError::new("WRITER_TERMINAL", error.to_string(), 1);
        if let Err(persist_error) = lease.persist_text(machine.text()) {
            app_error.detail.message.push_str(&format!(
                " Latest draft could not be retained: {persist_error}."
            ));
        }
        return Err(app_error);
    }

    match outcome {
        InteractiveOutcome::Save => {
            sync_machine_request(lease, &machine)?;
            // The terminal is restored before this final commit boundary. A
            // database replacement or other validation failure must leave the
            // exact last edit in the frozen editable draft, so persist it
            // before revalidating the live destination.
            persist_then_validate(lease, machine.text(), |request| {
                validate_active_destination(request, global)?;
                validate_save(request, resolved, target)?;
                Ok(())
            })?;
            let request = lease.prepare_submission().map_err(map_draft_error)?;
            lease.release();
            add::capture_frozen_request(request, global)
        }
        InteractiveOutcome::Exit => {
            let persisted = lease
                .persist_text(machine.text())
                .map_err(map_draft_error)?;
            machine.mark_persisted();
            Ok(draft_output(lease, persisted, None))
        }
        InteractiveOutcome::Interrupted => {
            let persisted = lease
                .persist_text(machine.text())
                .map_err(map_draft_error)?;
            machine.mark_persisted();
            let mut error = AppError::new(
                "CANCELLED",
                if persisted {
                    "writer interrupted before capture; the draft was retained locally"
                } else {
                    "writer interrupted before capture; no nonempty draft was present"
                },
                130,
            );
            if !persisted {
                error.detail.retryable = false;
            }
            Err(error)
        }
    }
}

/// Persist the latest interactive body before running any final destination
/// or target validation. If the validator rejects a changed database, the
/// exact body is still present in the editable recovery record.
fn persist_then_validate<T, F>(
    lease: &mut DraftLease,
    text: &str,
    validate: F,
) -> Result<T, AppError>
where
    F: FnOnce(&CaptureRequest) -> Result<T, AppError>,
{
    lease.persist_text(text).map_err(map_draft_error)?;
    let request = lease.request().clone();
    validate(&request)
}

fn sync_machine_request(lease: &mut DraftLease, machine: &WriterMachine) -> Result<(), AppError> {
    lease.request_mut().map_err(map_draft_error)?.text = machine.text().to_owned();
    Ok(())
}

fn validate_save(
    request: &CaptureRequest,
    resolved: &ResolvedCapsule,
    target: &WriterPreferences,
) -> Result<(), AppError> {
    if request.text.trim().is_empty() {
        return Err(AppError::new(
            "INVALID_INPUT",
            "writer text is required before saving",
            2,
        ));
    }
    if target.blocks_save(request.text.split_whitespace().count()) {
        return Err(AppError::new(
            "WRITER_TARGET",
            format!(
                "Gauntlet target is {} words; add {} more before saving",
                target.word_target,
                target
                    .word_target
                    .saturating_sub(request.text.split_whitespace().count())
            ),
            2,
        ));
    }
    if !draft::request_matches_resolved(request, resolved) {
        return Err(AppError::new(
            "DB_REPLACED",
            "the active Capsule database changed while this draft was open; refusing to redirect it",
            4,
        ));
    }
    Ok(())
}

fn validate_active_destination(
    request: &CaptureRequest,
    global: &GlobalOptions,
) -> Result<ResolvedCapsule, AppError> {
    let active = query::resolve(global).map_err(|error| AppError::new("DB_READ", error, 3))?;
    if !draft::request_matches_resolved(request, &active) {
        return Err(AppError::new(
            "DB_REPLACED",
            "the active Capsule database changed while this draft was open; refusing to redirect it",
            4,
        ));
    }
    Ok(active)
}

fn draft_output(lease: &DraftLease, persisted: bool, warning: Option<String>) -> CommandOutput {
    let request = lease.request();
    let words = request.text.split_whitespace().count();
    let destination = db::path_to_string(&request.database_path);
    let data = json!({
        "saveState": "draft",
        "draftPersisted": persisted,
        "captureId": request.capture_id,
        "reservedUuid": request.reserved_uuid,
        "wordCount": words,
        "databasePath": destination,
        "databaseIdentity": request.database_identity,
        "writerDraft": true,
    });
    let mut output = CommandOutput::new(
        data,
        if persisted {
            format!(
                "Draft saved locally ({words} words). Resume with `cap write`; destination is {destination}."
            )
        } else {
            "Writer closed without a nonempty draft; nothing was saved locally.".to_string()
        },
    );
    output.warnings = warning.into_iter().collect();
    output
}

fn terminal_height(_capabilities: &cap_effects::TerminalCapabilities) -> u16 {
    crossterm::terminal::size()
        .map(|(_, rows)| rows)
        .unwrap_or_else(|_| {
            // `TerminalCapabilities` intentionally records only the detected
            // width; use a conservative standard height when the platform
            // cannot answer the size query rather than treating width as rows.
            24
        })
        .max(5)
}

fn editor_error(error: editor::EditorError) -> AppError {
    let (code, exit) = match error {
        editor::EditorError::Unavailable(_) => ("EDITOR_UNAVAILABLE", 2),
        editor::EditorError::Launch(_) | editor::EditorError::Failed { .. } => ("EDITOR_FAILED", 3),
        editor::EditorError::Io { .. } => ("EDITOR_IO", 1),
        editor::EditorError::InvalidOutput { .. } => ("EDITOR_OUTPUT_INVALID", 2),
    };
    AppError::new(code, error.to_string(), exit)
}

fn map_draft_error(error: DraftError) -> AppError {
    match error {
        DraftError::State(StateError::Busy(message)) => AppError::new("CAPTURE_BUSY", message, 4),
        DraftError::State(error) => AppError::new("RECOVERY_STORAGE", error.to_string(), 5),
        DraftError::Database(message) => AppError::new("DB_READ", message, 3),
        DraftError::Invalid(message) if message.contains("different or replaced") => {
            AppError::new("DB_REPLACED", message, 4)
        }
        DraftError::Invalid(message) => AppError::new("INVALID_INPUT", message, 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capsule_core::db::{CapabilityReport, PathSource, SafePathSettings, SchemaCapabilities};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn resolved() -> ResolvedCapsule {
        ResolvedCapsule {
            database_path: PathBuf::from(r"C:\lab\capsule.db"),
            config_path: None,
            backup_directory: PathBuf::from(r"C:\lab\backups"),
            database_source: PathSource::Explicit,
            config_source: PathSource::Fallback,
            config_valid: true,
            config_error: None,
            backup_source: PathSource::Fallback,
            settings_path: None,
            settings_source: PathSource::Fallback,
            settings_valid: true,
            settings_error: None,
            database_identity: None,
            settings: SafePathSettings::default(),
            diagnostics: Vec::new(),
            capabilities: CapabilityReport {
                database_path: Some(r"C:\lab\capsule.db".to_string()),
                database_exists: true,
                readable: true,
                schema: SchemaCapabilities {
                    table_names: vec![],
                    required_columns: Default::default(),
                    optional_tables: Default::default(),
                    has_entries_table: true,
                    has_tags_table: true,
                    has_fts_table: false,
                    supports_read: true,
                    supports_write: true,
                    missing_required_columns: vec![],
                },
                write_support_reasons: vec![],
                warnings: vec![],
            },
        }
    }

    #[test]
    fn gauntlet_blocks_only_when_both_shared_flags_are_enabled() {
        let mut settings = WriterPreferences {
            word_target_enabled: true,
            gauntlet_mode_enabled: true,
            word_target: 3,
        };
        assert!(settings.blocks_save(2));
        assert!(!settings.blocks_save(3));
        settings.gauntlet_mode_enabled = false;
        assert!(!settings.blocks_save(0));
    }

    #[test]
    fn preflight_rejects_machine_and_non_dry_writer_modes() {
        let global = GlobalOptions {
            json: true,
            ..GlobalOptions::default()
        };
        assert!(preflight_terminal(&global).is_err());
        let global = GlobalOptions {
            dry_run: true,
            ..GlobalOptions::default()
        };
        assert!(preflight_terminal(&global).is_err());
    }

    #[test]
    fn save_validation_rejects_empty_text_and_target_shortfall() {
        let resolved = resolved();
        let writer = WriterPreferences {
            word_target_enabled: true,
            gauntlet_mode_enabled: true,
            word_target: 2,
        };
        let request = CaptureRequest::new(
            "one",
            "cap_writer_test",
            "entry_writer_test",
            resolved.database_path.clone(),
            chrono::Local::now().fixed_offset(),
        );
        assert!(validate_save(&request, &resolved, &writer).is_err());
    }

    #[test]
    fn latest_body_is_persisted_before_destination_validation() {
        let directory = tempdir().unwrap();
        let store = StateStore::at_dir(directory.path());
        let resolved = resolved();
        let mut lease = DraftLease::new(store.clone(), &resolved).unwrap();
        let capture_id = lease.capture_id().to_owned();
        let error = persist_then_validate(&mut lease, "last edit", |request| {
            let saved = store
                .read_pending(&request.capture_id)
                .unwrap()
                .expect("body must be durable before validation");
            assert_eq!(saved.request.as_ref().unwrap().text, "last edit");
            Err::<(), _>(AppError::new(
                "DB_REPLACED",
                "the active database changed",
                4,
            ))
        })
        .expect_err("simulated destination replacement must reject the save");
        assert_eq!(error.detail.code, "DB_REPLACED");
        assert_eq!(
            store
                .read_pending(&capture_id)
                .unwrap()
                .unwrap()
                .request
                .unwrap()
                .text,
            "last edit"
        );
    }
}
