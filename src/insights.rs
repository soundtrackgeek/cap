//! Cap-local state for memory rediscovery and milestone glints.
//!
//! This state is deliberately separate from Capsule's journal and
//! gamification tables.  Every record is keyed by the canonical database
//! identity, and updates are serialized through a persistent OS file lock plus
//! an atomic same-directory replacement.  A failure to read/query/persist a
//! glint is returned as a warning so it can never turn a confirmed capture
//! into a failed save.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use capsule_core::{
    db::FileIdentity,
    stats::{self, MemoryEntry, MilestoneInputs, NaiveDate},
};
use serde::{Deserialize, Serialize};

use crate::preferences;

pub const MEMORY_STATE_FILE_NAME: &str = "memory-state.json";

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct MemoryState {
    #[serde(default)]
    last_recall_by_database: BTreeMap<String, String>,
    #[serde(default)]
    emitted_milestones_by_database: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStateStore {
    path: PathBuf,
}

impl MemoryStateStore {
    pub fn from_env() -> Result<Self, String> {
        let home = preferences::config_home_from_env().map_err(|error| error.to_string())?;
        Ok(Self::at_dir(&home))
    }

    pub fn at_dir(directory: &Path) -> Self {
        Self {
            path: directory.join(MEMORY_STATE_FILE_NAME),
        }
    }

    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock_path(&self) -> PathBuf {
        self.path.with_extension("lock")
    }

    fn load_unlocked(&self) -> Result<MemoryState, String> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(MemoryState::default())
            }
            Err(error) => return Err(format!("memory state read failed: {error}")),
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| format!("memory state read failed: {error}"))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| format!("memory state is invalid JSON: {error}"))
    }

    fn with_lock<T>(
        &self,
        operation: impl FnOnce(&mut MemoryState) -> Result<T, String>,
    ) -> Result<T, String> {
        self.with_lock_until(None, operation)
    }

    fn with_lock_until<T>(
        &self,
        deadline: Option<Instant>,
        operation: impl FnOnce(&mut MemoryState) -> Result<T, String>,
    ) -> Result<T, String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "memory state path has no parent directory".to_string())?;
        if deadline.is_some_and(|value| Instant::now() >= value) {
            return Err("memory state deadline exceeded before directory setup".to_string());
        }
        fs::create_dir_all(parent)
            .map_err(|error| format!("memory state directory failed: {error}"))?;
        if deadline.is_some_and(|value| Instant::now() >= value) {
            return Err("memory state deadline exceeded before lock".to_string());
        }
        let lock_path = self.lock_path();
        let lock = acquire_lock(&lock_path, deadline)?;
        let _guard = LockGuard {
            _path: lock_path,
            _file: lock,
        };
        let mut state = self.load_unlocked()?;
        if deadline.is_some_and(|value| Instant::now() >= value) {
            return Err("memory state deadline exceeded before update".to_string());
        }
        let result = operation(&mut state);
        if result.is_ok() {
            if deadline.is_some_and(|value| Instant::now() >= value) {
                return Err("memory state deadline exceeded before publish".to_string());
            }
            self.save_unlocked(&state)?;
        }
        result
    }

    fn save_unlocked(&self, state: &MemoryState) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "memory state path has no parent directory".to_string())?;
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let file_name = self
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(MEMORY_STATE_FILE_NAME);
        let temporary = parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            counter
        ));
        let result = (|| -> Result<(), String> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .map_err(|error| format!("memory state temporary file failed: {error}"))?;
            let mut bytes = serde_json::to_vec_pretty(state)
                .map_err(|error| format!("memory state serialization failed: {error}"))?;
            bytes.push(b'\n');
            file.write_all(&bytes)
                .and_then(|()| file.flush())
                .and_then(|()| file.sync_all())
                .map_err(|error| format!("memory state write failed: {error}"))?;
            drop(file);
            replace_file(&temporary, &self.path)
                .map_err(|error| format!("memory state publish failed: {error}"))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    /// Return the previous recall UUID for a database without creating state.
    pub fn previous_recall(&self, identity: &FileIdentity) -> Result<Option<String>, String> {
        let state = self.load_unlocked()?;
        Ok(state
            .last_recall_by_database
            .get(&identity_key(identity))
            .cloned())
    }

    /// Select and persist a recall candidate atomically.  The previous UUID is
    /// avoided whenever there is another candidate.
    pub fn choose_recall(
        &self,
        identity: &FileIdentity,
        candidates: &[MemoryEntry],
        seed: u64,
    ) -> Result<Option<MemoryEntry>, String> {
        self.with_lock(|state| {
            let key = identity_key(identity);
            let previous = state.last_recall_by_database.get(&key).map(String::as_str);
            let selected = stats::choose_recall(candidates, previous, seed).cloned();
            if let Some(uuid) = selected.as_ref().and_then(|entry| entry.uuid.clone()) {
                state.last_recall_by_database.insert(key, uuid);
            }
            Ok(selected)
        })
    }

    /// Query shared metrics once and return the current totals without
    /// emitting a glint.  A committed UUID is required to prove a crossing;
    /// callers that have one should use `milestone_receipt_for_committed`.
    pub fn milestone_receipt(
        &self,
        path: &Path,
        identity: &FileIdentity,
        today: NaiveDate,
        include_hidden: bool,
    ) -> MilestoneReceipt {
        let started = Instant::now();
        let inputs = match stats::milestone_inputs_for_database(path, today, include_hidden) {
            Ok(inputs) => inputs,
            Err(error) => {
                return MilestoneReceipt {
                    glints: Vec::new(),
                    warnings: vec![format!("Milestones unavailable: {error}")],
                    inputs: None,
                }
            }
        };
        let mut warnings = Vec::new();
        if started.elapsed() > Duration::from_millis(200) {
            warnings.push("Milestone check exceeded its 200ms optional budget.".to_string());
        }
        let _ = identity;
        MilestoneReceipt {
            glints: Vec::new(),
            warnings,
            inputs: Some(inputs),
        }
    }

    /// Query one post-commit snapshot and emit only thresholds crossed by the
    /// supplied committed UUID.  This is the strict receipt-facing API for a
    /// capture consumer; query, state lock, and atomic publish are bounded and
    /// all failures become warnings.
    pub fn milestone_receipt_for_committed(
        &self,
        path: &Path,
        identity: &FileIdentity,
        today: NaiveDate,
        committed_uuid: &str,
        include_hidden: bool,
    ) -> MilestoneReceipt {
        if identity.stable_id.is_none() {
            return MilestoneReceipt {
                glints: Vec::new(),
                warnings: vec![
                    "Milestones unavailable because the database has no stable file identity."
                        .to_string(),
                ],
                inputs: None,
            };
        }
        if !identity_matches_path(path, identity) {
            return MilestoneReceipt {
                glints: Vec::new(),
                warnings: vec![
                    "Milestones unavailable because the database identity changed before the check."
                        .to_string(),
                ],
                inputs: None,
            };
        }
        let started = Instant::now();
        let deadline = started + Duration::from_millis(200);
        let query_timeout =
            Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now()));
        let crossing = match stats::milestone_crossing_for_database_with_timeout(
            path,
            today,
            committed_uuid,
            include_hidden,
            query_timeout,
        ) {
            Ok(crossing) => crossing,
            Err(error) => {
                return MilestoneReceipt {
                    glints: Vec::new(),
                    warnings: vec![format!("Milestones unavailable: {error}")],
                    inputs: None,
                }
            }
        };
        if Instant::now() >= deadline {
            return MilestoneReceipt {
                glints: Vec::new(),
                warnings: vec!["Milestone check exceeded its 200ms optional budget.".to_string()],
                inputs: Some(crossing.inputs),
            };
        }
        if !identity_matches_path(path, identity) {
            return MilestoneReceipt {
                glints: Vec::new(),
                warnings: vec![
                    "Milestones unavailable because the database identity changed after the check."
                        .to_string(),
                ],
                inputs: Some(crossing.inputs),
            };
        }
        let (glints, publish_overrun) =
            match self.emit_crossings_until(path, identity, &crossing, deadline) {
                Ok(result) => result,
                Err(error) => {
                    return MilestoneReceipt {
                        glints: Vec::new(),
                        warnings: vec![format!("Milestones could not be persisted: {error}")],
                        inputs: Some(crossing.inputs),
                    }
                }
            };
        let mut warnings = Vec::new();
        if publish_overrun {
            warnings
                .push("Milestone state publish exceeded its 200ms optional budget.".to_string());
        }
        MilestoneReceipt {
            glints,
            warnings,
            inputs: Some(crossing.inputs),
        }
    }

    fn emit_crossings_until(
        &self,
        path: &Path,
        identity: &FileIdentity,
        crossing: &capsule_core::stats::MilestoneCrossing,
        deadline: Instant,
    ) -> Result<(Vec<MilestoneGlint>, bool), String> {
        let glints = self.with_lock_until(Some(deadline), |state| {
            if !identity_matches_path(path, identity) {
                return Err(
                    "database identity changed before milestone state publication".to_string(),
                );
            }
            let emitted = state
                .emitted_milestones_by_database
                .entry(identity_key(identity))
                .or_default();
            let mut glints = Vec::new();
            let inputs = &crossing.inputs;
            let daily_id = format!("daily:{}:{}", inputs.day, inputs.daily_threshold);
            if crossing.daily_crossed && emitted.insert(daily_id.clone()) {
                glints.push(MilestoneGlint {
                    id: daily_id,
                    kind: "daily_words".to_string(),
                    period: inputs.day.clone(),
                    words: inputs.daily_words,
                    threshold: inputs.daily_threshold,
                });
            }
            let weekly_id = format!("weekly:{}:{}", inputs.week_start, inputs.weekly_threshold);
            if crossing.weekly_crossed && emitted.insert(weekly_id.clone()) {
                glints.push(MilestoneGlint {
                    id: weekly_id,
                    kind: "weekly_words".to_string(),
                    period: inputs.week_start.clone(),
                    words: inputs.weekly_words,
                    threshold: inputs.weekly_threshold,
                });
            }
            Ok(glints)
        })?;
        Ok((glints, Instant::now() >= deadline))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MilestoneGlint {
    pub id: String,
    pub kind: String,
    pub period: String,
    pub words: i64,
    pub threshold: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MilestoneReceipt {
    pub glints: Vec<MilestoneGlint>,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<MilestoneInputs>,
}

pub fn identity_key(identity: &FileIdentity) -> String {
    format!(
        "{}|{}",
        identity.stable_id.as_deref().unwrap_or("-"),
        identity.canonical_path
    )
}

fn identity_matches_path(path: &Path, expected: &FileIdentity) -> bool {
    expected.same_file(&FileIdentity::for_path(path))
}

pub fn system_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0)
}

