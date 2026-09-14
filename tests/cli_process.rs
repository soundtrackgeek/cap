use std::process::Command;
fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cap"))
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
