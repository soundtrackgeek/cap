//! Durable writer-draft handling on top of cap's existing recovery records.
//!
//! There is deliberately no writer-specific body file. A draft is a normal
//! pending [`RecoveryRecord`] with `writer_draft.kind = "editable"`; the
//! record's request keeps the frozen timestamp, UUID, database identity and
//! backup policy used by the eventual capture.

use std::{
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use capsule_core::{
    contracts::{BackupPolicy, CaptureRequest},
    db::ResolvedCapsule,
    entries, identity,
};
use chrono::Local;

use crate::recovery::{
    self, RecordLock, RecoveryRecord, RecoveryState, StateError, StateStore, WriterDraftMetadata,
};

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum DraftError {
    State(StateError),
    Invalid(String),
    Database(String),
}

impl std::fmt::Display for DraftError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::State(error) => error.fmt(formatter),
            Self::Invalid(message) | Self::Database(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for DraftError {}

impl From<StateError> for DraftError {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}

/// A writer's process-scoped lease. The lock is held through every editable
/// persistence update and until the marker is cleared before submission.
pub struct DraftLease {
    store: StateStore,
    lock: Option<RecordLock>,
    record: Option<RecoveryRecord>,
    request: CaptureRequest,
    prepared: bool,
}

impl std::fmt::Debug for DraftLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DraftLease")
            .field("capture_id", &self.request.capture_id)
            .field("has_record", &self.record.is_some())
            .field("prepared", &self.prepared)
            .finish_non_exhaustive()
    }
}

impl DraftLease {
    pub fn new(store: StateStore, resolved: &ResolvedCapsule) -> Result<Self, DraftError> {
        let request = new_request(resolved);
        let lock = store.lock(&request.capture_id)?;
        Ok(Self {
            store,
            lock: Some(lock),
            record: None,
            request,
            prepared: false,
        })
    }

    pub fn from_record(
        store: StateStore,
        record: RecoveryRecord,
        resolved: &ResolvedCapsule,
    ) -> Result<Self, DraftError> {
        if !record.is_editable_writer_draft() {
            return Err(DraftError::Invalid(
                "only an editable pending writer draft can be resumed".to_string(),
            ));
        }
        let request = record.request.clone().ok_or_else(|| {
            DraftError::Invalid("editable writer draft is missing its request".to_string())
        })?;
        let request_fingerprint =
            recovery::request_fingerprint(&request).map_err(DraftError::Invalid)?;
        if !record_matches_request(&record, &request, &request_fingerprint) {
            return Err(DraftError::Invalid(
                "writer draft metadata does not match its frozen request".to_string(),
            ));
        }
        if !request_matches_resolved(&request, resolved) {
            return Err(DraftError::Invalid(
                "the writer draft is bound to a different or replaced database".to_string(),
            ));
        }
        identity::validate_capture_id(&request.capture_id)
            .map_err(|error| DraftError::Invalid(error.to_string()))?;
        identity::validate_reserved_uuid(&request.reserved_uuid)
            .map_err(|error| DraftError::Invalid(error.to_string()))?;

        let lock = store.lock(&request.capture_id)?;
        // The prompt is intentionally outside this lease. Re-read after
        // acquiring it so a concurrent recover operation cannot replace the
        // record between listing and resume.
        let current = store.read_pending(&request.capture_id)?.ok_or_else(|| {
            DraftError::Invalid("writer draft disappeared before resume".to_string())
        })?;
        if !current.is_editable_writer_draft() {
            return Err(DraftError::Invalid(
                "writer draft is no longer editable; inspect it with cap status/recover"
                    .to_string(),
            ));
        }
        let current_request = current.request.clone().ok_or_else(|| {
            DraftError::Invalid("editable writer draft is missing its request".to_string())
        })?;
        if !request_matches_resolved(&current_request, resolved)
            || current_request.capture_id != request.capture_id
            || current_request.reserved_uuid != request.reserved_uuid
            || current_request.created_at != request.created_at
            || current_request.database_path != request.database_path
            || current_request.database_identity != request.database_identity
            || current_request.backup_policy != request.backup_policy
            || recovery::request_fingerprint(&current_request).map_err(DraftError::Invalid)?
                != current.request_fingerprint
            || current_request.backup_policy != Some(expected_backup_policy(resolved))
            || !record_matches_request(&current, &current_request, &current.request_fingerprint)
        {
            return Err(DraftError::Invalid(
                "writer draft changed while waiting to resume".to_string(),
            ));
        }
        ensure_uuid_unclaimed(&current_request)?;
        Ok(Self {
            store,
            lock: Some(lock),
            record: Some(current),
            request: current_request,
            prepared: false,
        })
    }

