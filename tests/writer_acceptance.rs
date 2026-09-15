//! Writer/recovery integration tested through the actual CLI process.
mod support;

use cap::recovery::{StateStore, WriterDraftMetadata};
use std::fs;
use support::Fixture;

#[test]
fn new_writer_uuid_survives_durable_draft_and_save_in_capsule_format() {
    use cap::writer::draft::DraftLease;
    use capsule_core::db::{resolve_capsule, ResolveRequest};

    let fixture = Fixture::new();
    let resolved = resolve_capsule(ResolveRequest {
        database_path: Some(fixture.db.clone()),
        backup_directory: Some(fixture.backup_dir().to_path_buf()),
        settings: Some(Default::default()),
        ..Default::default()
    })
    .unwrap();
    let state_root =
        fixture.isolated_environment()[std::ffi::OsStr::new("CAP_CONFIG_HOME")].clone();
    let store = StateStore::at_dir(state_root);
    let mut draft = DraftLease::new(store.clone(), &resolved).unwrap();
    let capture_id = draft.capture_id().to_owned();
    draft.persist_text("Writer UUID regression").unwrap();
    let uuid = store
        .read_pending(&capture_id)
        .unwrap()
        .unwrap()
        .reserved_uuid;
    assert_eq!(uuid.len(), 14);
    assert!(uuid.starts_with("entry_"));
    assert!(uuid[6..]
        .bytes()
        .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase()));
    drop(draft);

    let output = fixture
        .command()
        .args(["--json", "--no-context", "recover", "retry", &capture_id])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let db = rusqlite::Connection::open(&fixture.db).unwrap();
    let saved_uuid: String = db
        .query_row(
            "SELECT uuid FROM entries WHERE text=?1",
            ["Writer UUID regression"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(saved_uuid, uuid);
    assert_eq!(
        store
            .read_receipt(&capture_id)
            .unwrap()
            .unwrap()
            .reserved_uuid,
        uuid
    );
}

#[test]
fn noninteractive_writer_rejects_without_journal_or_draft_mutation() {
    let fixture = Fixture::new();
    let before = fixture.snapshot().unwrap();
    for args in [
        vec!["write"],
        vec!["--json", "write"],
        vec!["--quiet", "write"],
        vec!["--dry-run", "write"],
        vec!["write", "--editor"],
    ] {
        let output = fixture.command().args(args).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.stdout.contains(&0x1b));
        fixture.assert_snapshot_unchanged(&before).unwrap();
    }
    assert_eq!(fs::read_dir(fixture.backup_dir()).unwrap().count(), 0);
    let state_root =
        fixture.isolated_environment()[std::ffi::OsStr::new("CAP_CONFIG_HOME")].clone();
    assert!(!StateStore::at_dir(state_root).pending_dir().exists());
}

#[test]
fn explicit_recovery_freezes_an_editable_writer_draft_before_capture() {
    let fixture = Fixture::new();
    let entries_before = fixture.row_count("entries").unwrap();
    let blocker = fixture.root.path().join("blocked-writer-backup");
    fs::write(&blocker, "synthetic backup blocker").unwrap();
    let call = |args: &[&str]| {
        fixture
            .command()
            .env("CAPSULE_BACKUP_DIR", &blocker)
            .args(["--json", "--no-context"])
            .args(args)
            .output()
            .unwrap()
    };
    let initial = call(&[
        "add",
        "--capture-id",
        "writer-acceptance",
        "Useful editable draft",
    ]);
    assert_eq!(initial.status.code(), Some(5));
    let state_root =
        fixture.isolated_environment()[std::ffi::OsStr::new("CAP_CONFIG_HOME")].clone();
    let store = StateStore::at_dir(state_root);
    let mut record = store.read_pending("writer-acceptance").unwrap().unwrap();
    let original_uuid = record.reserved_uuid.clone();
    let original_created = record.request.as_ref().unwrap().created_at;
    record.writer_draft = Some(WriterDraftMetadata::editable());
    {
        let _lock = store.lock("writer-acceptance").unwrap();
        store.write_pending(&record).unwrap();
    }
    let failed = call(&["recover", "retry", "writer-acceptance"]);
    assert_eq!(failed.status.code(), Some(5));
    let frozen = store.read_pending("writer-acceptance").unwrap().unwrap();
    assert!(!frozen.is_editable_writer_draft());
    assert_eq!(frozen.reserved_uuid, original_uuid);
    assert_eq!(
        frozen.request.as_ref().unwrap().created_at,
        original_created
    );
    fs::remove_file(&blocker).unwrap();
    fs::create_dir(&blocker).unwrap();
    assert_eq!(
        call(&["recover", "retry", "writer-acceptance"])
            .status
            .code(),
        Some(0)
    );
    // Retry acts on pending drafts. Once consumed, another retry reports
    // no pending capture; the durable committed receipt remains inspectable.
    assert_eq!(
        call(&["recover", "retry", "writer-acceptance"])
            .status
            .code(),
        Some(3)
    );
    assert_eq!(
        call(&["status", "--capture-id", "writer-acceptance"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(fixture.row_count("entries").unwrap(), entries_before + 1);
    assert!(store.read_pending("writer-acceptance").unwrap().is_none());
    assert!(!store
        .read_receipt("writer-acceptance")
        .unwrap()
        .unwrap()
        .is_editable_writer_draft());
}
