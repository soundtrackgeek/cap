//! Orchestrator acceptance at the executable boundary, independent of the
//! capture implementation's fault hooks. Every invocation owns its whole lab.
mod support;

use serde_json::Value;
use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
};
use support::Fixture;

fn command(fixture: &Fixture) -> Command {
    // Allows reviewing an immutable worker executable before cherry-picking
    // its implementation. The child still receives only the fixture env.
    if let Some(executable) = std::env::var_os("CAP_ACCEPTANCE_EXECUTABLE") {
        let mut command = Command::new(executable);
        command.env_clear().envs(fixture.isolated_environment());
        command.current_dir(fixture.root.path());
        command
    } else {
        fixture.command()
    }
}

fn envelope(output: Output, expected_exit: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(expected_exit),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.stdout.contains(&0x1b));
    assert!(!output.stderr.contains(&0x1b));
    let value: Value = serde_json::from_slice(&output.stdout).expect("one JSON object");
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["ok"], expected_exit == 0);
    assert!(output.stdout.ends_with(b"\n"));
    value
}

fn saved(fixture: &Fixture, args: &[&str]) -> Value {
    envelope(
        command(fixture)
            .args(["--json", "--no-context"])
            .args(args)
            .output()
            .unwrap(),
        0,
    )
}

#[test]
fn continuation_number_is_frozen_as_a_uuid_before_pending_storage() {
    let fixture = Fixture::new();
    let db = rusqlite::Connection::open(&fixture.db).unwrap();
    let parent: String = db
        .query_row("SELECT uuid FROM entries WHERE id=1", [], |row| row.get(0))
        .unwrap();
    drop(db);
    let blocked = fixture.root.path().join("continuation-backup-file");
    fs::write(&blocked, "synthetic blocker").unwrap();
    envelope(
        command(&fixture)
            .env("CAPSULE_BACKUP_DIR", &blocked)
            .args([
                "--json",
                "--no-context",
                "add",
                "--capture-id",
                "continued-draft",
                "--continue",
                "1",
                "Next chapter",
            ])
            .output()
            .unwrap(),
        5,
    );
    let pending = saved(&fixture, &["recover", "show", "continued-draft"]);
    assert_eq!(pending["data"]["draft"]["continueFromUuid"], parent);
    fs::remove_file(&blocked).unwrap();
    fs::create_dir(&blocked).unwrap();
    let retry = saved(&fixture, &["recover", "retry", "continued-draft"]);
    assert_eq!(retry["data"]["saveState"], "committed");
}

#[test]
fn real_save_crosses_daily_milestone_once_and_enrichment_never_duplicates_it() {
    let fixture = Fixture::new();
    // Move only our generated seed rows away from the real local test date.
    let db = rusqlite::Connection::open(&fixture.db).unwrap();
    db.execute(
        "UPDATE entries SET created_at='2001' || substr(created_at, 5)",
        [],
    )
    .unwrap();
    drop(db);
    let words = std::iter::repeat_n("harbor", 50)
        .collect::<Vec<_>>()
        .join(" ");
    let result = saved(
        &fixture,
        &["add", "--capture-id", "daily-crossing", "--", &words],
    );
    let glints = result["data"]["milestones"]
        .as_array()
        .expect("actual threshold crossing");
    assert!(glints
        .iter()
        .any(|glint| glint["kind"] == "daily_words" && glint["threshold"] == 50));
    let retry = saved(
        &fixture,
        &["add", "--capture-id", "daily-crossing", "--", &words],
    );
    assert!(retry["data"].get("milestones").is_none());
    assert_eq!(retry["data"]["entryUuid"], result["data"]["entryUuid"]);
    let uuid = result["data"]["entryUuid"].as_str().unwrap();
    let before = fixture.snapshot().unwrap();
    let enriched = saved(&fixture, &["enrich", uuid]);
    assert_eq!(enriched["data"]["entryUuid"], uuid);
    assert_eq!(
        before,
        fixture.snapshot().unwrap(),
        "no-context enrichment changed stored data"
    );
    let extra = saved(&fixture, &["add", "one more word"]);
    assert!(extra["data"].get("milestones").is_none());
}

