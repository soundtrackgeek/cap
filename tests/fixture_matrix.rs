mod support;

use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, ErrorCode, OpenFlags};
use support::{
    Fixture, FixtureFts, FixtureIds, FixtureProfile, FixtureSchema, SeededClock,
    UnsupportedRequiredColumn, REQUIRED_ENTRY_COLUMNS,
};

#[test]
fn canonical_fixture_sql_is_a_full_synthetic_database() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(include_str!("fixtures/capsule.sql"))
        .unwrap();
    let entries: i64 = connection
        .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
        .unwrap();
    let locations: i64 = connection
        .query_row("SELECT COUNT(*) FROM plugin_entry_locations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!((entries, locations), (5, 2));
}

#[test]
fn full_fixture_covers_entry_relations_and_context() {
    let fixture = Fixture::from_profile(FixtureProfile::full());
    let tables = fixture.table_names().unwrap();

    for required in ["entries", "tags", "entry_tags", "entries_fts"] {
        assert!(
            tables.iter().any(|name| name == required),
            "missing {required}"
        );
    }
    for optional in [
        "entry_continuations",
        "entry_thread_titles",
        "entry_thread_summaries",
        "history",
        "plugin_media_assets",
        "plugin_entry_media",
        "plugin_entry_locations",
        "plugin_location_cache",
        "sync_location_tombstones",
        "sync_entry_thread_title_tombstones",
        "sync_entry_thread_summary_tombstones",
        "sync_image_tombstones",
    ] {
        assert!(
            tables.iter().any(|name| name == optional),
            "missing {optional}"
        );
        assert!(fixture.row_count(optional).unwrap() > 0, "empty {optional}");
    }

    assert_eq!(fixture.row_count("entries").unwrap(), 5);
    let connection = fixture.connection().unwrap();
    let visible: i64 = connection
        .query_row("SELECT COUNT(*) FROM entries WHERE hidden = 0", [], |row| {
            row.get(0)
        })
        .unwrap();
    let hidden: i64 = connection
        .query_row("SELECT COUNT(*) FROM entries WHERE hidden = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!((visible, hidden), (4, 1));
    let continuation_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM entry_continuations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(continuation_count, 3);
    let weather: (String, f64, String) = connection
        .query_row(
            "SELECT place_name, weather_temp_c, weather_condition
             FROM plugin_entry_locations WHERE entry_uuid = 'entry_today'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        weather,
        ("Bergen".to_string(), 12.0, "Light rain".to_string())
    );
}

#[test]
fn optional_tables_absent_profile_keeps_required_projection_only() {
    let fixture = Fixture::from_profile(FixtureProfile::optional_tables_absent());
    let tables = fixture.table_names().unwrap();
    assert!(tables.iter().any(|name| name == "entries"));
    assert!(tables.iter().any(|name| name == "tags"));
    assert!(tables.iter().any(|name| name == "entry_tags"));
    assert!(tables.iter().any(|name| name == "entries_fts"));
    for optional in [
        "entry_continuations",
        "entry_thread_titles",
        "entry_thread_summaries",
        "history",
        "plugin_media_assets",
        "plugin_entry_media",
        "plugin_entry_locations",
        "plugin_location_cache",
    ] {
        assert!(
            !tables.iter().any(|name| name == optional),
            "unexpected {optional}"
        );
    }
}

#[test]
fn fts_matrix_distinguishes_fts5_legacy_and_absent() {
    for (profile, expected_table, expected_virtual) in [
        (FixtureProfile::full(), true, true),
        (FixtureProfile::legacy_fts(), true, false),
        (FixtureProfile::no_fts(), false, false),
    ] {
        let fixture = Fixture::from_profile(profile);
        let tables = fixture.table_names().unwrap();
        assert_eq!(
            tables.iter().any(|name| name == "entries_fts"),
            expected_table
        );
        if expected_table {
            let connection = fixture.connection().unwrap();
            let sql: String = connection
                .query_row(
                    "SELECT COALESCE(sql, '') FROM sqlite_master WHERE name = 'entries_fts'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                sql.to_ascii_lowercase().contains("virtual"),
                expected_virtual
            );
            assert_eq!(fixture.row_count("entries_fts").unwrap(), 5);
        }
    }
}

#[test]
fn id_matrix_exposes_missing_nullable_and_duplicate_ids_without_repair() {
    for (profile, expected_column, expected_nulls, expected_duplicates) in [
        (FixtureProfile::missing_ids(), false, 0, 0),
        (FixtureProfile::nullable_ids(), true, 5, 0),
        (FixtureProfile::duplicate_ids(), true, 0, 1),
    ] {
        let fixture = Fixture::from_profile(profile);
        let columns = fixture.table_columns("entries").unwrap();
        assert_eq!(columns.iter().any(|column| column == "id"), expected_column);
        let connection = fixture.connection().unwrap();
        if expected_column {
            let nulls: i64 = connection
                .query_row("SELECT COUNT(*) FROM entries WHERE id IS NULL", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(nulls, expected_nulls);
            let duplicate_groups: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM (
                        SELECT id FROM entries
                        WHERE id IS NOT NULL
                        GROUP BY id HAVING COUNT(*) > 1
                    )",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(duplicate_groups, expected_duplicates);
        } else {
            let rowids: i64 = connection
                .query_row("SELECT COUNT(*) FROM entries WHERE rowid > 0", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(rowids, 5);
        }
        assert_eq!(fixture.row_count("entries").unwrap(), 5);
    }
}

#[test]
fn unsupported_required_columns_are_explicitly_missing() {
    for (profile, missing) in [
        (
            FixtureProfile::unsupported_created_at(),
            UnsupportedRequiredColumn::CreatedAt,
        ),
        (
            FixtureProfile::unsupported_text(),
            UnsupportedRequiredColumn::Text,
        ),
        (
            FixtureProfile::unsupported_uuid(),
            UnsupportedRequiredColumn::Uuid,
        ),
    ] {
        assert_eq!(profile.schema, FixtureSchema::Unsupported(missing));
        let fixture = Fixture::from_profile(profile);
        let columns = fixture.table_columns("entries").unwrap();
        let missing_name = match missing {
            UnsupportedRequiredColumn::CreatedAt => "created_at",
            UnsupportedRequiredColumn::Text => "text",
            UnsupportedRequiredColumn::Uuid => "uuid",
        };
        assert!(!columns.iter().any(|column| column == missing_name));
        let present_required = REQUIRED_ENTRY_COLUMNS
            .iter()
            .filter(|column| columns.iter().any(|actual| actual == **column))
            .count();
        assert_eq!(present_required, REQUIRED_ENTRY_COLUMNS.len() - 1);
    }
}

#[test]
fn snapshots_are_reproducible_and_capture_schema_plus_rows() {
    let fixture = Fixture::from_profile(FixtureProfile::full());
    let before = fixture.snapshot().unwrap();
    assert!(!before.objects.is_empty());
    assert!(before.tables.iter().any(|table| table.name == "entries"));
    assert!(before
        .tables
        .iter()
        .find(|table| table.name == "entries")
        .unwrap()
        .rows
        .iter()
        .any(|row| row
            .iter()
            .any(|value| matches!(value, support::SqlValue::Text(text) if text == "entry_child"))));
    assert_eq!(before, fixture.snapshot().unwrap());
    fixture.assert_snapshot_unchanged(&before).unwrap();
}

#[test]
fn subprocess_environment_isolated_to_fixture_paths() {
    let fixture = Fixture::from_profile(FixtureProfile::full());
    fixture.assert_all_owned().unwrap();
    let environment = fixture.isolated_environment();
    for key in [
        "CAPSULE_DB_PATH",
        "CAPSULE_CONFIG_PATH",
        "CAPSULE_PATH_SETTINGS_PATH",
        "CAPSULE_BACKUP_DIR",
        "CAPSULE_IMAGES_MEDIA_ROOT",
        "CAPSULE_SYNC_PATH",
        "CAP_CONFIG_HOME",
        "CAPSULE_HOME",
    ] {
        let value = environment.get(OsStr::new(key)).unwrap();
        fixture.assert_owned(Path::new(value)).unwrap();
    }
    for secret in [
        "CAPSULE_GITHUB_GIST_ID",
        "CAPSULE_GITHUB_GIST_TOKEN",
        "OPENAI_API_KEY",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
    ] {
        assert!(!environment.contains_key(OsStr::new(secret)));
    }
    let result = fixture.command().arg("--help").output().unwrap();
    assert!(result.status.success());
}

#[test]
fn owned_guard_rejects_parent_traversal_and_accepts_new_children() {
    let fixture = Fixture::new();
    let inside_not_yet_created = fixture.root.path().join("future").join("output.db");
    assert!(fixture.assert_owned(&inside_not_yet_created).is_ok());

    let outside_not_yet_created = fixture.root.path().join("..").join("outside-output.db");
    assert!(fixture.assert_owned(&outside_not_yet_created).is_err());

    let parent_traversal = Path::new(".")
        .join(fixture.root.path())
        .join("..")
        .join("escape");
    assert!(fixture.assert_owned(&parent_traversal).is_err());
}

#[test]
fn seeded_clock_and_disposable_write_lock_are_deterministic() {
    let fixture = Fixture::builder(FixtureProfile::no_fts())
        .with_clock(SeededClock::new("2024-02-29 23:59:00"))
        .build();
    assert_eq!(fixture.clock.now(), "2024-02-29 23:59:00");
    assert_eq!(fixture.clock.date(), "2024-02-29");
    let lock = fixture.hold_write_lock().unwrap();
    assert!(lock.is_held());

    let connection = Connection::open_with_flags(
        &fixture.db,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    connection.busy_timeout(Duration::from_millis(20)).unwrap();
    let error = connection.execute_batch("BEGIN IMMEDIATE").unwrap_err();
    assert!(matches!(
        error,
        rusqlite::Error::SqliteFailure(ref failure, _)
            if matches!(failure.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    ));
    drop(connection);
    lock.release().unwrap();

    let connection = Connection::open(&fixture.db).unwrap();
    connection
        .execute_batch("BEGIN IMMEDIATE; ROLLBACK")
        .unwrap();
}

#[test]
fn profile_combinators_keep_matrix_dimensions_independent() {
    let profile = FixtureProfile::optional_tables_absent()
        .with_fts(FixtureFts::Legacy)
        .with_ids(FixtureIds::Nullable);
    let fixture = Fixture::from_profile(profile);
    assert_eq!(fixture.profile, profile);
    assert_eq!(fixture.row_count("entries").unwrap(), 5);
    assert!(!fixture
        .table_names()
        .unwrap()
        .iter()
        .any(|name| name == "entry_continuations"));
    assert!(fixture
        .table_names()
        .unwrap()
        .iter()
        .any(|name| name == "entries_fts"));
}
