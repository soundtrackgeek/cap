mod support;

#[test]
fn fixture_and_command_are_isolated() {
    let fixture = support::Fixture::new();
    fixture.assert_owned(&fixture.db).unwrap();
    let other = tempfile::tempdir().unwrap();
    assert!(fixture.assert_owned(other.path()).is_err());
    let result = fixture.command().arg("--help").output().unwrap();
    assert!(result.status.success());
    let connection = rusqlite::Connection::open(&fixture.db).unwrap();
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn output_envelope_has_stable_success_and_error_fields() {
    use cap::contracts::{CliError, OutputEnvelope};
    let success =
        OutputEnvelope::success("show", serde_json::json!({"entryUuid":"entry_fixture1"}));
    let mut out = vec![];
    cap::output::write_json(&mut out, &success).unwrap();
    let object: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(object["schemaVersion"], 1);
    assert!(object["error"].is_null());
    let error =
        OutputEnvelope::<()>::failure("add", CliError::new("DB_BUSY", "Database is busy", true));
    let object = serde_json::to_value(error).unwrap();
    assert_eq!(object["ok"], false);
    assert_eq!(object["error"]["code"], "DB_BUSY");
    assert!(object["data"].is_null());
}
