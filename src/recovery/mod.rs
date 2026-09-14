//! Cap-local capture/recovery state.
//!
//! The Capsule database remains the only journal.  This module stores only
//! enough request/receipt metadata to reconcile a process that stopped around
//! the commit boundary.  Files are written through a flushed temporary file
//! and one same-directory rename; a per-capture sidecar prevents two
//! processes from updating the same record concurrently.

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use capsule_core::{
    contracts::{CaptureRequest, CommitReceipt, ContextResult},
    db::FileIdentity,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const STATE_VERSION: u32 = 1;
const RECEIPT_RETENTION_DAYS: i64 = 30;
const STATE_HOME_ENV: &str = "CAP_CONFIG_HOME";
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryState {
    Pending,
    Unknown,
    Committed,
}

/// Metadata that marks a pending request as an editable interactive-writer
/// draft.  The request body, identity, timestamp and reserved UUID remain in
/// the normal [`RecoveryRecord`]; this marker only tells the writer that it is
/// safe to resume editing.  A submitted request clears the marker before the
/// capture helper is invoked, so a saved/unknown/committed request can never be
/// reopened as mutable text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WriterDraftMetadata {
    pub version: u32,
    pub kind: String,
}

impl WriterDraftMetadata {
    pub const VERSION: u32 = 1;
    pub const KIND: &'static str = "editable";

    pub fn editable() -> Self {
        Self {
            version: Self::VERSION,
            kind: Self::KIND.to_owned(),
        }
    }

