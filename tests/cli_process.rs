mod support;

use std::process::{Command, Stdio};
use support::Fixture;
fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cap"))
}

#[test]
fn unknown_commands_show_full_help_without_creating_entries_or_recovery_state() {
    let fixture = Fixture::new();
    let before = fixture.snapshot().unwrap();
    let help = fixture.command().arg("--help").output().unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for args in [
        vec!["test"],
        vec!["Had", "a", "lovely", "walk"],
        vec!["--", "today", "was wonderful"],
        vec!["--json", "test"],
        vec!["test", "--json"],
    ] {
        let output = fixture.command().args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "args: {args:?}");
        let message = if args.contains(&"--json") {
            assert!(output.stderr.is_empty());
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["ok"], false);
            assert_eq!(value["error"]["code"], "INVALID_INPUT");
            value["error"]["message"].as_str().unwrap().to_owned()
        } else {
            assert!(output.stdout.is_empty());
            String::from_utf8(output.stderr).unwrap()
        };
        if args.contains(&"--") {
            assert!(message.contains("unexpected argument"), "{message}");
        } else {
            assert!(message.contains("Command not recognized"), "{message}");
            assert!(
                message.ends_with(&help),
                "help differs for {args:?}: {message}"
            );
        }
        fixture.assert_snapshot_unchanged(&before).unwrap();
        assert_eq!(std::fs::read_dir(fixture.backup_dir()).unwrap().count(), 0);
        assert_eq!(
            std::fs::read_dir(&fixture.paths().state).unwrap().count(),
            0
        );
    }
}

#[test]
fn explicit_add_saves_the_same_word_as_an_entry() {
    let fixture = Fixture::new();
    let before_rows = fixture.row_count("entries").unwrap();
    let added = fixture
        .command()
        .args(["--json", "--no-context", "add", "test"])
        .output()
        .unwrap();
    assert!(added.status.success());
    assert_eq!(fixture.row_count("entries").unwrap(), before_rows + 1);
    let value: serde_json::Value = serde_json::from_slice(&added.stdout).unwrap();
    let shown = fixture
        .command()
        .args([
            "--json",
            "show",
            value["data"]["entryUuid"].as_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(shown.status.success());
    let entry: serde_json::Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(entry["data"]["text"], "test");
}

#[test]
fn bare_cap_shows_help_without_reading_stdin_or_starting_a_draft() {
    let fixture = Fixture::new();
    let before = fixture.snapshot().unwrap();
    let help = fixture.command().arg("--help").output().unwrap();
    let input = fixture.root.path().join("piped-entry.txt");
    std::fs::write(&input, b"Do not save this piped entry").unwrap();
    for args in [vec![], vec!["--json"], vec!["--dry-run"]] {
        let output = fixture
            .command()
            .args(&args)
            .stdin(Stdio::from(std::fs::File::open(&input).unwrap()))
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        if args.contains(&"--json") {
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["command"], "help");
            assert_eq!(
                value["data"]["text"],
                String::from_utf8_lossy(&help.stdout).as_ref()
            );
        } else {
            assert_eq!(output.stdout, help.stdout);
        }
        fixture.assert_snapshot_unchanged(&before).unwrap();
        assert_eq!(std::fs::read_dir(fixture.backup_dir()).unwrap().count(), 0);
        assert_eq!(
            std::fs::read_dir(&fixture.paths().state).unwrap().count(),
            0
        );
    }
}

#[test]
fn json_help_and_parse_errors_are_single_plain_objects() {
    for args in [
        vec!["--json", "--help"],
        vec!["--json", "doctor", "nonsense"],
        vec!["--json", "--unknown"],
    ] {
        let output = command().args(&args).output().unwrap();
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert!(!output.stdout.contains(&0x1b));
        assert!(!output.stderr.contains(&0x1b));
        if args.contains(&"--help") {
            assert!(output.status.success());
        } else {
            assert_eq!(output.status.code(), Some(2));
        }
    }
}

#[test]
fn dry_run_on_read_is_usage_error() {
    let output = command()
        .args(["--json", "--dry-run", "doctor"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["code"], "INVALID_INPUT");
}

#[test]
fn human_usage_errors_cannot_inject_terminal_controls() {
    let output = command()
        .arg("--unknown\u{1b}]52;c;AAAA\u{7}")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!output.stderr.contains(&0x1b));
    assert!(!output.stderr.contains(&0x07));
}
#[test]
fn version_identifies_shared_capsule_revision() {
    let output = command().args(["--json", "--version"]).output().unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["data"]["text"]
        .as_str()
        .unwrap()
        .contains("Capsule core"));
    assert!(value["data"]["text"]
        .as_str()
        .unwrap()
        .contains(&env!("CAP_CORE_REVISION")[..12]));
}