#[test]
fn supported_legacy_ids_are_repaired_only_after_capture_backup() {
    {
        let profile = support::FixtureProfile::nullable_ids();
        let fixture = Fixture::from_profile(profile);
        let before = fixture.snapshot().unwrap();
        let dry = saved(&fixture, &["--dry-run", "add", "A repaired capture"]);
        assert_eq!(dry["data"]["dryRun"], true);
        assert_eq!(before, fixture.snapshot().unwrap());
        let result = saved(&fixture, &["add", "A repaired capture"]);
        let backup_path = result["data"]["backup"]["path"]
            .as_str()
            .expect("verified backup");
        let backup = rusqlite::Connection::open_with_flags(
            backup_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            backup
                .query_row("SELECT COUNT(*) FROM entries", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            5
        );
        assert!(
            backup
                .query_row(
                    "SELECT COUNT(*) - COUNT(DISTINCT id) FROM entries",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap()
                > 0,
            "backup must preserve the original repair candidate"
        );
        let db = rusqlite::Connection::open(&fixture.db).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM entries", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            6
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) - COUNT(DISTINCT id) FROM entries",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
    let fixture = Fixture::from_profile(support::FixtureProfile::duplicate_ids());
    let before = fixture.snapshot().unwrap();
    let failed = envelope(
        command(&fixture)
            .args([
                "--json",
                "--no-context",
                "add",
                "Ambiguous references must be refused",
            ])
            .output()
            .unwrap(),
        5,
    );
    assert!(failed["error"]["message"]
        .as_str()
        .unwrap()
        .contains("duplicated"));
    assert_eq!(before, fixture.snapshot().unwrap());
}

#[test]
fn file_content_survives_exactly_and_display_controls_never_enter_storage() {
    let fixture = Fixture::new();
    let text = "  # Harbor\r\n\r\nCafé · 窓 · 👩🏽‍💻\rtrailing  \r\n";
    let input = fixture.root.path().join("note with spaces.md");
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(text.as_bytes());
    fs::write(&input, bytes).unwrap();
    let result = envelope(
        command(&fixture)
            .args(["--json", "--no-context", "--theme", "neon", "add", "--file"])
            .arg(&input)
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(result["command"], "add");
    assert!(
        result["data"].get("text").is_none(),
        "capture receipt leaked full body"
    );
    let uuid = result["data"]["entryUuid"].as_str().unwrap();
    let shown = saved(&fixture, &["show", uuid]);
    assert_eq!(
        shown["data"]["text"],
        text.replace("\r\n", "\n").replace('\r', "\n")
    );
    assert!(!shown["data"]["text"].as_str().unwrap().contains('\x1b'));
}

#[test]
fn shell_ambiguities_and_deliberate_identical_entries_are_preserved() {
    let fixture = Fixture::new();
    let a = saved(&fixture, &["--", "today", "was wonderful"]);
    let b = saved(&fixture, &["add", "--", "today was wonderful"]);
    let a_uuid = a["data"]["entryUuid"].as_str().unwrap();
    let b_uuid = b["data"]["entryUuid"].as_str().unwrap();
    assert_ne!(a_uuid, b_uuid, "ordinary repeated text was deduplicated");
    for uuid in [a_uuid, b_uuid] {
        assert_eq!(
            saved(&fixture, &["show", uuid])["data"]["text"],
            "today was wonderful"
        );
    }
    let literal = saved(&fixture, &["add", "--", "--mood", "literal"]);
    assert_eq!(
        saved(
            &fixture,
            &["show", literal["data"]["entryUuid"].as_str().unwrap()]
        )["data"]["text"],
        "--mood literal"
    );
    let before = fixture.snapshot().unwrap();
    envelope(
        command(&fixture)
            .args(["--json", "doctor", "nonsense"])
            .output()
            .unwrap(),
        2,
    );
    fixture.assert_snapshot_unchanged(&before).unwrap();
}

#[test]
fn preflight_errors_and_failed_backup_never_claim_a_save() {
    let fixture = Fixture::new();
    let before = fixture.snapshot().unwrap();
    let bad_theme = envelope(
        command(&fixture)
            .args(["--json", "--theme", "banana", "add", "No mutation"])
            .output()
            .unwrap(),
        2,
    );
    assert!(!bad_theme["ok"].as_bool().unwrap());
    fixture.assert_snapshot_unchanged(&before).unwrap();
    let blocked = fixture.root.path().join("backup-target-file");
    fs::write(&blocked, "synthetic blocker").unwrap();
    let backup = envelope(
        command(&fixture)
            .env("CAPSULE_BACKUP_DIR", &blocked)
            .args([
                "--json",
                "--no-context",
                "add",
                "--capture-id",
                "blocked-backup",
                "No mutation",
            ])
            .output()
            .unwrap(),
        5,
    );
    assert_ne!(backup["data"]["saveState"], "committed");
    let message = backup["error"]["message"].as_str().unwrap();
    assert!(message.contains("failed to create"), "{message}");
    assert!(message.contains("backup-target-file"), "{message}");
    assert!(message.contains("Entry was not saved"), "{message}");
    assert!(
        message.contains("cap recover retry blocked-backup"),
        "{message}"
    );
    fixture.assert_snapshot_unchanged(&before).unwrap();
    let pending = saved(&fixture, &["recover", "show", "blocked-backup"]);
    assert_ne!(pending["data"]["saveState"], "committed");
}

#[test]
fn recovery_preserves_existing_orphan_metadata_in_verified_backup() {
    let fixture = Fixture::new();
    let db = rusqlite::Connection::open(&fixture.db).unwrap();
    db.execute_batch(
        "PRAGMA foreign_keys=OFF;
         INSERT INTO plugin_entry_locations
             (entry_uuid, latitude, longitude, created_at)
             VALUES ('deleted-entry', 0, 0, '2026-09-15');
         INSERT INTO plugin_media_assets
             (hash, mime_type, bytes, width, height, storage_backend, storage_key, created_at)
             VALUES ('synthetic-orphan', 'image/png', 1, 1, 1, 'local', 'synthetic.png', '2026-09-15');
         INSERT INTO plugin_entry_media
             (entry_uuid, media_id, created_at)
             SELECT 'deleted-entry', id, '2026-09-15' FROM plugin_media_assets LIMIT 1;",
    )
    .unwrap();
    let violations = |db: &rusqlite::Connection| {
        let mut statement = db.prepare("PRAGMA foreign_key_check").unwrap();
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    let original_violations = violations(&db);
    assert_eq!(original_violations.len(), 2);
    let original_count: i64 = db
        .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
        .unwrap();
    let text =
        "Created a new version of the Capsule CLI tool. Hopefully it will get ne writing again.";
    let args = [
        "add",
        "--capture-id",
        "orphan-metadata",
        "--mood",
        "ok",
        "--tags",
        "capsule,writing,cli",
        text,
    ];
    fs::remove_dir(fixture.backup_dir()).unwrap();
    fs::write(fixture.backup_dir(), "synthetic backup blocker").unwrap();
    let failure = envelope(
        command(&fixture)
            .args(["--json", "--no-context"])
            .args(args)
            .output()
            .unwrap(),
        5,
    );
    assert_eq!(failure["data"]["saveState"], "not_committed");
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM entries", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        original_count
    );
    fs::remove_file(fixture.backup_dir()).unwrap();
    fs::create_dir(fixture.backup_dir()).unwrap();
    let result = saved(&fixture, &["recover", "retry", "orphan-metadata"]);
    let entry = saved(
        &fixture,
        &["show", result["data"]["entryUuid"].as_str().unwrap()],
    );
    assert_eq!(entry["data"]["text"], text);
    assert_eq!(entry["data"]["mood"], "ok");
    assert_eq!(
        entry["data"]["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tag| tag["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["capsule", "cli", "writing"]
    );
    assert_eq!(violations(&db), original_violations);
    let backup = rusqlite::Connection::open_with_flags(
        result["data"]["backup"]["path"].as_str().unwrap(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(violations(&backup), original_violations);
    assert_eq!(
        backup
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        original_count
    );
    assert_eq!(
        backup
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    let retry = saved(&fixture, &args);
    assert_eq!(retry["data"]["entryUuid"], result["data"]["entryUuid"]);
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM entries", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        original_count + 1
    );
}

#[test]
fn capture_id_retries_match_canonical_content_and_refuse_another_database() {
    let fixture = Fixture::new();
    let first = saved(
        &fixture,
        &[
            "add",
            "--capture-id",
            "caller-retry",
            "--tag",
            " Work ",
            "--tag",
            "WORK",
            "--title",
            "  Walk  ",
            "--",
            "same words",
        ],
    );
    let again = saved(
        &fixture,
        &[
            "add",
            "--capture-id",
            "caller-retry",
            "--tag",
            "work",
            "--title",
            "Walk",
            "--",
            "same words",
        ],
    );
    assert_eq!(first["data"]["entryUuid"], again["data"]["entryUuid"]);
    assert_eq!(first["data"]["createdAt"], again["data"]["createdAt"]);
    let other = Fixture::new();
    let before = fixture.snapshot().unwrap();
    let other_before = other.snapshot().unwrap();
    let redirected = envelope(
        command(&fixture)
            .arg("--db")
            .arg(&other.db)
            .args([
                "--json",
                "--no-context",
                "add",
                "--capture-id",
                "caller-retry",
                "--tag",
                "work",
                "--title",
                "Walk",
                "--",
                "same words",
            ])
            .output()
            .unwrap(),
        4,
    );
    assert_eq!(redirected["error"]["code"], "DB_REPLACED");
    fixture.assert_snapshot_unchanged(&before).unwrap();
    other.assert_snapshot_unchanged(&other_before).unwrap();
}

#[test]
fn default_piped_capture_is_one_entry_and_invalid_utf8_is_rejected() {
    let fixture = Fixture::new();
    let mut child = command(&fixture)
        .args(["--json", "--no-context"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"one\r\ntwo\r\n")
        .unwrap();
    let created = envelope(child.wait_with_output().unwrap(), 0);
    let shown = saved(
        &fixture,
        &["show", created["data"]["entryUuid"].as_str().unwrap()],
    );
    assert_eq!(shown["data"]["text"], "one\ntwo\n");
    let before = fixture.snapshot().unwrap();
    let invalid = fixture.root.path().join("invalid-utf8.txt");
    fs::write(&invalid, [0xff, 0xfe, 0x80]).unwrap();
    envelope(
        command(&fixture)
            .args(["--json", "add", "--file"])
            .arg(invalid)
            .output()
            .unwrap(),
        2,
    );
    fixture.assert_snapshot_unchanged(&before).unwrap();
}
