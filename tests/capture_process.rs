mod support;

use cap::recovery::{request_fingerprint, RecoveryRecord, StateStore};
use capsule_core::{
    contracts::{BackupPolicy, CaptureRequest},
    db::FileIdentity,
};
use chrono::Utc;
use rusqlite::Connection;
use serde_json::Value;
use std::io::Write;
use std::process::Stdio;
use support::Fixture;

#[cfg(feature = "test-hooks")]
fn enable_test_hooks(fixture: &Fixture) {
    std::fs::write(
        fixture.root.path().join("CAP_TEST_LAB_MARKER"),
        b"cap test lab\n",
    )
    .unwrap();
}

fn json_output(output: &std::process::Output) -> Value {
    assert!(
        output.stdout.starts_with(b"{"),
        "stdout was not one JSON envelope: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).expect("valid JSON envelope")
}

fn add_args<'a>(capture_id: &'a str, text: &'a str) -> Vec<&'a str> {
    vec![
        "--json",
        "--no-context",
        "add",
        "--capture-id",
        capture_id,
        "--mood",
        "content",
        "--tag",
        "life",
        "--title",
        "Walk",
        "--summary",
        "Short note",
        "--star",
        "--pin",
        "--format",
        "plain",
        "--",
        text,
    ]
}

#[test]
fn add_commits_metadata_backup_and_durable_receipt() {
    let fixture = Fixture::new();
    let before_rows = fixture.row_count("entries").unwrap();
    let output = fixture
        .command()
        .args(add_args("capture-process-1", "A quick walk"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_output(&output);
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["command"], "add");
    assert_eq!(value["data"]["captureId"], "capture-process-1");
    assert_eq!(value["data"]["saveState"], "committed");
    assert_eq!(value["data"]["wordCount"], 3);
    assert_eq!(value["data"]["weather"]["status"], "skipped");
    assert_eq!(fixture.row_count("entries").unwrap(), before_rows + 1);

    let uuid = value["data"]["entryUuid"].as_str().unwrap().to_string();
    let connection = Connection::open(&fixture.db).unwrap();
    let row = connection
        .query_row(
            "SELECT text, content_format, title, summary, mood, starred, pinned FROM entries WHERE uuid = ?1",
            [&uuid],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "A quick walk");
    assert_eq!(row.1, "plain");
    assert_eq!(row.2.as_deref(), Some("Walk"));
    assert_eq!(row.3.as_deref(), Some("Short note"));
    assert_eq!(row.4.as_deref(), Some("content"));
    assert_eq!(row.5, 1);
    assert_eq!(row.6, 1);

    let state = StateStore::at_dir(&fixture.paths().state);
    let receipt = state.read_receipt("capture-process-1").unwrap().unwrap();
    assert!(
        receipt.request.is_none(),
        "receipt must not retain entry body"
    );
    assert_eq!(receipt.receipt.unwrap().uuid, uuid);
    assert!(state.binding_path("capture-process-1").is_file());
    assert!(fixture.backup_dir().read_dir().unwrap().next().is_some());
}

#[test]
fn comma_separated_tags_save_individually_and_match_repeated_tags_on_retry() {
    let fixture = Fixture::new();
    let before_rows = fixture.row_count("entries").unwrap();
    let mut saved_uuid = None;
    for tag_args in [
        vec!["--tags", "life,outdoors,gratitude,exercise"],
        vec![
            "--tag",
            "life",
            "--tag",
            "outdoors",
            "--tag",
            "gratitude",
            "--tag",
            "exercise",
        ],
        vec![
            "--tags",
            " Life, outdoors,, ",
            "--tag",
            "gratitude",
            "--tags",
            "exercise,LIFE,",
        ],
    ] {
        let output = fixture
            .command()
            .args([
                "--json",
                "--no-context",
                "add",
                "--capture-id",
                "comma-tags",
                "--mood",
                "good",
            ])
            .args(tag_args)
            .arg("Had a lovely walk")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let value = json_output(&output);
        let uuid = value["data"]["entryUuid"].as_str().unwrap().to_string();
        if let Some(expected) = &saved_uuid {
            assert_eq!(&uuid, expected);
        } else {
            saved_uuid = Some(uuid.clone());
        }
        assert_eq!(fixture.row_count("entries").unwrap(), before_rows + 1);

        let show = fixture
            .command()
            .args(["--json", "show", &uuid])
            .output()
            .unwrap();
        assert!(show.status.success());
        let entry = json_output(&show);
        assert_eq!(entry["data"]["text"], "Had a lovely walk");
        assert_eq!(entry["data"]["mood"], "good");
        let mut tags: Vec<_> = entry["data"]["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tag| tag["name"].as_str().unwrap())
            .collect();
        tags.sort();
        assert_eq!(tags, ["exercise", "gratitude", "life", "outdoors"]);
    }
}

#[test]
fn explicit_capture_id_is_idempotent_and_conflicts_are_visible() {
    let fixture = Fixture::new();
    let first = fixture
        .command()
        .args(add_args("capture-process-2", "Same content"))
        .output()
        .unwrap();
    assert!(first.status.success());
    let first_value = json_output(&first);
    let first_uuid = first_value["data"]["entryUuid"].as_str().unwrap();
    let rows_after_first = fixture.row_count("entries").unwrap();

    let second = fixture
        .command()
        .args(add_args("capture-process-2", "Same content"))
        .output()
        .unwrap();
    assert!(second.status.success());
    let second_value = json_output(&second);
    assert_eq!(second_value["data"]["entryUuid"], first_uuid);
    assert_eq!(fixture.row_count("entries").unwrap(), rows_after_first);

    let conflict = fixture
        .command()
        .args(add_args("capture-process-2", "Different content"))
        .output()
        .unwrap();
    assert_eq!(conflict.status.code(), Some(2));
    let conflict_value = json_output(&conflict);
    assert!(!conflict_value["ok"].as_bool().unwrap());
    assert_eq!(conflict_value["error"]["code"], "CAPTURE_ID_CONFLICT");
    assert_eq!(fixture.row_count("entries").unwrap(), rows_after_first);
}

#[test]
fn dry_run_is_read_only_and_reports_the_proposed_payload() {
    let fixture = Fixture::new();
    let before = fixture.snapshot().unwrap();
    let output = fixture
        .command()
        .args([
            "--json",
            "--dry-run",
            "--no-context",
            "add",
            "--capture-id",
            "capture-dry-run",
            "--tag",
            "draft",
            "--",
            "A dry run",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value = json_output(&output);
    assert_eq!(value["data"]["dryRun"], true);
    assert_eq!(value["data"]["saveState"], "not_committed");
    assert_eq!(value["data"]["text"], "A dry run");
    fixture.assert_snapshot_unchanged(&before).unwrap();
    let state = StateStore::at_dir(&fixture.paths().state);
    assert!(!state.pending_path("capture-dry-run").exists());
    assert!(!state.receipt_path("capture-dry-run").exists());
    assert!(fixture.backup_dir().read_dir().unwrap().next().is_none());
}

#[test]
fn stdin_capture_and_recovery_status_are_machine_safe() {
    let fixture = Fixture::new();
    let mut command = fixture.command();
    command
        .args([
            "--json",
            "--no-context",
            "add",
            "--stdin",
            "--capture-id",
            "capture-stdin",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"Piped entry\r\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(!output.stdout.contains(&0x1b));
    let saved = json_output(&output);
    let uuid = saved["data"]["entryUuid"].clone();

    let status = fixture
        .command()
        .args(["--json", "status", "--capture-id", "capture-stdin"])
        .output()
        .unwrap();
    assert!(status.status.success());
    let status_value = json_output(&status);
    assert_eq!(status_value["data"]["captureId"], "capture-stdin");
    assert_eq!(status_value["data"]["saveState"], "committed");
    assert_eq!(status_value["data"]["receipt"]["uuid"], uuid);

    let show = fixture
        .command()
        .args(["--json", "recover", "show", "capture-stdin"])
        .output()
        .unwrap();
    assert!(show.status.success());
    assert_eq!(json_output(&show)["data"]["saveState"], "committed");
}

#[test]
fn invalid_theme_is_rejected_before_any_capture_state_or_backup() {
    let fixture = Fixture::new();
    let before = fixture.snapshot().unwrap();
    let output = fixture
        .command()
        .args([
            "--json",
            "--theme",
            "banana",
            "--no-context",
            "add",
            "--capture-id",
            "capture-invalid-theme",
            "--",
            "Should not save",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value = json_output(&output);
    assert_eq!(value["error"]["code"], "INVALID_CONFIG");
    fixture.assert_snapshot_unchanged(&before).unwrap();
    let state = StateStore::at_dir(&fixture.paths().state);
    assert!(!state.pending_path("capture-invalid-theme").exists());
    assert!(fixture.backup_dir().read_dir().unwrap().next().is_none());
}

#[test]
fn a_capture_id_bound_to_another_database_is_not_replayed() {
    let first = Fixture::new();
    let saved = first
        .command()
        .args(add_args("capture-bound-to-first-db", "Bound content"))
        .output()
        .unwrap();
    assert!(saved.status.success());

    let second = Fixture::new();
    let before_rows = second.row_count("entries").unwrap();
    let output = second
        .command()
        .env("CAP_CONFIG_HOME", &first.paths().state)
        .args(add_args("capture-bound-to-first-db", "Bound content"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(json_output(&output)["error"]["code"], "DB_REPLACED");
    assert_eq!(second.row_count("entries").unwrap(), before_rows);
}

#[test]
fn capture_record_lock_blocks_a_second_process_until_released() {
    let fixture = Fixture::new();
    let state = StateStore::at_dir(&fixture.paths().state);
    let lock = state.lock("capture-lock-holder").unwrap();
    let blocked = fixture
        .command()
        .args([
            "--json",
            "--no-context",
            "add",
            "--capture-id",
            "capture-lock-holder",
            "--",
            "blocked once",
        ])
        .output()
        .unwrap();
    assert_eq!(blocked.status.code(), Some(4));
    assert_eq!(json_output(&blocked)["error"]["code"], "CAPTURE_BUSY");
    drop(lock);

    let retried = fixture
        .command()
        .args([
            "--json",
            "--no-context",
            "add",
            "--capture-id",
            "capture-lock-holder",
            "--",
            "blocked once",
        ])
        .output()
        .unwrap();
    assert!(retried.status.success());
    assert_eq!(fixture.row_count("entries").unwrap(), 3);
}

#[test]
fn recover_show_reveals_only_the_pending_draft_body() {
    let fixture = Fixture::new();
    let mut request = CaptureRequest::new(
        "pending draft body",
        "capture-draft-show",
        "entry_pending_show",
        fixture.db.clone(),
        Utc::now().fixed_offset(),
    );
    request.database_identity = Some(FileIdentity::for_path(&fixture.db));
    request.backup_policy = Some(BackupPolicy::new(fixture.backup_dir(), 3));
    let fingerprint = request_fingerprint(&request).unwrap();
    let record = RecoveryRecord::pending(request, fingerprint, true);
    let state = StateStore::at_dir(&fixture.paths().state);
    state.write_pending(&record).unwrap();

    let show = fixture
        .command()
        .args(["--json", "recover", "show", "capture-draft-show"])
        .output()
        .unwrap();
    assert!(show.status.success());
    let show_value = json_output(&show);
    assert_eq!(show_value["data"]["draft"]["text"], "pending draft body");

    let status = fixture
        .command()
        .args(["--json", "status", "--capture-id", "capture-draft-show"])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(json_output(&status)["data"].get("draft").is_none());
}

#[test]
fn simultaneous_same_id_captures_commit_at_most_one_entry() {
    let fixture = Fixture::new();
    let args = [
        "--json",
        "--no-context",
        "add",
        "--capture-id",
        "capture-race",
        "--",
        "one durable entry",
    ];
    let mut first = fixture.command();
    first
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut second = fixture.command();
    second
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let first = first.spawn().unwrap();
    let second = second.spawn().unwrap();
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    for output in [&first, &second] {
        assert!(
            output.status.success() || output.status.code() == Some(4),
            "unexpected race result: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        if output.status.success() {
            assert_eq!(json_output(output)["data"]["saveState"], "committed");
        }
    }
    assert_eq!(fixture.row_count("entries").unwrap(), 3);
}

#[cfg(feature = "test-hooks")]
#[test]
fn killed_after_commit_is_reconciled_without_a_duplicate() {
    let fixture = Fixture::new();
    enable_test_hooks(&fixture);
    let killed = fixture
        .command()
        .env("CAP_TEST_LAB_ROOT", fixture.root.path())
        .env("CAP_TEST_HOOK", "after_commit")
        .args([
            "--json",
            "--no-context",
            "add",
            "--capture-id",
            "capture-killed-after-commit",
            "--",
            "crash after commit",
        ])
        .output()
        .unwrap();
    assert_eq!(killed.status.code(), Some(97));
    assert_eq!(fixture.row_count("entries").unwrap(), 3);
    let state = StateStore::at_dir(&fixture.paths().state);
    assert!(state.pending_path("capture-killed-after-commit").is_file());
    assert!(!state.receipt_path("capture-killed-after-commit").exists());

    let status = fixture
        .command()
        .args([
            "--json",
            "status",
            "--capture-id",
            "capture-killed-after-commit",
        ])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert_eq!(json_output(&status)["data"]["saveState"], "committed");
    assert_eq!(fixture.row_count("entries").unwrap(), 3);
    assert!(state.receipt_path("capture-killed-after-commit").is_file());
}

#[cfg(feature = "test-hooks")]
#[test]
fn unknown_commit_keeps_exit_six_and_status_can_reconcile() {
    let fixture = Fixture::new();
    enable_test_hooks(&fixture);
    let unknown = fixture
        .command()
        .env("CAP_TEST_LAB_ROOT", fixture.root.path())
        .env("CAP_TEST_HOOK", "during_commit:error")
        .args([
            "--json",
            "--no-context",
            "add",
            "--capture-id",
            "capture-unknown",
            "--",
            "unknown result",
        ])
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(6));
    let value = json_output(&unknown);
    assert_eq!(value["error"]["code"], "COMMIT_UNKNOWN");
    assert_eq!(value["data"]["saveState"], "unknown");
    let state = StateStore::at_dir(&fixture.paths().state);
    assert!(state.pending_path("capture-unknown").is_file());

    let status = fixture
        .command()
        .args(["--json", "status", "--capture-id", "capture-unknown"])
        .output()
        .unwrap();
    assert!(status.status.success());
    let status_value = json_output(&status);
    assert!(matches!(
        status_value["data"]["saveState"].as_str(),
        Some("committed") | Some("unknown") | Some("pending")
    ));
    assert!(fixture.row_count("entries").unwrap() <= 3);
}

#[cfg(feature = "test-hooks")]
#[test]
fn receipt_and_binding_write_failures_never_turn_a_saved_entry_into_a_retry() {
    for (capture_id, failure_kind) in [
        ("capture-receipt-failure", "receipt"),
        ("capture-binding-failure", "binding"),
    ] {
        let fixture = Fixture::new();
        enable_test_hooks(&fixture);
        let output = fixture
            .command()
            .env("CAP_TEST_LAB_ROOT", fixture.root.path())
            .env("CAP_TEST_STORAGE_FAIL", failure_kind)
            .args([
                "--json",
                "--no-context",
                "add",
                "--capture-id",
                capture_id,
                "--",
                "saved before local state failure",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(json_output(&output)["data"]["saveState"], "committed");
        assert_eq!(fixture.row_count("entries").unwrap(), 3);
        let state = StateStore::at_dir(&fixture.paths().state);
        assert!(state.pending_path(capture_id).is_file());

        let status = fixture
            .command()
            .args(["--json", "status", "--capture-id", capture_id])
            .output()
            .unwrap();
        assert!(status.status.success());
        assert_eq!(json_output(&status)["data"]["saveState"], "committed");
        assert_eq!(fixture.row_count("entries").unwrap(), 3);
        assert!(state.receipt_path(capture_id).is_file());
        assert!(state.binding_path(capture_id).is_file());
        assert!(!state.pending_path(capture_id).exists());
    }
}

#[cfg(feature = "test-hooks")]
#[test]
fn discarded_explicit_unknown_capture_leaves_an_unverifiable_tombstone() {
    let fixture = Fixture::new();
    enable_test_hooks(&fixture);
    let killed = fixture
        .command()
        .env("CAP_TEST_LAB_ROOT", fixture.root.path())
        .env("CAP_TEST_HOOK", "after_commit")
        .args([
            "--json",
            "--no-context",
            "add",
            "--capture-id",
            "capture-tombstone",
            "--",
            "do not know if this committed",
        ])
        .output()
        .unwrap();
    assert_eq!(killed.status.code(), Some(97));
    let discard = fixture
        .command()
        .args(["--json", "recover", "discard", "capture-tombstone", "--yes"])
        .output()
        .unwrap();
    assert!(discard.status.success());
    assert_eq!(json_output(&discard)["data"]["tombstoned"], true);

    let replay = fixture
        .command()
        .args([
            "--json",
            "--no-context",
            "add",
            "--capture-id",
            "capture-tombstone",
            "--",
            "do not know if this committed",
        ])
        .output()
        .unwrap();
    assert_eq!(replay.status.code(), Some(6));
    assert_eq!(
        json_output(&replay)["error"]["code"],
        "CAPTURE_UNVERIFIABLE"
    );
}
