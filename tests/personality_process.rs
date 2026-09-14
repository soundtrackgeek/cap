mod support;

use serde_json::Value;
use support::Fixture;

fn json(fixture: &Fixture, args: &[&str]) -> Value {
    let output = fixture.command().arg("--json").args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stderr.is_empty(),
        "JSON diagnostics must stay in the envelope"
    );
    assert!(!output.stdout.contains(&0x1b));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["ok"], true);
    value
}

#[test]
fn fictional_previews_work_without_journal_or_preferences() {
    let fixture = Fixture::new();
    let before = fixture.snapshot().unwrap();
    let missing = fixture.root.path().join("absent.db");
    let invalid_preferences = fixture.paths().config_home.join("preferences.json");
    std::fs::write(&invalid_preferences, "{ deliberately invalid").unwrap();
    for args in [vec!["fx", "all"], vec!["theme", "preview", "c64"]] {
        let output = fixture
            .command()
            .args(["--json", "--db"])
            .arg(&missing)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["data"]["synthetic"], true);
        assert!(!output.stdout.contains(&0x1b));
    }
    assert!(!missing.exists());
    assert_eq!(
        std::fs::read_to_string(&invalid_preferences).unwrap(),
        "{ deliberately invalid"
    );
    fixture.assert_snapshot_unchanged(&before).unwrap();
}

#[test]
fn preferences_survive_process_restart_without_changing_capsule() {
    let fixture = Fixture::new();
    let before = fixture.snapshot().unwrap();
    let capsule_config = std::fs::read(fixture.config_path()).unwrap();
    let capsule_settings = std::fs::read(fixture.path_settings_path()).unwrap();
    assert_eq!(
        json(&fixture, &["theme", "set", "amber"])["data"]["saved"],
        true
    );
    assert_eq!(
        json(&fixture, &["config", "show"])["data"]["preferences"]["theme"],
        "amber"
    );
    json(&fixture, &["config", "set", "motion", "reduced"]);
    assert_eq!(
        json(&fixture, &["config", "show"])["data"]["preferences"]["motion"],
        "reduced"
    );
    let rejected = fixture
        .command()
        .args(["--json", "config", "set", "databasePath", "elsewhere.db"])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert_eq!(
        std::fs::read(fixture.config_path()).unwrap(),
        capsule_config
    );
    assert_eq!(
        std::fs::read(fixture.path_settings_path()).unwrap(),
        capsule_settings
    );
    fixture.assert_snapshot_unchanged(&before).unwrap();
    assert_eq!(std::fs::read_dir(fixture.backup_dir()).unwrap().count(), 0);
}

#[test]
fn output_controls_hold_at_the_process_boundary() {
    let fixture = Fixture::new();
    for flags in [
        vec!["--plain", "--color", "always", "--motion", "full"],
        vec!["--color", "never"],
        vec![],
    ] {
        let output = fixture
            .command()
            .args(flags)
            .args(["theme", "preview", "neon"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(!output.stdout.contains(&0x1b));
        assert!(String::from_utf8_lossy(&output.stdout).contains("neon"));
    }
    let quiet = fixture
        .command()
        .args(["--quiet", "fx", "all"])
        .output()
        .unwrap();
    assert!(quiet.status.success());
    assert!(quiet.stdout.is_empty());
    let no_color = fixture
        .command()
        .env("NO_COLOR", "1")
        .args(["theme", "preview", "aurora"])
        .output()
        .unwrap();
    assert!(no_color.status.success());
    assert!(!no_color.stdout.contains(&0x1b));
    let forced = fixture
        .command()
        .args([
            "--color", "always", "--motion", "off", "theme", "preview", "aurora",
        ])
        .output()
        .unwrap();
    assert!(forced.status.success());
    assert!(forced.stdout.contains(&0x1b));
}
