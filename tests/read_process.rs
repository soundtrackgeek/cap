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
