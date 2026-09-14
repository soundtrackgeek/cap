mod support;

use capsule_core::{
    db::{LocalPathSettings, PathSource, ResolverEnvironment},
    entries::list_entries_read_only_for_database,
    models::EntryFilters,
    resolve_capsule_with_environment, ResolveRequest,
};
use support::{Fixture, FixtureProfile};

fn request(fixture: &Fixture) -> ResolveRequest {
    ResolveRequest::explicit_database(&fixture.db)
        .with_config_path(fixture.config_path())
        .with_backup_directory(fixture.backup_dir())
        .with_path_settings_path(fixture.path_settings_path())
}

#[test]
fn pinned_core_reads_full_fixture_without_repair_or_side_effects() {
    let fixture = Fixture::from_profile(FixtureProfile::full());
    let before = fixture.snapshot().unwrap();
    let resolved = resolve_capsule_with_environment(request(&fixture), Default::default()).unwrap();
    assert!(resolved.capabilities.supports_read());
    assert!(resolved.capabilities.supports_write());
    assert_eq!(resolved.database_source, PathSource::Explicit);
    assert!(resolved.database_identity.unwrap().stable_id.is_some());
    let entries =
        list_entries_read_only_for_database(&fixture.db, EntryFilters::default()).unwrap();
    assert_eq!(entries.total, 4);
    assert!(entries.entries.iter().all(|entry| !entry.hidden));
    fixture.assert_snapshot_unchanged(&before).unwrap();
    for directory in [
        fixture.backup_dir(),
        fixture.cache_dir(),
        fixture.draft_dir(),
    ] {
        assert_eq!(std::fs::read_dir(directory).unwrap().count(), 0);
    }
}

#[test]
fn explicit_missing_database_cannot_fall_through_to_valid_environment() {
    let fixture = Fixture::new();
    let missing = fixture.root.path().join("missing.db");
    let error = resolve_capsule_with_environment(
        request(&fixture).with_database_path(&missing),
        ResolverEnvironment {
            capsule_db_path: Some(fixture.db.clone()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("explicit database path"));
    assert!(!missing.exists());
}

#[test]
fn config_and_schema_failures_do_not_mutate_the_fixture() {
    let fixture = Fixture::from_profile(FixtureProfile::unsupported_uuid());
    let before = fixture.snapshot().unwrap();
    let resolved = resolve_capsule_with_environment(request(&fixture), Default::default()).unwrap();
    assert!(!resolved.capabilities.supports_read());
    assert!(!resolved.capabilities.supports_write());
    std::fs::write(fixture.config_path(), "{ malformed configuration").unwrap();
    assert!(resolve_capsule_with_environment(request(&fixture), Default::default()).is_err());
    fixture.assert_snapshot_unchanged(&before).unwrap();
}

#[test]
fn saved_appdata_binding_and_allowlisted_diagnostics_exclude_tokens() {
    let fixture = Fixture::new();
    let directory = fixture.paths().appdata.join("Capsule");
    std::fs::create_dir_all(&directory).unwrap();
    let settings = LocalPathSettings {
        database_path: Some(fixture.db.to_string_lossy().into_owned()),
        github_gist_token: Some("synthetic-token-must-never-appear".into()),
        ..Default::default()
    };
    std::fs::write(
        directory.join("path_settings.json"),
        serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    let resolved = resolve_capsule_with_environment(
        ResolveRequest::default(),
        ResolverEnvironment {
            app_data: Some(fixture.paths().appdata.clone()),
            user_home: Some(fixture.paths().profile.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(resolved.database_source, PathSource::SavedSettings);
    assert_eq!(resolved.database_path, fixture.db);
    assert!(!serde_json::to_string(&resolved)
        .unwrap()
        .contains("synthetic-token"));
}