    pub fn store(&self) -> &StateStore {
        &self.store
    }

    pub fn request(&self) -> &CaptureRequest {
        &self.request
    }

    pub fn request_mut(&mut self) -> Result<&mut CaptureRequest, DraftError> {
        self.ensure_held()?;
        Ok(&mut self.request)
    }

    pub fn record(&self) -> Option<&RecoveryRecord> {
        self.record.as_ref()
    }

    pub fn has_persisted_record(&self) -> bool {
        self.record.is_some()
    }

    pub fn capture_id(&self) -> &str {
        &self.request.capture_id
    }

    /// Persist the current body as an editable draft. Empty/whitespace-only
    /// text removes an existing local draft and never writes an invalid
    /// CaptureRequest fingerprint.
    pub fn persist_text(&mut self, text: &str) -> Result<bool, DraftError> {
        self.ensure_held()?;
        self.request.text = normalize_newlines(text);
        if self.request.text.trim().is_empty() {
            if self.record.take().is_some() {
                self.store.remove_pending(&self.request.capture_id)?;
            }
            return Ok(false);
        }
        let fingerprint =
            recovery::request_fingerprint(&self.request).map_err(DraftError::Invalid)?;
        let record = if let Some(record) = self.record.as_mut() {
            record.request = Some(self.request.clone());
            record.request_fingerprint = fingerprint;
            record.word_count = self.request.text.split_whitespace().count();
            record.state = RecoveryState::Pending;
            record.receipt = None;
            record.last_error = None;
            record.writer_draft = Some(WriterDraftMetadata::editable());
            record.updated_at = chrono::Utc::now();
            record.clone()
        } else {
            let mut record = RecoveryRecord::pending(self.request.clone(), fingerprint, false);
            record.writer_draft = Some(WriterDraftMetadata::editable());
            record
        };
        self.store.write_pending(&record)?;
        self.record = Some(record);
        Ok(true)
    }

    /// Mark a nonempty body ready for the shared capture helper. This write is
    /// the immutable transition boundary: the editable marker is cleared while
    /// the per-capture lock is still held, and the caller must then release the
    /// lease before invoking `capture_frozen_request`.
    pub fn prepare_submission(&mut self) -> Result<CaptureRequest, DraftError> {
        self.ensure_held()?;
        if self.request.text.trim().is_empty() {
            return Err(DraftError::Invalid("writer text is required".to_string()));
        }
        self.persist_text(&self.request.text.clone())?;
        let record = self.record.as_mut().ok_or_else(|| {
            DraftError::Invalid("writer draft could not be persisted".to_string())
        })?;
        record.writer_draft = None;
        record.updated_at = chrono::Utc::now();
        self.store.write_pending(record)?;
        self.prepared = true;
        Ok(self.request.clone())
    }

    pub fn discard(&mut self) -> Result<(), DraftError> {
        self.ensure_held()?;
        self.store.remove_pending(&self.request.capture_id)?;
        self.record = None;
        Ok(())
    }

    pub fn release(&mut self) {
        self.lock.take();
    }

    fn ensure_held(&self) -> Result<(), DraftError> {
        if self.lock.is_none() {
            return Err(DraftError::Invalid(
                "writer draft lease has already been released".to_string(),
            ));
        }
        if self.prepared {
            return Err(DraftError::Invalid(
                "writer draft lease has already crossed the submission boundary".to_string(),
            ));
        }
        Ok(())
    }
}

impl Drop for DraftLease {
    fn drop(&mut self) {
        self.lock.take();
    }
}

pub fn editable_draft(state: &StateStore) -> Result<Option<RecoveryRecord>, DraftError> {
    let mut candidates = Vec::new();
    for record in state.list_records()? {
        if record.writer_draft.is_some() {
            if !record.is_editable_writer_draft() {
                return Err(DraftError::Invalid(
                    "a writer draft is no longer editable; use cap status/recover before continuing"
                        .to_string(),
                ));
            }
            candidates.push(record);
        }
    }
    candidates.sort_by_key(|record| record.updated_at);
    Ok(candidates.pop())
}

pub fn request_matches_resolved(request: &CaptureRequest, resolved: &ResolvedCapsule) -> bool {
    let left = comparable_path(&request.database_path);
    let right = comparable_path(&resolved.database_path);
    left == right
        && matches!(
            (&request.database_identity, &resolved.database_identity),
            (Some(left), Some(right)) if left.same_file(right)
        )
}

