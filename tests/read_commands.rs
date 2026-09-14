use std::fs;

use cap::{
    cli::{GlobalOptions, Pagination, ReadArgs, SearchArgs, ShowArgs},
    commands::{context, moods, recent, search, show, tags, today},
};
use rusqlite::Connection;
use tempfile::TempDir;

fn fixture() -> (TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().expect("tempdir");
    let db_path = directory.path().join("capsule.db");
    let connection = Connection::open(&db_path).expect("database");
    connection
        .execute_batch(
            "
            CREATE TABLE entries (
                id INTEGER PRIMARY KEY,
                uuid TEXT UNIQUE,
                created_at TEXT NOT NULL,
                updated_at TEXT,
                text TEXT NOT NULL,
                text_plain TEXT NOT NULL DEFAULT '',
                content_format TEXT NOT NULL DEFAULT 'plain',
                title TEXT,
                summary TEXT,
                mood TEXT,
                starred INTEGER DEFAULT 0,
                pinned INTEGER DEFAULT 0,
                hidden INTEGER DEFAULT 0
            );
            CREATE TABLE tags (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
            CREATE TABLE entry_tags (entry_id INTEGER NOT NULL, tag_id INTEGER NOT NULL);
            INSERT INTO entries
                (id, uuid, created_at, updated_at, text, text_plain, content_format, title, mood, hidden)
            VALUES
                (1, 'entry-visible', '2026-09-14 08:00', '2026-09-14 08:00', 'first line\nsecond line', 'first line\nsecond line', 'plain', 'Visible', 'calm', 0),
                (2, 'entry-hidden', '2026-09-14 09:00', '2026-09-14 09:00', 'secret note', 'secret note', 'plain', 'Hidden', 'secret', 1),
                (3, 'entry-later', '2026-09-14 10:00', '2026-09-14 10:00', 'Rust query', 'Rust query', 'plain', 'Later', 'calm', 0);
            INSERT INTO tags (id, name) VALUES (1, 'work'), (2, 'private');
            INSERT INTO entry_tags (entry_id, tag_id) VALUES (1, 1), (2, 2);
            ",
        )
        .expect("fixture schema");
    drop(connection);
    (directory, db_path)
}

fn global(db: &std::path::Path) -> GlobalOptions {
    GlobalOptions {
        db: Some(db.to_path_buf()),
        json: true,
        ..GlobalOptions::default()
    }
}

#[test]
fn read_commands_preserve_fixture_and_protect_hidden_rows() {
    let (directory, db_path) = fixture();
    let before = fs::read(&db_path).expect("snapshot");
    let global = global(&db_path);
    let recent = recent::run(
        &ReadArgs {
            page: Pagination {
                limit: 20,
                offset: 0,
            },
            include_hidden: false,
        },
        &global,
    )
    .expect("recent");
    assert_eq!(recent.data["total"], 2);
    assert!(recent.data["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| { item["uuid"] != "entry-hidden" }));
    let hidden = show::run(
        &ShowArgs {
            identifier: "2".to_string(),
            include_hidden: false,
        },
        &global,
    );
    assert!(hidden.is_err());
    let detail = show::run(
        &ShowArgs {
            identifier: "entry-visible".to_string(),
            include_hidden: false,
        },
        &global,
    )
    .expect("detail");
    assert!(detail.human.contains("first line\nsecond line"));
    assert_eq!(before, fs::read(&db_path).expect("unchanged"));
    let leftovers = fs::read_dir(directory.path())
        .expect("directory")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.contains("backup") || name.contains("cache"))
        .collect::<Vec<_>>();
    assert!(leftovers.is_empty(), "read created sidecars: {leftovers:?}");
}

#[test]
fn metadata_and_search_results_use_items_and_preserve_fallback_diagnostics() {
    let (_directory, db_path) = fixture();
    let global = global(&db_path);
    let tags = tags::run(
        &Pagination {
            limit: 20,
            offset: 0,
        },
        &global,
    )
    .expect("tags");
    assert_eq!(tags.data["items"][0]["name"], "work");
    let moods = moods::run(
        &Pagination {
            limit: 20,
            offset: 0,
        },
        &global,
    )
    .expect("moods");
    assert_eq!(moods.data["items"][0]["name"], "calm");
    let search = search::run(
        &SearchArgs {
            query: "Rust".to_string(),
            read: ReadArgs {
                page: Pagination {
                    limit: 20,
                    offset: 0,
                },
                include_hidden: false,
            },
        },
        &global,
    )
    .expect("search");
    assert_eq!(search.data["items"][0]["uuid"], "entry-later");
    assert_eq!(search.data["usedFts"], false);
}

#[test]
fn today_command_uses_local_clock_and_stays_machine_safe() {
    let (_directory, db_path) = fixture();
    let global = global(&db_path);
    let output = today::run(
        &ReadArgs {
            page: Pagination {
                limit: 1,
                offset: 0,
            },
            include_hidden: false,
        },
        &global,
    )
    .expect("today");
    assert_eq!(output.data["limit"], 1);
    assert!(!output.human.contains('\x1b'));
}

#[test]
fn context_command_uses_shared_safe_settings_and_effective_flags() {
    let (directory, db_path) = fixture();
    fs::write(
        directory.path().join("config.json"),
        br#"{"location.auto_capture":true,"location.default_location_name":"Bergen","token":"secret"}"#,
    )
    .expect("config");

    let global = global(&db_path);
    let output = context::run(&global).expect("context");
    assert_eq!(output.data["configSource"], "fallback");
    assert_eq!(output.data["configValid"], true);
    assert_eq!(output.data["defaultLocationName"], "Bergen");
    assert_eq!(output.data["requestMode"], "normal");
    assert_eq!(output.data["allowNetwork"], true);
    assert!(!output.data.to_string().contains("secret"));

    let mut offline = global.clone();
    offline.offline = true;
    let output = context::run(&offline).expect("offline context");
    assert_eq!(output.data["requestMode"], "offline");
    assert_eq!(output.data["allowNetwork"], false);
    assert_eq!(output.data["allowCache"], true);

    let mut disabled = global;
    disabled.no_context = true;
    let output = context::run(&disabled).expect("disabled context");
    assert_eq!(output.data["requestMode"], "no_context");
    assert_eq!(output.data["autoCapture"], false);
    assert_eq!(output.data["allowNetwork"], false);
    assert_eq!(output.data["allowCache"], false);
}