    pub fn is_editable(&self) -> bool {
        self.version == Self::VERSION && self.kind == Self::KIND
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryRecord {
    pub version: u32,
    pub capture_id: String,
    pub reserved_uuid: String,
    pub request: Option<CaptureRequest>,
    pub request_fingerprint: String,
    pub database_path: PathBuf,
    pub database_identity: Option<FileIdentity>,
    pub entry_created_at: chrono::DateTime<chrono::FixedOffset>,
    pub word_count: usize,
    pub state: RecoveryState,
    pub receipt: Option<CommitReceipt>,
    pub context: Option<ContextResult>,
    pub explicit_capture_id: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    /// Present only for an unsent interactive-writer draft.  This remains in
    /// the existing pending record rather than introducing a second journal
    /// or body format.
    #[serde(default)]
    pub writer_draft: Option<WriterDraftMetadata>,
}

impl RecoveryRecord {
    pub fn pending(
        request: CaptureRequest,
        request_fingerprint: String,
        explicit_capture_id: bool,
    ) -> Self {
        let now = Utc::now();
        Self {
            version: STATE_VERSION,
            capture_id: request.capture_id.clone(),
            reserved_uuid: request.reserved_uuid.clone(),
            database_path: request.database_path.clone(),
            database_identity: request.database_identity.clone(),
            entry_created_at: request.created_at,
            word_count: request.text.split_whitespace().count(),
            request: Some(request),
            request_fingerprint,
            state: RecoveryState::Pending,
            receipt: None,
            context: None,
            explicit_capture_id,
            created_at: now,
            updated_at: now,
            expires_at: None,
            last_error: None,
            writer_draft: None,
        }
    }

    pub fn is_editable_writer_draft(&self) -> bool {
        self.state == RecoveryState::Pending
            && self
                .writer_draft
                .as_ref()
                .is_some_and(WriterDraftMetadata::is_editable)
    }

    pub fn mark_unknown(&mut self, error: impl Into<String>) {
        self.state = RecoveryState::Unknown;
        self.last_error = Some(error.into());
        self.updated_at = Utc::now();
    }

    /// Replace the request with its metadata-free receipt once the commit is
    /// confirmed.  The normalized entry body therefore is not retained in a
    /// receipt file.
    pub fn mark_committed(&mut self, receipt: CommitReceipt) {
        self.state = RecoveryState::Committed;
        self.reserved_uuid = receipt.uuid.clone();
        self.receipt = Some(receipt);
        self.request = None;
        self.writer_draft = None;
        self.last_error = None;
        self.updated_at = Utc::now();
        self.expires_at = Some(self.updated_at + chrono::Duration::days(RECEIPT_RETENTION_DAYS));
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|expires| expires <= now)
    }

    pub fn display_state(&self) -> &'static str {
        match self.state {
            RecoveryState::Pending => "pending",
            RecoveryState::Unknown => "unknown",
            RecoveryState::Committed => "committed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureBinding {
    pub version: u32,
    pub capture_id: String,
    pub reserved_uuid: String,
    pub database_path: PathBuf,
    pub database_identity: Option<FileIdentity>,
    pub request_fingerprint: String,
    pub explicit_capture_id: bool,
    pub committed_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// A compact explicit-ID tombstone left when an unresolved/pending request is
/// deliberately discarded.  It contains no commit timestamp or receipt and
/// therefore only prevents unsafe replay; it never claims that an entry was
/// saved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTombstone {
    pub version: u32,
    pub capture_id: String,
    pub reserved_uuid: String,
    pub database_path: PathBuf,
    pub database_identity: Option<FileIdentity>,
    pub request_fingerprint: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct StateStore {
    root: PathBuf,
}

impl StateStore {
    pub fn from_env() -> Result<Self, String> {
        let root = if let Some(value) = std::env::var_os(STATE_HOME_ENV) {
            let value = value.to_string_lossy().trim().to_owned();
            if value.is_empty() {
                return Err("CAP_CONFIG_HOME is set but empty".to_string());
            }
            PathBuf::from(value)
        } else {
            #[cfg(windows)]
            {
                let local = std::env::var_os("LOCALAPPDATA")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .ok_or_else(|| {
                        "Unable to determine LOCALAPPDATA; set CAP_CONFIG_HOME".to_string()
                    })?;
                PathBuf::from(local).join("Capsule").join("cap")
            }
            #[cfg(not(windows))]
            {
                let home = std::env::var_os("XDG_STATE_HOME")
                    .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
                    .or_else(|| std::env::var_os("HOME"))
                    .ok_or_else(|| {
                        "Unable to determine a cap state directory; set CAP_CONFIG_HOME".to_string()
                    })?;
                let home = PathBuf::from(home);
                if std::env::var_os("XDG_STATE_HOME").is_some() {
                    home.join("Capsule").join("cap")
                } else if std::env::var_os("XDG_CONFIG_HOME").is_some() {
                    home.join("Capsule").join("cap")
                } else {
                    home.join(".local")
                        .join("state")
                        .join("Capsule")
                        .join("cap")
                }
            }
        };
        Ok(Self::at_dir(root))
    }

    pub fn at_dir(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn pending_dir(&self) -> PathBuf {
        self.root.join("pending")
    }

    pub fn receipts_dir(&self) -> PathBuf {
        self.root.join("receipts")
    }

    pub fn bindings_dir(&self) -> PathBuf {
        self.root.join("bindings")
    }

    pub fn tombstones_dir(&self) -> PathBuf {
        self.root.join("tombstones")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn pending_path(&self, capture_id: &str) -> PathBuf {
        self.pending_dir()
            .join(format!("{}.json", safe_key(capture_id)))
    }

    pub fn receipt_path(&self, capture_id: &str) -> PathBuf {
        self.receipts_dir()
            .join(format!("{}.json", safe_key(capture_id)))
    }

    pub fn binding_path(&self, capture_id: &str) -> PathBuf {
        self.bindings_dir()
            .join(format!("{}.json", safe_key(capture_id)))
    }

    pub fn tombstone_path(&self, capture_id: &str) -> PathBuf {
        self.tombstones_dir()
            .join(format!("{}.json", safe_key(capture_id)))
    }

    /// Acquire a stable sidecar lock.  `create_new` means a second process
    /// never truncates or unlinks the first process's lock file.
    pub fn lock(&self, capture_id: &str) -> Result<RecordLock, StateError> {
        let path = self
            .pending_dir()
            .join(format!("{}.lock", safe_key(capture_id)));
        fs::create_dir_all(self.pending_dir()).map_err(StateError::Io)?;
        // The sidecar is persistent. OS-level advisory locking, rather than
        // marker-file deletion, is what records ownership and automatically
        // releases it if a process is killed.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(StateError::Io)?;
        match file.try_lock() {
            Ok(()) => Ok(RecordLock { file: Some(file) }),
            Err(std::fs::TryLockError::WouldBlock) => Err(StateError::Busy(format!(
                "capture {capture_id} is being handled by another process"
            ))),
            Err(std::fs::TryLockError::Error(error)) => Err(StateError::Io(error)),
        }
    }

    pub fn read_pending(&self, capture_id: &str) -> Result<Option<RecoveryRecord>, StateError> {
        self.read_record(&self.pending_path(capture_id))
    }

    pub fn read_receipt(&self, capture_id: &str) -> Result<Option<RecoveryRecord>, StateError> {
        let path = self.receipt_path(capture_id);
        let value = self.read_record(&path)?;
        if let Some(record) = value.as_ref() {
            if record.is_expired(Utc::now()) {
                // Expiry is only meaningful when the durable explicit-ID
                // binding exists.  Do not remove the receipt here: readers
                // may run concurrently with a writer, and unlinking without
                // the per-record lock can race a commit.  A receipt whose
                // binding write failed remains authoritative and idempotent.
                let binding = self.read_binding(capture_id)?;
                if binding.is_some() {
                    return Ok(None);
                }
            }
        }
        Ok(value)
    }

    /// Remove an expired receipt only while holding its per-record lock and
    /// only after a durable explicit-ID binding has been observed.  This is
    /// intentionally separate from `read_receipt`, which is a read-only path.
    pub fn prune_expired_receipt(&self, capture_id: &str) -> Result<bool, StateError> {
        let _lock = self.lock(capture_id)?;
        let path = self.receipt_path(capture_id);
        let Some(record) = self.read_record(&path)? else {
            return Ok(false);
        };
        if !record.is_expired(Utc::now()) {
            return Ok(false);
        }
        if self.read_binding(capture_id)?.is_none() {
            return Ok(false);
        }
        self.remove_receipt(capture_id)?;
        Ok(true)
    }

    pub fn read_binding(&self, capture_id: &str) -> Result<Option<CaptureBinding>, StateError> {
        let path = self.binding_path(capture_id);
        let value: Option<CaptureBinding> = read_json(&path)?;
        Ok(value)
    }

    pub fn read_tombstone(&self, capture_id: &str) -> Result<Option<CaptureTombstone>, StateError> {
        let path = self.tombstone_path(capture_id);
        let value: Option<CaptureTombstone> = read_json(&path)?;
        if let Some(tombstone) = value.as_ref() {
            if tombstone
                .expires_at
                .is_some_and(|expires| expires <= Utc::now())
            {
                return Ok(None);
            }
        }
        Ok(value)
    }

    pub fn write_pending(&self, record: &RecoveryRecord) -> Result<(), StateError> {
        self.write_json(&self.pending_path(&record.capture_id), record)
    }

    pub fn write_receipt(&self, record: &RecoveryRecord) -> Result<(), StateError> {
        self.write_json(&self.receipt_path(&record.capture_id), record)
    }

    pub fn write_binding(&self, record: &RecoveryRecord) -> Result<(), StateError> {
        if !record.explicit_capture_id || record.state != RecoveryState::Committed {
            return Ok(());
        }
        if record.receipt.is_none() {
            return Ok(());
        }
        let binding = CaptureBinding {
            version: STATE_VERSION,
            capture_id: record.capture_id.clone(),
            reserved_uuid: record.reserved_uuid.clone(),
            database_path: record.database_path.clone(),
            database_identity: record.database_identity.clone(),
            request_fingerprint: record.request_fingerprint.clone(),
            explicit_capture_id: true,
            committed_at: record.updated_at,
            expires_at: record.expires_at,
        };
        self.write_json(&self.binding_path(&record.capture_id), &binding)
    }

    pub fn write_tombstone(&self, record: &RecoveryRecord) -> Result<(), StateError> {
        if !record.explicit_capture_id || record.state == RecoveryState::Committed {
            return Ok(());
        }
        let tombstone = CaptureTombstone {
            version: STATE_VERSION,
            capture_id: record.capture_id.clone(),
            reserved_uuid: record.reserved_uuid.clone(),
            database_path: record.database_path.clone(),
            database_identity: record.database_identity.clone(),
            request_fingerprint: record.request_fingerprint.clone(),
            created_at: Utc::now(),
            expires_at: Some(Utc::now() + chrono::Duration::days(RECEIPT_RETENTION_DAYS)),
        };
        self.write_json(&self.tombstone_path(&record.capture_id), &tombstone)
    }

    pub fn remove_pending(&self, capture_id: &str) -> Result<(), StateError> {
        match fs::remove_file(self.pending_path(capture_id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(StateError::Io(error)),
        }
    }

    pub fn remove_receipt(&self, capture_id: &str) -> Result<(), StateError> {
        match fs::remove_file(self.receipt_path(capture_id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(StateError::Io(error)),
        }
    }

    pub fn list_records(&self) -> Result<Vec<RecoveryRecord>, StateError> {
        let mut records = BTreeMap::<String, RecoveryRecord>::new();
        for directory in [self.pending_dir(), self.receipts_dir()] {
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(StateError::Io(error)),
            };
            for entry in entries {
                let entry = entry.map_err(StateError::Io)?;
                if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                if let Some(record) = read_json::<RecoveryRecord>(&entry.path())? {
                    if record.state == RecoveryState::Committed && record.is_expired(Utc::now()) {
                        // Housekeeping is serialized and guarded by the
                        // durable binding.  If another process owns the
                        // record, leave it visible for that process to finish
                        // and do not fail the whole recovery listing.
                        match self.prune_expired_receipt(&record.capture_id) {
                            Ok(true) => continue,
                            Ok(false) | Err(StateError::Busy(_)) => {
                                records.insert(record.capture_id.clone(), record);
                            }
                            Err(error) => return Err(error),
                        }
                    } else {
                        records.insert(record.capture_id.clone(), record);
                    }
                }
            }
        }
        let mut records = records.into_values().collect::<Vec<_>>();
        records.sort_by_key(|record| record.updated_at);
        Ok(records)
    }

    fn read_record(&self, path: &Path) -> Result<Option<RecoveryRecord>, StateError> {
        read_json(path)
    }

    fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<(), StateError> {
        #[cfg(feature = "test-hooks")]
        if let Some(kind) = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .and_then(|directory| match directory {
                "pending" => Some("pending"),
                "receipts" => Some("receipt"),
                "bindings" => Some("binding"),
                "tombstones" => Some("tombstone"),
                _ => None,
            })
        {
            if crate::test_hooks::storage_failure(kind, path) {
                return Err(StateError::Io(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("test hook blocked {kind} persistence"),
                )));
            }
        }
        let parent = path
            .parent()
            .ok_or_else(|| StateError::Invalid("state path has no parent".to_string()))?;
        fs::create_dir_all(parent).map_err(StateError::Io)?;
        let bytes = serde_json::to_vec_pretty(value).map_err(StateError::Json)?;
        atomic_write(path, &bytes).map_err(StateError::Io)
    }
}

pub struct RecordLock {
    file: Option<File>,
}

impl Drop for RecordLock {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            // Keep the sidecar itself. A persistent path avoids races with a
            // process opening a newly-created lock between operations.
            drop(file);
        }
    }
}

#[derive(Debug)]
pub enum StateError {
    Io(io::Error),
    Json(serde_json::Error),
    Busy(String),
    Invalid(String),
}

impl std::fmt::Display for StateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "recovery storage failed: {error}"),
            Self::Json(error) => write!(formatter, "recovery record is invalid JSON: {error}"),
            Self::Busy(message) | Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for StateError {}

impl From<io::Error> for StateError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn safe_key(value: &str) -> String {
    let mut key = String::with_capacity(value.len().saturating_mul(2));
    for byte in value.as_bytes() {
        use std::fmt::Write as _;
        let _ = write!(key, "{byte:02x}");
    }
    if key.is_empty() {
        "empty".to_string()
    } else {
        key
    }
}

pub fn request_fingerprint(request: &CaptureRequest) -> Result<String, String> {
    let canonical =
        capsule_core::normalize_capture_content(request).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec(&canonical).map_err(|error| error.to_string())?;
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    Ok(format!("sha256-{}", hex_bytes(&digest)))
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>, StateError> {
    let bytes = match read_consistent(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(StateError::Io(error)),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(StateError::Json)
}

fn read_consistent(path: &Path) -> io::Result<Vec<u8>> {
    for attempt in 0..200 {
        match fs::read(path) {
            Ok(bytes) => return Ok(bytes),
            Err(error)
                if cfg!(windows)
                    && matches!(error.raw_os_error(), Some(5 | 32 | 33))
                    && attempt < 199 =>
            {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("bounded state read loop always returns")
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "state path has no parent"))?;
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("state.json");
    let temporary = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), counter));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
        let wide = |value: &Path| {
            value
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        };
        let source = wide(temporary);
        let target = wide(destination);
        #[allow(non_snake_case)]
        unsafe extern "system" {
            fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
        }
        for attempt in 0..200 {
            let moved = unsafe {
                MoveFileExW(
                    source.as_ptr(),
                    target.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            };
            if moved != 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            if !matches!(error.raw_os_error(), Some(5 | 32 | 33)) || attempt == 199 {
                return Err(error);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        unreachable!("bounded state replacement loop always returns")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone};

    fn request() -> CaptureRequest {
        CaptureRequest::new(
            "hello\r\nworld",
            "cap/one",
            "entry_reserved",
            PathBuf::from("C:\\lab\\capsule.db"),
            FixedOffset::east_opt(3600)
                .unwrap()
                .with_ymd_and_hms(2026, 9, 14, 12, 0, 0)
                .unwrap(),
        )
    }

    #[test]
    fn capture_ids_are_encoded_before_becoming_paths() {
        let store = StateStore::at_dir("state");
        assert!(!store
            .pending_path("../escape")
            .to_string_lossy()
            .contains("escape.json"));
        assert!(store
            .pending_path("cap/one")
            .starts_with(store.pending_dir()));
    }

    #[test]
    fn committed_records_strip_body_and_round_trip_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::at_dir(directory.path());
        let request = request();
        let fingerprint = request_fingerprint(&request).unwrap();
        let mut record = RecoveryRecord::pending(request, fingerprint, true);
        record.mark_committed(CommitReceipt::committed(
            "entry_reserved",
            "cap/one",
            Utc::now(),
        ));
        store.write_receipt(&record).unwrap();
        store.write_binding(&record).unwrap();
        let loaded = store.read_receipt("cap/one").unwrap().unwrap();
        assert!(loaded.request.is_none());
        assert_eq!(loaded.receipt.unwrap().uuid, "entry_reserved");
        assert!(store.read_binding("cap/one").unwrap().is_some());
    }

    #[test]
    fn writer_draft_marker_round_trips_and_is_cleared_on_commit() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::at_dir(directory.path());
        let request = request();
        let fingerprint = request_fingerprint(&request).unwrap();
        let mut record = RecoveryRecord::pending(request, fingerprint, false);
        record.writer_draft = Some(WriterDraftMetadata::editable());
        assert!(record.is_editable_writer_draft());
        store.write_pending(&record).unwrap();
        let loaded = store.read_pending("cap/one").unwrap().unwrap();
        assert_eq!(loaded.writer_draft, Some(WriterDraftMetadata::editable()));

        record.mark_committed(CommitReceipt::committed(
            "entry_reserved",
            "cap/one",
            Utc::now(),
        ));
        assert!(record.writer_draft.is_none());
        assert!(!record.is_editable_writer_draft());
    }

    #[test]
    fn legacy_records_without_writer_metadata_remain_readable() {
        let value = serde_json::json!({
            "version": 1,
            "captureId": "cap/one",
            "reservedUuid": "entry_reserved",
            "request": null,
            "requestFingerprint": "sha256-test",
            "databasePath": "C:\\lab\\capsule.db",
            "databaseIdentity": null,
            "entryCreatedAt": "2026-09-14T12:00:00+01:00",
            "wordCount": 0,
            "state": "committed",
            "receipt": null,
            "context": null,
            "explicitCaptureId": false,
            "createdAt": "2026-09-14T11:00:00Z",
            "updatedAt": "2026-09-14T11:00:00Z",
            "expiresAt": null,
            "lastError": null
        });
        let record: RecoveryRecord = serde_json::from_value(value).unwrap();
        assert!(record.writer_draft.is_none());
    }

    #[test]
    fn lock_sidecar_is_exclusive_and_released_after_drop() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::at_dir(directory.path());
        let lock = store.lock("cap-1").unwrap();
        assert!(matches!(store.lock("cap-1"), Err(StateError::Busy(_))));
        drop(lock);
        assert!(store.lock("cap-1").is_ok());
    }

    #[test]
    fn expiry_is_read_only_without_binding_and_locked_with_binding() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::at_dir(directory.path());
        let request = request();
        let fingerprint = request_fingerprint(&request).unwrap();
        let mut record = RecoveryRecord::pending(request, fingerprint, true);
        record.mark_committed(CommitReceipt::committed(
            "entry_reserved",
            "cap/one",
            Utc::now(),
        ));
        record.expires_at = Some(Utc::now() - chrono::Duration::seconds(1));
        store.write_receipt(&record).unwrap();
        assert!(store.read_receipt("cap/one").unwrap().is_some());
        assert!(store.receipt_path("cap/one").is_file());

        store.write_binding(&record).unwrap();
        assert!(store.read_receipt("cap/one").unwrap().is_none());
        assert!(store.receipt_path("cap/one").is_file());
        assert!(store.prune_expired_receipt("cap/one").unwrap());
        assert!(!store.receipt_path("cap/one").exists());
    }

