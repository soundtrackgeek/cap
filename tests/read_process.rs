mod support;

use serde_json::Value;
use support::{Fixture, FixtureProfile};

fn run(fixture: &Fixture, args: &[&str], success: bool) -> Value {
    let output = fixture.command().arg("--json").args(args).output().unwrap();
    assert_eq!(
        output.status.success(),
        success,
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.stdout.contains(&0x1b));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["ok"], success);
    value
}

#[test]
fn all_read_commands_are_wired_and_leave_the_journal_unchanged() {
    let fixture = Fixture::from_profile(FixtureProfile::full());
    let before = fixture.snapshot().unwrap();
    let preferences = fixture.paths().config_home.join("preferences.json");
    std::fs::write(&preferences, "invalid presentation JSON").unwrap();
    for args in [
        vec!["show", "entry_root"],
        vec!["today"],
        vec!["recent"],
        vec!["search", "tag:work"],
        vec!["tags"],
        vec!["moods"],
        vec!["context"],
        vec!["doctor"],
    ] {
        let value = run(&fixture, &args, true);
        assert_eq!(value["command"], args[0]);
    }
    let recent = run(&fixture, &["recent", "--limit", "2"], true);
    assert_eq!(recent["data"]["total"], 4);
    assert_eq!(recent["data"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(recent["data"]["hasMore"], true);
    run(&fixture, &["show", "entry_hidden"], false);
    let hidden = run(
        &fixture,
        &["show", "entry_hidden", "--include-hidden"],
        true,
    );
    assert_eq!(hidden["data"]["uuid"], "entry_hidden");
    assert_eq!(
        std::fs::read_to_string(preferences).unwrap(),
        "invalid presentation JSON"
    );
    fixture.assert_snapshot_unchanged(&before).unwrap();
}

#[test]
fn read_commands_keep_explicit_missing_database_authoritative() {
    let fixture = Fixture::new();
    let missing = fixture.root.path().join("missing journal.db");
    let before = fixture.snapshot().unwrap();
    let output = fixture
        .command()
        .args(["--json", "--db"])
        .arg(&missing)
        .arg("recent")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert!(!missing.exists());
    fixture.assert_snapshot_unchanged(&before).unwrap();
}

#[test]
fn entry_lists_show_complete_multiline_text_without_changing_the_journal() {
    let fixture = Fixture::from_profile(FixtureProfile::full());
    let body = format!(
        "{}End of the long first paragraph.\n\nSecond paragraph: Håvard, e\u{301}, 👨‍👩‍👧‍👦 and 界.\nFinal line.",
        "A memory that needs more than a narrow preview. ".repeat(10)
    );
    let connection = rusqlite::Connection::open(&fixture.db).unwrap();
    connection
        .execute(
            "UPDATE entries SET created_at = ?1 || printf(' %02d:00:00', id)",
            [chrono::Local::now().format("%Y-%m-%d").to_string()],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE entries SET text = ?1, text_plain = ?1 WHERE uuid = 'entry_root'",
            [&body],
        )
        .unwrap();
    drop(connection);
    let before = fixture.snapshot().unwrap();
    let without_whitespace = |text: &str| {
        text.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
    };
    for args in [
        vec!["today"],
        vec!["recent"],
        vec!["search", "tag:personal"],
    ] {
        for plain in [false, true] {
            let mut command = fixture.command();
            if plain {
                command.arg("--plain");
            }
            let output = command.args(&args).output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(
                without_whitespace(&text).contains(&without_whitespace(&body)),
                "{args:?}: {text}"
            );
            assert!(text.contains("\n  \n  Second paragraph"), "{text}");
            assert!(!text.contains('\x1b'));
        }
        let json = run(&fixture, &args, true);
        let entry = json["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["uuid"] == "entry_root")
            .unwrap();
        assert_eq!(entry["textPlain"], body);
    }
    fixture.assert_snapshot_unchanged(&before).unwrap();
}