pub fn ensure_uuid_unclaimed(request: &CaptureRequest) -> Result<(), DraftError> {
    let entries = entries::list_entries_by_uuids_read_only_for_database(
        &request.database_path,
        std::slice::from_ref(&request.reserved_uuid),
    )
    .map_err(|error| DraftError::Database(error.to_string()))?;
    if entries.is_empty() {
        Ok(())
    } else {
        Err(DraftError::Invalid(format!(
            "writer draft UUID {} already exists; it cannot be edited",
            request.reserved_uuid
        )))
    }
}

pub fn new_request(resolved: &ResolvedCapsule) -> CaptureRequest {
    let now = Local::now().fixed_offset();
    let id_seed = unix_nanos();
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let capture_id = format!("cap_writer_{id_seed:x}_{:x}_{counter}", std::process::id());
    let reserved_uuid = format!(
        "entry_writer_{id_seed:x}_{:x}_{counter}",
        std::process::id()
    );
    let policy = expected_backup_policy(resolved);
    let mut request = CaptureRequest::new(
        String::new(),
        capture_id,
        reserved_uuid,
        resolved.database_path.clone(),
        now,
    );
    request.database_identity = resolved.database_identity.clone();
    request.backup_policy = Some(policy);
    request
}

fn expected_backup_policy(resolved: &ResolvedCapsule) -> BackupPolicy {
    BackupPolicy::new(
        resolved.backup_directory.clone(),
        resolved
            .settings
            .backup_retention_count
            .unwrap_or(capsule_core::db::DEFAULT_BACKUP_RETENTION_COUNT),
    )
}

fn record_matches_request(
    record: &RecoveryRecord,
    request: &CaptureRequest,
    request_fingerprint: &str,
) -> bool {
    record.capture_id == request.capture_id
        && record.reserved_uuid == request.reserved_uuid
        && record.database_path == request.database_path
        && record.database_identity == request.database_identity
        && record.entry_created_at == request.created_at
        && record.request_fingerprint == request_fingerprint
}

fn comparable_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
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
    use std::path::PathBuf;

    fn resolved() -> ResolvedCapsule {
        let path = PathBuf::from("C:\\lab\\capsule.db");
        ResolvedCapsule {
            database_path: path.clone(),
            config_path: None,
            backup_directory: PathBuf::from("C:\\lab\\backups"),
            database_source: capsule_core::db::PathSource::Explicit,
            config_source: capsule_core::db::PathSource::Fallback,
            config_valid: true,
            config_error: None,
            backup_source: capsule_core::db::PathSource::Fallback,
            settings_path: None,
            settings_source: capsule_core::db::PathSource::Fallback,
            settings_valid: true,
            settings_error: None,
            database_identity: None,
            settings: Default::default(),
            diagnostics: Vec::new(),
            capabilities: capsule_core::db::CapabilityReport {
                database_path: Some(path.to_string_lossy().into_owned()),
                database_exists: true,
                readable: true,
                schema: capsule_core::db::SchemaCapabilities {
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
    fn generated_request_freezes_ids_time_and_backup_policy() {
        let path = PathBuf::from("C:\\lab\\capsule.db");
        let resolved = ResolvedCapsule {
            database_path: path.clone(),
            config_path: None,
            backup_directory: PathBuf::from("C:\\lab\\backups"),
            database_source: capsule_core::db::PathSource::Explicit,
            config_source: capsule_core::db::PathSource::Fallback,
            config_valid: true,
            config_error: None,
            backup_source: capsule_core::db::PathSource::Fallback,
            settings_path: None,
            settings_source: capsule_core::db::PathSource::Fallback,
            settings_valid: true,
            settings_error: None,
            database_identity: None,
            settings: Default::default(),
            diagnostics: Vec::new(),
            capabilities: capsule_core::db::CapabilityReport {
                database_path: Some(path.to_string_lossy().into_owned()),
                database_exists: true,
                readable: true,
                schema: capsule_core::db::SchemaCapabilities {
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
        };
        let request = new_request(&resolved);
        assert!(request.capture_id.starts_with("cap_writer_"));
        assert!(request.reserved_uuid.starts_with("entry_writer_"));
        assert_eq!(
            request.backup_policy.unwrap().directory,
            PathBuf::from("C:\\lab\\backups")
        );
    }

    #[test]
    fn submission_boundary_rejects_further_mutation_even_while_lock_is_held() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::at_dir(directory.path());
        let mut lease = DraftLease::new(store, &resolved()).unwrap();
        lease.persist_text("ready to submit").unwrap();
        lease.prepare_submission().unwrap();
        assert!(lease.request_mut().is_err());
        assert!(lease.persist_text("changed after submit").is_err());
        assert!(lease.discard().is_err());
        lease.release();
    }
}