    #[test]
    fn explicit_pending_discard_tombstone_has_no_commit_timestamp() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::at_dir(directory.path());
        let request = request();
        let fingerprint = request_fingerprint(&request).unwrap();
        let record = RecoveryRecord::pending(request, fingerprint, true);
        store.write_tombstone(&record).unwrap();
        let tombstone = store.read_tombstone("cap/one").unwrap().unwrap();
        assert_eq!(tombstone.capture_id, "cap/one");
        assert!(tombstone.expires_at.is_some());
        let json = serde_json::to_value(tombstone).unwrap();
        assert!(json.get("committedAt").is_none());
    }

    #[test]
    fn request_fingerprint_uses_only_shared_canonical_content() {
        let mut first = request();
        first.tags = vec![
            "Life".to_string(),
            " outdoors ".to_string(),
            "life".to_string(),
        ];
        first.title = Some("Title".to_string());
        let mut second = first.clone();
        second.capture_id = "different-id".to_string();
        second.reserved_uuid = "entry_other".to_string();
        second.database_path = PathBuf::from("C:\\other\\capsule.db");
        second.created_at += chrono::Duration::hours(3);
        second.tags = vec!["OUTDOORS".to_string(), "life".to_string()];
        assert_eq!(
            request_fingerprint(&first).unwrap(),
            request_fingerprint(&second).unwrap()
        );
        second.summary = Some("different".to_string());
        assert_ne!(
            request_fingerprint(&first).unwrap(),
            request_fingerprint(&second).unwrap()
        );
    }
}
