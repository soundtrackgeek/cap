mod support;

use std::fs;
use std::sync::Mutex;

use cap::{
    cli::{Command, GlobalOptions, Period},
    commands::{calendar, garden, on_this_day, recall, stats},
};
use capsule_core::db::FileIdentity;
use capsule_core::stats::NaiveDate;
use rusqlite::Connection;
use support::{Fixture, FixtureProfile};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn global(db: &std::path::Path) -> GlobalOptions {
    GlobalOptions {
        db: Some(db.to_path_buf()),
        json: true,
        plain: true,
        ..GlobalOptions::default()
    }
}

#[test]
fn memory_commands_use_full_fixture_and_leave_journal_bytes_unchanged() {
    let fixture = Fixture::from_profile(FixtureProfile::full());
    let before = fs::read(&fixture.db).expect("fixture snapshot");
    let global = global(&fixture.db);
    let date = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();

    let calendar = calendar::run_at(date, false, &global).expect("calendar");
    assert_eq!(calendar.data["activeDays"], 4);
    assert_eq!(calendar.data["totalEntries"], 4);
    assert!(calendar.data["days"].as_array().unwrap().len() >= 14);

    let monthly = stats::run_at(Period::Month, date, false, &global).expect("stats");
    assert_eq!(monthly.data["totalEntries"], 4);
    assert_eq!(monthly.data["activeDays"], 4);
    assert!(monthly.data["currentStreakDays"].as_i64().unwrap() >= 1);

    let garden = garden::run_at(date, false, &global).expect("garden");
    assert_eq!(garden.data["days"].as_array().unwrap().len(), 7);
    assert_eq!(garden.data["days"][6]["growth"], "seed");
    assert!(garden.human.contains("Legend:"));

    let on_day = on_this_day::run_at(date, 20, 0, false, &global).expect("on this day");
    assert_eq!(on_day.data["items"].as_array().unwrap().len(), 0);
    assert!(on_day.human.contains("No entries from earlier years"));

    assert_eq!(before, fs::read(&fixture.db).expect("fixture unchanged"));
}

#[test]
fn hidden_rows_are_opt_in_and_legacy_fts_remains_readable() {
    let fixture = Fixture::from_profile(FixtureProfile::legacy_fts());
    let global = global(&fixture.db);
    let date = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();

    let visible = stats::run_at(Period::Year, date, false, &global).expect("visible stats");
    let hidden = stats::run_at(Period::Year, date, true, &global).expect("hidden stats");
    assert_eq!(visible.data["totalEntries"], 4);
    assert_eq!(hidden.data["totalEntries"], 5);

    let on_day = on_this_day::run_at(date, 20, 0, true, &global).expect("on this day");
    assert!(on_day.data["items"].as_array().unwrap().is_empty());
}

#[test]
fn recall_is_non_repeating_with_synthetic_cap_state() {
    let _guard = ENV_LOCK.lock().unwrap();
    let fixture = Fixture::from_profile(FixtureProfile::full());
    let state_dir = fixture.root.path().join("cap-state");
    let old = std::env::var_os("CAP_CONFIG_HOME");
    std::env::set_var("CAP_CONFIG_HOME", &state_dir);
    let global = global(&fixture.db);
    let command = Command::Recall {
        tag: None,
        include_hidden: false,
    };
    let first = recall::run(&command, &global).expect("first recall");
    let second = recall::run(&command, &global).expect("second recall");
    let first_uuid = first.data["entry"]["uuid"].as_str().expect("first uuid");
    let second_uuid = second.data["entry"]["uuid"].as_str().expect("second uuid");
    assert_ne!(first_uuid, second_uuid);
    assert!(state_dir.join("memory-state.json").is_file());
    match old {
        Some(value) => std::env::set_var("CAP_CONFIG_HOME", value),
        None => std::env::remove_var("CAP_CONFIG_HOME"),
    }
}

#[test]
fn milestone_receipt_requires_a_real_crossing_and_is_once_only() {
    let fixture = Fixture::from_profile(FixtureProfile::full());
    let words = std::iter::repeat_n("word", 50)
        .collect::<Vec<_>>()
        .join(" ");
    let connection = Connection::open(&fixture.db).expect("fixture db");
    connection
        .execute(
            "INSERT INTO entries
             (uuid, created_at, updated_at, text, text_plain, content_format, hidden)
             VALUES ('entry-crossing', '2026-09-14 11:00:00', '2026-09-14 11:00:00', ?1, ?1, 'plain', 0)",
            [&words],
        )
        .expect("crossing entry");
    drop(connection);
    let state = cap::insights::MemoryStateStore::at_dir(&fixture.root.path().join("state"));
    let identity = FileIdentity::for_path(&fixture.db);
    let today = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();
    let first = state.milestone_receipt_for_committed(
        &fixture.db,
        &identity,
        today,
        "entry-crossing",
        false,
    );
    assert_eq!(first.glints.len(), 1);
    assert_eq!(first.glints[0].kind, "daily_words");
    let second = state.milestone_receipt_for_committed(
        &fixture.db,
        &identity,
        today,
        "entry-crossing",
        false,
    );
    assert!(second.glints.is_empty());

    // A later one-word save does not retrigger the already-crossed threshold.
    let connection = Connection::open(&fixture.db).expect("fixture db");
    connection
        .execute(
            "INSERT INTO entries
             (uuid, created_at, updated_at, text, text_plain, content_format, hidden)
             VALUES ('entry-after', '2026-09-14 11:01:00', '2026-09-14 11:01:00', 'one', 'one', 'plain', 0)",
            [],
        )
        .expect("after entry");
    drop(connection);
    let third =
        state.milestone_receipt_for_committed(&fixture.db, &identity, today, "entry-after", false);
    assert!(third.glints.is_empty());
}
