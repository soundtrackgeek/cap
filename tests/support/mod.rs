use std::path::{Path, PathBuf};

pub struct Fixture {
    pub root: tempfile::TempDir,
    pub db: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary fixture directory");
        let db = root.path().join("capsule.db");
        let connection = rusqlite::Connection::open(&db).expect("fixture DB");
        connection
            .execute_batch(include_str!("../fixtures/capsule.sql"))
            .expect("synthetic schema");
        std::fs::write(
            root.path().join("config.json"),
            r#"{"location.auto_capture":false}"#,
        )
        .unwrap();
        std::fs::write(root.path().join("path_settings.json"), "{}").unwrap();
        Self { root, db }
    }

    pub fn assert_owned(&self, path: &Path) -> Result<(), &'static str> {
        let root = self
            .root
            .path()
            .canonicalize()
            .map_err(|_| "fixture root missing")?;
        let resolved = path.canonicalize().map_err(|_| "target missing")?;
        if resolved.starts_with(root) {
            Ok(())
        } else {
            Err("refusing a target outside this fixture")
        }
    }

    pub fn command(&self) -> std::process::Command {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_cap"));
        command
            .env("CAPSULE_DB_PATH", &self.db)
            .env(
                "CAPSULE_PATH_SETTINGS_PATH",
                self.root.path().join("path_settings.json"),
            )
            .env("CAPSULE_CONFIG_PATH", self.root.path().join("config.json"))
            .env("CAPSULE_BACKUP_DIR", self.root.path().join("backups"))
            .env("CAP_CONFIG_HOME", self.root.path().join("cap-state"))
            .env("CAPSULE_HOME", self.root.path())
            .env_remove("CAPSULE_SYNC_PATH")
            .env_remove("CAPSULE_GITHUB_GIST_TOKEN");
        command
    }
}