fn acquire_lock(path: &Path, deadline: Option<Instant>) -> Result<File, String> {
    let started = Instant::now();
    let lock_budget = deadline
        .map(|value| {
            value
                .saturating_duration_since(started)
                .min(Duration::from_millis(100))
        })
        .unwrap_or(Duration::from_millis(500));
    loop {
        if deadline.is_some_and(|value| Instant::now() >= value) {
            return Err(format!("memory state lock timed out: {}", path.display()));
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| format!("memory state lock failed: {error}"))?;
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {
                if started.elapsed() >= lock_budget
                    || deadline.is_some_and(|value| Instant::now() >= value)
                {
                    return Err(format!("memory state is busy: {}", path.display()));
                }
                drop(file);
                let sleep_for = deadline
                    .and_then(|value| value.checked_duration_since(Instant::now()))
                    .map_or(Duration::from_millis(2), |remaining| {
                        remaining.min(Duration::from_millis(2))
                    });
                std::thread::sleep(sleep_for);
            }
            Err(std::fs::TryLockError::Error(error)) => {
                return Err(format!("memory state lock failed: {error}"));
            }
        }
    }
}

struct LockGuard {
    _path: PathBuf,
    _file: File,
}

fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
        const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
        let wide = |path: &Path| {
            path.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        };
        #[allow(non_snake_case)]
        unsafe extern "system" {
            fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
        }
        let temporary_wide = wide(temporary);
        let destination_wide = wide(destination);
        for _ in 0..200 {
            let moved = unsafe {
                MoveFileExW(
                    temporary_wide.as_ptr(),
                    destination_wide.as_ptr(),
                    MOVEFILE_WRITE_THROUGH | MOVEFILE_REPLACE_EXISTING,
                )
            };
            if moved != 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            if !matches!(error.raw_os_error(), Some(5 | 32 | 33)) {
                return Err(error);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "timed out publishing memory state",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capsule_core::stats::MemoryEntry;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::tempdir;

    fn identity(path: &Path) -> FileIdentity {
        FileIdentity::for_path(path)
    }

    fn entry(uuid: &str) -> MemoryEntry {
        MemoryEntry {
            id: 1,
            uuid: Some(uuid.to_string()),
            created_at: "2026-09-14 09:00".to_string(),
            date: "2026-09-14".to_string(),
            text: uuid.to_string(),
            text_plain: uuid.to_string(),
            mood: None,
            hidden: false,
        }
    }

    #[test]
    fn recall_state_is_database_bound_and_non_repeating() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("capsule.db");
        File::create(&db).unwrap();
        let store = MemoryStateStore::at_dir(dir.path());
        let identity = identity(&db);
        let candidates = vec![entry("one"), entry("two")];
        assert_eq!(
            store
                .choose_recall(&identity, &candidates, 0)
                .unwrap()
                .unwrap()
                .uuid,
            Some("one".to_string())
        );
        assert_eq!(
            store
                .choose_recall(&identity, &candidates, 0)
                .unwrap()
                .unwrap()
                .uuid,
            Some("two".to_string())
        );
        let other = FileIdentity::for_path(&dir.path().join("other.db"));
        assert!(store.previous_recall(&other).unwrap().is_none());
    }

    #[test]
    fn concurrent_recall_updates_remain_valid_json() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("capsule.db");
        File::create(&db).unwrap();
        let store = Arc::new(MemoryStateStore::at_dir(dir.path()));
        let identity = identity(&db);
        let candidates = Arc::new(vec![entry("one"), entry("two")]);
        let barrier = Arc::new(Barrier::new(8));
        let mut handles = Vec::new();
        for seed in 0..8 {
            let store = Arc::clone(&store);
            let mut identity = identity.clone();
            identity.canonical_path.push_str(&format!("-{seed}"));
            let candidates = Arc::clone(&candidates);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                let result = store.choose_recall(&identity, &candidates, seed);
                (identity_key(&identity), result)
            }));
        }
        let mut successful = Vec::new();
        for handle in handles {
            let (key, result) = handle.join().unwrap();
            match result {
                Ok(Some(entry)) => successful.push((key, entry.uuid.unwrap())),
                // Recall state is optional and has a bounded lock wait. A
                // contended runner may legitimately exhaust that budget.
                Err(error) => assert!(error.starts_with("memory state is busy:"), "{error}"),
                Ok(None) => panic!("non-empty recall candidates returned no entry"),
            }
        }
        assert!(
            !successful.is_empty(),
            "at least the first lock holder must finish"
        );
        let state: MemoryState = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
        assert_eq!(state.last_recall_by_database.len(), successful.len());
        for (key, uuid) in successful {
            assert_eq!(state.last_recall_by_database.get(&key), Some(&uuid));
        }
    }

    #[test]
    fn busy_recall_preserves_state_and_resumes_after_lock_release() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("capsule.db");
        File::create(&db).unwrap();
        let store = MemoryStateStore::at_dir(dir.path());
        let identity = identity(&db);
        let candidates = vec![entry("one"), entry("two")];
        store.choose_recall(&identity, &candidates, 0).unwrap();
        let before = fs::read(store.path()).unwrap();
        let lock = acquire_lock(&store.lock_path(), None).unwrap();
        let error = store.choose_recall(&identity, &candidates, 0).unwrap_err();
        assert!(error.starts_with("memory state is busy:"), "{error}");
        assert_eq!(fs::read(store.path()).unwrap(), before);
        drop(lock);
        assert_eq!(
            store
                .choose_recall(&identity, &candidates, 0)
                .unwrap()
                .unwrap()
                .uuid
                .as_deref(),
            Some("two")
        );
    }

    #[test]
    fn milestone_receipt_rejects_a_replaced_frozen_identity_before_reading() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("capsule.db");
        File::create(&db).unwrap();
        let observed = FileIdentity::for_path(&db);
        let frozen = FileIdentity {
            canonical_path: observed.canonical_path,
            stable_id: Some("frozen-before-replacement".to_string()),
            size_bytes: observed.size_bytes,
            modified_at: observed.modified_at,
        };
        let store = MemoryStateStore::at_dir(dir.path());
        let receipt = store.milestone_receipt_for_committed(
            &db,
            &frozen,
            NaiveDate::from_ymd_opt(2026, 9, 14).unwrap(),
            "entry_missing",
            false,
        );
        assert!(receipt.glints.is_empty());
        assert!(receipt
            .warnings
            .iter()
            .any(|warning| warning.contains("identity changed")));
        assert!(!store.path().exists());
    }
}
