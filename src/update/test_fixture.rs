//! Non-default test hooks require a copied executable and all inputs below an owned lab.
use super::Available;
use serde::Deserialize;
use std::{fs, path::PathBuf};

#[derive(Deserialize)]
pub(super) struct Fixture {
    pub current_version: String,
    pub latest: Available,
    pub archive: PathBuf,
    pub checksum: String,
}

pub(super) fn load() -> Option<Fixture> {
    let root = PathBuf::from(std::env::var_os("CAP_TEST_UPDATE_ROOT")?)
        .canonicalize()
        .ok()?;
    if !root.join("CAP_TEST_LAB_MARKER").is_file()
        || !std::env::current_exe()
            .ok()?
            .canonicalize()
            .ok()?
            .starts_with(&root)
        || !crate::preferences::config_home_from_env()
            .ok()?
            .canonicalize()
            .ok()?
            .starts_with(&root)
    {
        return None;
    }
    let fixture: Fixture =
        serde_json::from_slice(&fs::read(root.join("update-fixture.json")).ok()?).ok()?;
    if !fixture.archive.canonicalize().ok()?.starts_with(root) {
        return None;
    }
    Some(fixture)
}
