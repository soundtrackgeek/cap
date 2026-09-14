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
