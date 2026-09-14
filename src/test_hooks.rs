//! Test-only process fault seams.
//!
//! This module is compiled only with the non-default `test-hooks` feature.
//! Every action requires a marker file and all relevant paths to remain under
//! the declared synthetic fixture root, so an inherited environment cannot
//! affect a real journal.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use capsule_core::capture::CaptureHookPoint;

fn authorized() -> Option<PathBuf> {
    let root = std::env::var_os("CAP_TEST_LAB_ROOT").map(PathBuf::from)?;
    if !root.join("CAP_TEST_LAB_MARKER").is_file() {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let db = std::env::var_os("CAPSULE_DB_PATH").map(PathBuf::from)?;
    let state = std::env::var_os("CAP_CONFIG_HOME").map(PathBuf::from)?;
    if !under(&root, &db) || !under(&root, &state) {
        return None;
    }
    Some(root)
}

fn under(root: &Path, path: &Path) -> bool {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let Ok(current) = std::env::current_dir() else {
            return false;
        };
        current.join(path)
    };
    let mut existing = absolute.as_path();
    while !existing.exists() {
        let Some(parent) = existing.parent() else {
            return false;
        };
        existing = parent;
    }
    existing
        .canonicalize()
        .is_ok_and(|resolved| resolved.starts_with(root))
}

fn request(stage: &str) -> Option<String> {
    authorized()?;
    let value = std::env::var("CAP_TEST_HOOK").ok()?;
    let mut parts = value.splitn(2, ':');
    if !parts.next()?.eq_ignore_ascii_case(stage) {
        return None;
    }
    Some(parts.next().unwrap_or("exit").to_ascii_lowercase())
}

/// Run a named process checkpoint. The default action is an immediate exit,
/// emulating a kill at that exact boundary without unwinding or writing more
/// state. Tests can request `:error` for a controlled core-hook failure.
pub(crate) fn stage(stage: &str) -> Result<()> {
    match request(stage).as_deref() {
        Some("error") => Err(anyhow!("test hook interrupted {stage}")),
        Some("exit") | Some("kill") => std::process::exit(97),
        _ => Ok(()),
    }
}

/// Test-only capture hook for the core's named mutation checkpoints.
pub(crate) fn checkpoint(point: CaptureHookPoint) -> Result<()> {
    let stage = match point {
        CaptureHookPoint::BeforeBackup => "before_backup",
        CaptureHookPoint::AfterBackup => "after_backup",
        CaptureHookPoint::BeforeBegin => "before_begin",
        CaptureHookPoint::BeforeInsert => "before_insert",
        CaptureHookPoint::BeforeFts => "before_fts",
        CaptureHookPoint::BeforeResequence => "before_resequence",
        CaptureHookPoint::BeforeCommit => "before_commit",
        CaptureHookPoint::DuringCommit => "during_commit",
        CaptureHookPoint::AfterCommit => "after_commit_hook",
    };
    match request(stage).as_deref() {
        Some("error") => Err(anyhow!("test hook interrupted {stage}")),
        Some("exit") | Some("kill") => std::process::exit(97),
        _ => Ok(()),
    }
}

pub(crate) fn storage_failure(kind: &str, path: &Path) -> bool {
    let Some(root) = authorized() else {
        return false;
    };
    let Some(value) = std::env::var("CAP_TEST_STORAGE_FAIL").ok() else {
        return false;
    };
    value.trim().eq_ignore_ascii_case(kind) && under(&root, path)
}
