#![allow(dead_code)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

const DEFAULT_NOW: &str = "2026-09-14 12:00:00";
const SENSITIVE_ENVIRONMENT: &[&str] = &[
    "ALL_PROXY",
    "CAPSULE_GITHUB_GIST_ID",
    "CAPSULE_GITHUB_GIST_TOKEN",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "OPENAI_API_KEY",
];

/// The required entry columns used by Capsule's read projection.
pub const REQUIRED_ENTRY_COLUMNS: &[&str] = &[
    "id",
    "uuid",
    "created_at",
    "updated_at",
    "text",
    "text_plain",
    "content_format",
    "title",
    "summary",
    "mood",
    "starred",
    "pinned",
    "hidden",
];

/// A deliberately small, source-grounded schema matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixtureSchema {
    Full,
    OptionalTablesAbsent,
    Unsupported(UnsupportedRequiredColumn),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnsupportedRequiredColumn {
    CreatedAt,
    Text,
    Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixtureFts {
    Fts5,
    Legacy,
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixtureIds {
    Stable,
    MissingColumn,
    Nullable,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureProfile {
    pub schema: FixtureSchema,
    pub fts: FixtureFts,
    pub ids: FixtureIds,
}

impl FixtureProfile {
    pub const fn full() -> Self {
        Self {
            schema: FixtureSchema::Full,
            fts: FixtureFts::Fts5,
            ids: FixtureIds::Stable,
        }
    }

    pub const fn optional_tables_absent() -> Self {
        Self {
            schema: FixtureSchema::OptionalTablesAbsent,
            ..Self::full()
        }
    }

    pub const fn legacy_fts() -> Self {
        Self {
            fts: FixtureFts::Legacy,
            ..Self::full()
        }
    }

    pub const fn no_fts() -> Self {
        Self {
            fts: FixtureFts::Absent,
            ..Self::full()
        }
    }

    pub const fn missing_ids() -> Self {
        Self {
            ids: FixtureIds::MissingColumn,
            ..Self::full()
        }
    }

    pub const fn nullable_ids() -> Self {
        Self {
            ids: FixtureIds::Nullable,
            ..Self::full()
        }
    }

    pub const fn duplicate_ids() -> Self {
        Self {
            ids: FixtureIds::Duplicate,
            ..Self::full()
        }
    }

    pub const fn unsupported_created_at() -> Self {
        Self {
            schema: FixtureSchema::Unsupported(UnsupportedRequiredColumn::CreatedAt),
            ..Self::full()
        }
    }

    pub const fn unsupported_text() -> Self {
        Self {
            schema: FixtureSchema::Unsupported(UnsupportedRequiredColumn::Text),
            ..Self::full()
        }
    }

    pub const fn unsupported_uuid() -> Self {
        Self {
            schema: FixtureSchema::Unsupported(UnsupportedRequiredColumn::Uuid),
            ..Self::full()
        }
    }

    pub const fn with_schema(self, schema: FixtureSchema) -> Self {
        Self { schema, ..self }
    }

    pub const fn with_fts(self, fts: FixtureFts) -> Self {
        Self { fts, ..self }
    }

    pub const fn with_ids(self, ids: FixtureIds) -> Self {
        Self { ids, ..self }
    }

    pub fn label(self) -> String {
        format!(
            "schema={:?};fts={:?};ids={:?}",
            self.schema, self.fts, self.ids
        )
    }
}

impl Default for FixtureProfile {
    fn default() -> Self {
        Self::full()
    }
}

/// A deterministic request clock used to document date-sensitive fixtures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeededClock {
    now: String,
}

impl SeededClock {
    pub fn new(now: impl Into<String>) -> Self {
        Self { now: now.into() }
    }

    pub fn now(&self) -> &str {
        &self.now
    }

    pub fn date(&self) -> &str {
        self.now.get(..10).unwrap_or(&self.now)
    }
}

impl Default for SeededClock {
    fn default() -> Self {
        Self::new(DEFAULT_NOW)
    }
}

#[derive(Debug, Clone)]
pub struct FixtureBuilder {
    profile: FixtureProfile,
    clock: SeededClock,
    entry_count: usize,
}

impl FixtureBuilder {
    pub fn new(profile: FixtureProfile) -> Self {
        Self {
            profile,
            clock: SeededClock::default(),
            entry_count: SEED_ENTRIES.len(),
        }
    }

    pub fn with_clock(mut self, clock: SeededClock) -> Self {
        self.clock = clock;
        self
    }

    pub fn with_entry_count(mut self, entry_count: usize) -> Self {
        self.entry_count = entry_count.min(SEED_ENTRIES.len());
        self
    }

    pub fn build(self) -> Fixture {
        let root = tempfile::tempdir().expect("temporary fixture directory");
        let db = root.path().join("capsule.db");
        let paths = FixturePaths::for_root(root.path());
        paths.create_directories().expect("fixture directories");

        let connection = Connection::open(&db).expect("fixture DB");
        connection
            .execute_batch(&schema_sql(self.profile))
            .expect("synthetic schema");
        seed_entries(&connection, self.profile, self.entry_count).expect("synthetic entries");
        seed_relations(&connection, self.profile, self.entry_count).expect("synthetic relations");
        seed_fts(&connection, self.profile).expect("synthetic FTS");
        drop(connection);

        paths.write_metadata(&self.clock).expect("fixture metadata");

        let fixture = Fixture {
            root,
            db,
            profile: self.profile,
            clock: self.clock,
            paths,
        };
        fixture.assert_all_owned().expect("fixture paths are owned");
        fixture
    }
}

/// A disposable Capsule database and all paths a subprocess may write.
pub struct Fixture {
    pub root: tempfile::TempDir,
    pub db: PathBuf,
    pub profile: FixtureProfile,
    pub clock: SeededClock,
    paths: FixturePaths,
}

impl Fixture {
    /// Preserve the original foundation helper: full schema, FTS5 and stable IDs.
    pub fn new() -> Self {
        FixtureBuilder::new(FixtureProfile::full())
            .with_entry_count(2)
            .build()
    }

    pub fn from_profile(profile: FixtureProfile) -> Self {
        FixtureBuilder::new(profile).build()
    }

    pub fn with_clock(clock: SeededClock) -> Self {
        FixtureBuilder::new(FixtureProfile::full())
            .with_clock(clock)
            .build()
    }

    pub fn builder(profile: FixtureProfile) -> FixtureBuilder {
        FixtureBuilder::new(profile)
    }

    pub fn paths(&self) -> &FixturePaths {
        &self.paths
    }

    pub fn config_path(&self) -> &Path {
        &self.paths.config
    }

    pub fn path_settings_path(&self) -> &Path {
        &self.paths.path_settings
    }

    pub fn backup_dir(&self) -> &Path {
        &self.paths.backups
    }

    pub fn cache_dir(&self) -> &Path {
        &self.paths.cache
    }

    pub fn draft_dir(&self) -> &Path {
        &self.paths.drafts
    }

    pub fn media_root(&self) -> &Path {
        &self.paths.media
    }

    pub fn sync_dir(&self) -> &Path {
        &self.paths.sync
    }

    /// Resolve an existing or not-yet-created path and require it to stay below the fixture root.
    /// Existing symlinks are canonicalized, so a link escaping the root is rejected.
    pub fn assert_owned(&self, path: &Path) -> Result<(), &'static str> {
        let root = self
            .root
            .path()
            .canonicalize()
            .map_err(|_| "fixture root missing")?;
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|_| "current directory unavailable")?
                .join(path)
        };

        let mut existing = absolute.as_path();
        let mut missing_components = Vec::new();
        while !existing.exists() {
            let name = existing
                .file_name()
                .ok_or("target has no parent")?
                .to_os_string();
            missing_components.push(name);
            existing = existing.parent().ok_or("target parent missing")?;
        }

        let mut resolved = existing.canonicalize().map_err(|_| "target missing")?;
        for component in missing_components.iter().rev() {
            resolved.push(component);
        }
        if resolved.starts_with(&root) {
            Ok(())
        } else {
            Err("refusing a target outside this fixture")
        }
    }

    pub fn assert_all_owned(&self) -> Result<(), &'static str> {
        self.assert_owned(&self.db)?;
        for path in self.paths.all_paths() {
            self.assert_owned(path)?;
        }
        Ok(())
    }

    /// Return the exact environment allowlist used by [`Fixture::command`].
    pub fn isolated_environment(&self) -> BTreeMap<OsString, OsString> {
        let root = self.root.path();
        let mut values = BTreeMap::new();
        for key in ["PATH", "PATHEXT", "SYSTEMROOT", "WINDIR"] {
            if let Some(value) = std::env::var_os(key) {
                values.insert(OsString::from(key), value);
            }
        }

        let mut add_path = |key: &str, path: &Path| {
            values.insert(OsString::from(key), path.as_os_str().to_os_string());
        };
        add_path("APPDATA", &self.paths.appdata);
        add_path("CAP_CONFIG_HOME", &self.paths.state);
        add_path("CAPSULE_BACKUP_DIR", &self.paths.backups);
        add_path("CAPSULE_CONFIG_PATH", &self.paths.config);
        add_path("CAPSULE_DB_PATH", &self.db);
        add_path("CAPSULE_HOME", root);
        add_path("CAPSULE_IMAGES_MEDIA_ROOT", &self.paths.media);
        add_path("CAPSULE_PATH_SETTINGS_PATH", &self.paths.path_settings);
        add_path("CAPSULE_SYNC_PATH", &self.paths.sync);
        add_path("HOME", &self.paths.home);
        add_path("LOCALAPPDATA", &self.paths.localappdata);
        add_path("TEMP", &self.paths.temp);
        add_path("TMP", &self.paths.temp);
        add_path("TMPDIR", &self.paths.temp);
        add_path("USERPROFILE", &self.paths.profile);
        add_path("XDG_CACHE_HOME", &self.paths.cache);
        add_path("XDG_CONFIG_HOME", &self.paths.config_home);
        values
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cap"));
        command.env_clear();
        for (key, value) in self.isolated_environment() {
            command.env(key, value);
        }
        for key in SENSITIVE_ENVIRONMENT {
            command.env_remove(key);
        }
        command
    }

    pub fn connection(&self) -> Result<Connection, String> {
        Connection::open_with_flags(&self.db, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| format!("open fixture read-only: {error}"))
    }

    pub fn table_names(&self) -> Result<Vec<String>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .map_err(|error| format!("prepare table list: {error}"))?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("query table list: {error}"))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| format!("read table list: {error}"));
        names
    }

    pub fn table_columns(&self, table: &str) -> Result<Vec<String>, String> {
        let connection = self.connection()?;
        let sql = format!("PRAGMA table_info({})", quote_identifier(table));
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| format!("prepare columns for {table}: {error}"))?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|error| format!("query columns for {table}: {error}"))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| format!("read columns for {table}: {error}"));
        columns
    }

    pub fn row_count(&self, table: &str) -> Result<i64, String> {
        let connection = self.connection()?;
        let sql = format!("SELECT COUNT(*) FROM {}", quote_identifier(table));
        connection
            .query_row(&sql, [], |row| row.get::<_, i64>(0))
            .map_err(|error| format!("count {table}: {error}"))
    }

    /// Capture schema objects and sorted logical rows, including SQLite shadow tables.
    pub fn snapshot(&self) -> Result<LogicalSnapshot, String> {
        let connection = self.connection()?;
        logical_snapshot(&connection)
    }

    pub fn assert_snapshot_unchanged(&self, before: &LogicalSnapshot) -> Result<(), String> {
        let after = self.snapshot()?;
        if before == &after {
            Ok(())
        } else {
            Err("fixture changed during read-only operation (before != after)".to_string())
        }
    }

    /// Hold a write transaction until the guard is dropped or explicitly released.
    pub fn hold_write_lock(&self) -> Result<WriteLock, String> {
        self.assert_owned(&self.db)
            .map_err(|error| error.to_string())?;
        let connection = Connection::open_with_flags(
            &self.db,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| format!("open fixture lock connection: {error}"))?;
        connection
            .busy_timeout(Duration::from_millis(50))
            .map_err(|error| format!("set lock timeout: {error}"))?;
        connection
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|error| format!("hold fixture write lock: {error}"))?;
        Ok(WriteLock {
            connection: Some(connection),
        })
    }
}

pub struct WriteLock {
    connection: Option<Connection>,
}

impl WriteLock {
    pub fn is_held(&self) -> bool {
        self.connection.is_some()
    }

    pub fn release(mut self) -> Result<(), String> {
        if let Some(connection) = self.connection.take() {
            connection
                .execute_batch("ROLLBACK")
                .map_err(|error| format!("release fixture write lock: {error}"))?;
        }
        Ok(())
    }
}

impl Drop for WriteLock {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            let _ = connection.execute_batch("ROLLBACK");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogicalSnapshot {
    pub objects: Vec<SchemaObjectSnapshot>,
    pub tables: Vec<TableSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaObjectSnapshot {
    pub object_type: String,
    pub name: String,
    pub table_name: Option<String>,
    pub sql: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableSnapshot {
    pub name: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<SqlValue>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(String),
    Text(String),
    Blob(Vec<u8>),
}

fn logical_snapshot(connection: &Connection) -> Result<LogicalSnapshot, String> {
    let mut objects_statement = connection
        .prepare(
            "SELECT type, name, tbl_name, sql
             FROM sqlite_master
             ORDER BY type, name COLLATE NOCASE",
        )
        .map_err(|error| format!("prepare schema snapshot: {error}"))?;
    let objects = objects_statement
        .query_map([], |row| {
            Ok(SchemaObjectSnapshot {
                object_type: row.get(0)?,
                name: row.get(1)?,
                table_name: row.get(2)?,
                sql: row.get(3)?,
            })
        })
        .map_err(|error| format!("query schema snapshot: {error}"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("read schema snapshot: {error}"))?;

    let table_names = objects
        .iter()
        .filter(|object| object.object_type == "table")
        .map(|object| object.name.clone())
        .collect::<Vec<_>>();
    let tables = table_names
        .iter()
        .map(|name| table_snapshot(connection, name))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(LogicalSnapshot { objects, tables })
}

fn table_snapshot(connection: &Connection, name: &str) -> Result<TableSnapshot, String> {
    let columns = {
        let sql = format!("PRAGMA table_info({})", quote_identifier(name));
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| format!("prepare table info for {name}: {error}"))?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|error| format!("query table info for {name}: {error}"))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| format!("read table info for {name}: {error}"))?;
        columns
    };
    let sql = format!("SELECT * FROM {}", quote_identifier(name));
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| format!("prepare rows for {name}: {error}"))?;
    let column_count = statement.column_count();
    let mut rows = statement
        .query([])
        .map_err(|error| format!("query rows for {name}: {error}"))?;
    let mut values = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| format!("read rows for {name}: {error}"))?
    {
        let mut row_values = Vec::with_capacity(column_count);
        for index in 0..column_count {
            row_values
                .push(sql_value(row.get_ref(index).map_err(|error| {
                    format!("read value {name}[{index}]: {error}")
                })?)?);
        }
        values.push(row_values);
    }
    values.sort_by_key(|row| format!("{row:?}"));
    Ok(TableSnapshot {
        name: name.to_string(),
        columns,
        rows: values,
    })
}

fn sql_value(value: ValueRef<'_>) -> Result<SqlValue, String> {
    match value {
        ValueRef::Null => Ok(SqlValue::Null),
        ValueRef::Integer(value) => Ok(SqlValue::Integer(value)),
        ValueRef::Real(value) => Ok(SqlValue::Real(value.to_string())),
        ValueRef::Text(value) => String::from_utf8(value.to_vec())
            .map(SqlValue::Text)
            .map_err(|error| format!("invalid UTF-8 text in fixture snapshot: {error}")),
        ValueRef::Blob(value) => Ok(SqlValue::Blob(value.to_vec())),
    }
}

#[derive(Debug, Clone)]
pub struct FixturePaths {
    pub config: PathBuf,
    pub path_settings: PathBuf,
    pub backups: PathBuf,
    pub cache: PathBuf,
    pub drafts: PathBuf,
    pub media: PathBuf,
    pub sync: PathBuf,
    pub state: PathBuf,
    pub temp: PathBuf,
    pub appdata: PathBuf,
    pub home: PathBuf,
    pub profile: PathBuf,
    pub localappdata: PathBuf,
    pub config_home: PathBuf,
    pub clock: PathBuf,
}

impl FixturePaths {
    fn for_root(root: &Path) -> Self {
        Self {
            config: root.join("config.json"),
            path_settings: root.join("path_settings.json"),
            backups: root.join("backups"),
            cache: root.join("cache"),
            drafts: root.join("drafts"),
            media: root.join("media"),
            sync: root.join("sync"),
            state: root.join("cap-state"),
            temp: root.join("temp"),
            appdata: root.join("appdata"),
            home: root.join("home"),
            profile: root.join("profile"),
            localappdata: root.join("localappdata"),
            config_home: root.join("config-home"),
            clock: root.join("fixture-clock.json"),
        }
    }

    fn create_directories(&self) -> std::io::Result<()> {
        for path in [
            &self.backups,
            &self.cache,
            &self.drafts,
            &self.media,
            &self.sync,
            &self.state,
            &self.temp,
            &self.appdata,
            &self.home,
            &self.profile,
            &self.localappdata,
            &self.config_home,
        ] {
            std::fs::create_dir_all(path)?;
        }
        Ok(())
    }

    fn write_metadata(&self, clock: &SeededClock) -> std::io::Result<()> {
        std::fs::write(
            &self.config,
            "{\n  \"location.auto_capture\": false,\n  \"location.use_default_location\": true,\n  \"location.default_location_name\": \"Fixture Harbor\",\n  \"location.weather_provider\": \"open_meteo\"\n}\n",
        )?;
        std::fs::write(
            &self.path_settings,
            "{\n  \"database_path\": null,\n  \"backup_directory\": null\n}\n",
        )?;
        let mut clock_json = serde_json::to_vec_pretty(clock).expect("clock JSON");
        clock_json.push(b'\n');
        std::fs::write(&self.clock, clock_json)?;
        Ok(())
    }

    fn all_paths(&self) -> [&Path; 15] {
        [
            self.config.as_path(),
            self.path_settings.as_path(),
            self.backups.as_path(),
            self.cache.as_path(),
            self.drafts.as_path(),
            self.media.as_path(),
            self.sync.as_path(),
            self.state.as_path(),
            self.temp.as_path(),
            self.appdata.as_path(),
            self.home.as_path(),
            self.profile.as_path(),
            self.localappdata.as_path(),
            self.config_home.as_path(),
            self.clock.as_path(),
        ]
    }
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn sql_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn sql_optional(value: Option<&str>) -> String {
    value.map(sql_text).unwrap_or_else(|| "NULL".to_string())
}

fn entries_columns(profile: FixtureProfile) -> Vec<&'static str> {
    let mut columns = Vec::new();
    if !matches!(profile.ids, FixtureIds::MissingColumn) {
        columns.push("id");
    }
    if !matches!(
        profile.schema,
        FixtureSchema::Unsupported(UnsupportedRequiredColumn::Uuid)
    ) {
        columns.push("uuid");
    }
    if !matches!(
        profile.schema,
        FixtureSchema::Unsupported(UnsupportedRequiredColumn::CreatedAt)
    ) {
        columns.push("created_at");
    }
    columns.extend([
        "updated_at",
        "text",
        "text_plain",
        "content_format",
        "title",
        "summary",
        "mood",
        "starred",
        "pinned",
        "hidden",
    ]);
    if matches!(
        profile.schema,
        FixtureSchema::Unsupported(UnsupportedRequiredColumn::Text)
    ) {
        columns.retain(|column| *column != "text");
    }
    columns
}

fn entry_insert_columns(profile: FixtureProfile) -> Vec<&'static str> {
    entries_columns(profile)
        .into_iter()
        .filter(|column| !matches!((profile.ids, *column), (FixtureIds::Stable, "id")))
        .collect()
}

fn schema_sql(profile: FixtureProfile) -> String {
    let mut sql = String::new();
    let id_definition = match profile.ids {
        FixtureIds::Stable => "id INTEGER PRIMARY KEY AUTOINCREMENT",
        FixtureIds::MissingColumn => "",
        FixtureIds::Nullable | FixtureIds::Duplicate => "id INTEGER",
    };
    let entries = entries_columns(profile)
        .into_iter()
        .map(|column| match column {
            "id" => id_definition.to_string(),
            "uuid" => "uuid TEXT UNIQUE".to_string(),
            "created_at" => "created_at TEXT NOT NULL".to_string(),
            "updated_at" => "updated_at TEXT".to_string(),
            "text" => "text TEXT NOT NULL".to_string(),
            "text_plain" => "text_plain TEXT NOT NULL DEFAULT ''".to_string(),
            "content_format" => "content_format TEXT NOT NULL DEFAULT 'plain'".to_string(),
            "title" => "title TEXT".to_string(),
            "summary" => "summary TEXT".to_string(),
            "mood" => "mood TEXT".to_string(),
            "starred" => "starred INTEGER DEFAULT 0".to_string(),
            "pinned" => "pinned INTEGER DEFAULT 0".to_string(),
            "hidden" => "hidden INTEGER DEFAULT 0".to_string(),
            _ => unreachable!("known entry column"),
        })
        .filter(|column| !column.is_empty())
        .collect::<Vec<_>>()
        .join(",\n    ");
    sql.push_str(&format!("CREATE TABLE entries (\n    {entries}\n);\n"));
    sql.push_str(
        "CREATE TABLE tags (\n    id INTEGER PRIMARY KEY AUTOINCREMENT,\n    name TEXT NOT NULL UNIQUE\n);\nCREATE TABLE entry_tags (\n    entry_id INTEGER NOT NULL,\n    tag_id INTEGER NOT NULL,\n    PRIMARY KEY (entry_id, tag_id)\n);\n",
    );

    if matches!(profile.schema, FixtureSchema::Full) {
        sql.push_str(
            "CREATE TABLE entry_continuations (\n    child_entry_uuid TEXT PRIMARY KEY,\n    parent_entry_uuid TEXT NOT NULL,\n    updated_at TEXT\n);\nCREATE TABLE entry_thread_titles (\n    thread_root_uuid TEXT PRIMARY KEY,\n    title TEXT NOT NULL,\n    updated_at TEXT NOT NULL\n);\nCREATE TABLE entry_thread_summaries (\n    thread_root_uuid TEXT PRIMARY KEY,\n    summary TEXT NOT NULL,\n    updated_at TEXT NOT NULL\n);\nCREATE TABLE history (\n    id INTEGER PRIMARY KEY AUTOINCREMENT,\n    timestamp TEXT NOT NULL,\n    operation_type TEXT NOT NULL,\n    entry_id INTEGER NOT NULL,\n    old_data TEXT NOT NULL,\n    additional_data TEXT,\n    undone INTEGER DEFAULT 0,\n    redo_data TEXT\n);\nCREATE TABLE plugin_media_assets (\n    id INTEGER PRIMARY KEY AUTOINCREMENT,\n    hash TEXT NOT NULL UNIQUE,\n    mime_type TEXT NOT NULL,\n    bytes INTEGER NOT NULL,\n    width INTEGER NOT NULL,\n    height INTEGER NOT NULL,\n    storage_backend TEXT NOT NULL,\n    storage_key TEXT NOT NULL,\n    created_at TEXT NOT NULL,\n    deleted_at TEXT\n);\nCREATE TABLE plugin_entry_media (\n    id INTEGER PRIMARY KEY AUTOINCREMENT,\n    entry_uuid TEXT NOT NULL,\n    media_id INTEGER NOT NULL,\n    position INTEGER NOT NULL DEFAULT 0,\n    caption TEXT,\n    alt_text TEXT,\n    created_at TEXT NOT NULL,\n    FOREIGN KEY (entry_uuid) REFERENCES entries(uuid) ON DELETE CASCADE,\n    FOREIGN KEY (media_id) REFERENCES plugin_media_assets(id) ON DELETE CASCADE\n);\nCREATE TABLE plugin_entry_locations (\n    id INTEGER PRIMARY KEY AUTOINCREMENT,\n    entry_uuid TEXT NOT NULL UNIQUE,\n    latitude REAL NOT NULL,\n    longitude REAL NOT NULL,\n    place_name TEXT,\n    place_details TEXT,\n    source TEXT NOT NULL DEFAULT 'auto',\n    weather_condition TEXT,\n    weather_temp_c REAL,\n    weather_temp_f REAL,\n    weather_icon TEXT,\n    weather_humidity INTEGER,\n    weather_wind_kph REAL,\n    weather_fetched_at TEXT,\n    created_at TEXT NOT NULL,\n    FOREIGN KEY (entry_uuid) REFERENCES entries(uuid) ON DELETE CASCADE\n);\nCREATE TABLE plugin_location_cache (\n    id INTEGER PRIMARY KEY AUTOINCREMENT,\n    latitude REAL NOT NULL,\n    longitude REAL NOT NULL,\n    place_name TEXT NOT NULL,\n    place_details TEXT,\n    reverse_geocoded_at TEXT NOT NULL,\n    UNIQUE(latitude, longitude)\n);\nCREATE TABLE sync_location_tombstones (\n    entry_uuid TEXT NOT NULL PRIMARY KEY,\n    deleted_at TEXT NOT NULL\n);\nCREATE TABLE sync_entry_thread_title_tombstones (\n    thread_root_uuid TEXT PRIMARY KEY,\n    deleted_at TEXT NOT NULL\n);\nCREATE TABLE sync_entry_thread_summary_tombstones (\n    thread_root_uuid TEXT PRIMARY KEY,\n    deleted_at TEXT NOT NULL\n);\nCREATE TABLE sync_image_tombstones (\n    entry_uuid TEXT NOT NULL,\n    asset_hash TEXT NOT NULL,\n    position INTEGER NOT NULL DEFAULT 0,\n    caption TEXT,\n    alt_text TEXT,\n    deleted_at TEXT NOT NULL,\n    PRIMARY KEY (entry_uuid, asset_hash, position, caption, alt_text)\n);\n",
        );
    }
    match profile.fts {
        FixtureFts::Fts5 => sql.push_str("CREATE VIRTUAL TABLE entries_fts USING fts5(text);\n"),
        FixtureFts::Legacy => sql.push_str("CREATE TABLE entries_fts (text);\n"),
        FixtureFts::Absent => {}
    }
    sql
}

#[derive(Clone, Copy)]
struct SeedEntry {
    uuid: &'static str,
    created_at: &'static str,
    updated_at: &'static str,
    text: &'static str,
    text_plain: &'static str,
    content_format: &'static str,
    title: Option<&'static str>,
    summary: Option<&'static str>,
    mood: Option<&'static str>,
    starred: i64,
    pinned: i64,
    hidden: i64,
}

const SEED_ENTRIES: [SeedEntry; 5] = [
    SeedEntry {
        uuid: "entry_root",
        created_at: "2026-09-10 08:00",
        updated_at: "2026-09-10 08:00",
        text: "Root text for the synthetic garden walk.",
        text_plain: "Root text for the synthetic garden walk.",
        content_format: "markdown",
        title: Some("Root"),
        summary: Some("Thread root"),
        mood: Some("happy"),
        starred: 1,
        pinned: 0,
        hidden: 0,
    },
    SeedEntry {
        uuid: "entry_middle",
        created_at: "2026-09-11 08:00",
        updated_at: "2026-09-11 08:00",
        text: "Middle text continues the walk.",
        text_plain: "Middle text continues the walk.",
        content_format: "plain",
        title: None,
        summary: None,
        mood: Some("calm"),
        starred: 0,
        pinned: 0,
        hidden: 0,
    },
    SeedEntry {
        uuid: "entry_child",
        created_at: "2026-09-12 08:00",
        updated_at: "2026-09-12 08:00",
        text: "Child text records the weather stop.",
        text_plain: "Child text records the weather stop.",
        content_format: "plain",
        title: Some("Child"),
        summary: Some("Weather stop"),
        mood: Some("focused"),
        starred: 0,
        pinned: 1,
        hidden: 0,
    },
    SeedEntry {
        uuid: "entry_hidden",
        created_at: "2026-09-13 10:00",
        updated_at: "2026-09-13 10:00",
        text: "Hidden text is excluded from normal reads.",
        text_plain: "Hidden text is excluded from normal reads.",
        content_format: "plain",
        title: None,
        summary: None,
        mood: Some("quiet"),
        starred: 0,
        pinned: 0,
        hidden: 1,
    },
    SeedEntry {
        uuid: "entry_today",
        created_at: "2026-09-14 09:00",
        updated_at: "2026-09-14 09:00",
        text: "Today is a visible follow-up note.",
        text_plain: "Today is a visible follow-up note.",
        content_format: "markdown",
        title: Some("Today"),
        summary: Some("Latest note"),
        mood: None,
        starred: 0,
        pinned: 0,
        hidden: 0,
    },
];

fn seed_entries(
    connection: &Connection,
    profile: FixtureProfile,
    entry_count: usize,
) -> rusqlite::Result<()> {
    let columns = entry_insert_columns(profile);
    let mut sql = format!(
        "INSERT INTO entries ({}) VALUES ",
        columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let selected_entries = if entry_count == 2 {
        vec![SEED_ENTRIES[0], SEED_ENTRIES[3]]
    } else {
        SEED_ENTRIES[..entry_count].to_vec()
    };
    let values = selected_entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let id = match profile.ids {
                FixtureIds::Stable | FixtureIds::MissingColumn => None,
                FixtureIds::Nullable => Some("NULL".to_string()),
                FixtureIds::Duplicate => Some(
                    match index {
                        0 | 1 => 10,
                        2 => 20,
                        3 => 30,
                        _ => 40,
                    }
                    .to_string(),
                ),
            };
            columns
                .iter()
                .map(|column| match *column {
                    "id" => id.clone().expect("id value"),
                    "uuid" => sql_text(entry.uuid),
                    "created_at" => sql_text(entry.created_at),
                    "updated_at" => sql_text(entry.updated_at),
                    "text" => sql_text(entry.text),
                    "text_plain" => sql_text(entry.text_plain),
                    "content_format" => sql_text(entry.content_format),
                    "title" => sql_optional(entry.title),
                    "summary" => sql_optional(entry.summary),
                    "mood" => sql_optional(entry.mood),
                    "starred" => entry.starred.to_string(),
                    "pinned" => entry.pinned.to_string(),
                    "hidden" => entry.hidden.to_string(),
                    _ => unreachable!("known entry column"),
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect::<Vec<_>>()
        .join("),\n(");
    sql.push_str(&format!("({values});"));
    connection.execute_batch(&sql)
}

fn seed_relations(
    connection: &Connection,
    profile: FixtureProfile,
    entry_count: usize,
) -> rusqlite::Result<()> {
    connection
        .execute_batch("INSERT INTO tags (name) VALUES ('personal'), ('work'), ('outdoors');")?;
    let first_id = match profile.ids {
        FixtureIds::Duplicate => 10,
        _ => 1,
    };
    let middle_id = match profile.ids {
        FixtureIds::Duplicate => 10,
        _ => 2,
    };
    let child_id = match profile.ids {
        FixtureIds::Duplicate => 20,
        _ => 3,
    };
    let today_id = match profile.ids {
        FixtureIds::Duplicate => 40,
        _ => 5,
    };
    if entry_count == 2 {
        connection.execute_batch(&format!(
            "INSERT INTO entry_tags (entry_id, tag_id) VALUES ({first_id}, 1), ({middle_id}, 3);"
        ))?;
    } else {
        connection.execute_batch(&format!(
            "INSERT INTO entry_tags (entry_id, tag_id) VALUES ({first_id}, 1), ({middle_id}, 3), ({child_id}, 2), ({today_id}, 3);"
        ))?;
    }

    if !matches!(profile.schema, FixtureSchema::Full)
        || entry_count < SEED_ENTRIES.len()
        || matches!(
            profile.schema,
            FixtureSchema::Unsupported(UnsupportedRequiredColumn::Uuid)
        )
    {
        return Ok(());
    }
    let relation_sql = format!(
        r###"INSERT INTO entry_continuations (child_entry_uuid, parent_entry_uuid, updated_at)
         VALUES ('entry_middle', 'entry_root', '2026-09-11 08:00'),
                ('entry_child', 'entry_middle', '2026-09-12 08:00'),
                ('entry_today', 'entry_child', '2026-09-14 09:00');
         INSERT INTO entry_thread_titles (thread_root_uuid, title, updated_at)
         VALUES ('entry_root', 'Thread title', '2026-09-14 09:00');
         INSERT INTO entry_thread_summaries (thread_root_uuid, summary, updated_at)
         VALUES ('entry_root', 'Thread summary', '2026-09-14 09:00');
         INSERT INTO history (timestamp, operation_type, entry_id, old_data, additional_data, undone)
         VALUES ('2026-09-12 08:01', 'EDIT_TEXT', {child_id}, 'fixture', NULL, 0);
         INSERT INTO plugin_media_assets
             (hash, mime_type, bytes, width, height, storage_backend, storage_key, created_at)
         VALUES ('asset-one', 'image/jpeg', 100, 400, 300, 'local_fs', 'fixture/asset-one.jpg', '2026-09-12 08:00'),
                ('asset-two', 'image/jpeg', 120, 400, 300, 'local_fs', 'fixture/asset-two.jpg', '2026-09-12 08:00');
         INSERT INTO plugin_entry_media (entry_uuid, media_id, position, caption, alt_text, created_at)
         VALUES ('entry_child', 1, 0, 'A synthetic image', 'Synthetic image', '2026-09-12 08:00'),
                ('entry_today', 2, 0, NULL, 'Another synthetic image', '2026-09-14 09:00');
         INSERT INTO plugin_entry_locations
             (entry_uuid, latitude, longitude, place_name, place_details, source,
              weather_condition, weather_temp_c, weather_temp_f, weather_icon,
              weather_humidity, weather_wind_kph, weather_fetched_at, created_at)
         VALUES ('entry_child', 69.65, 18.96, 'Tromso', 'Synthetic harbor', 'manual',
                 'Overcast', 8.0, 46.4, 'cloudy', 82, 11.4, '2026-09-12 08:05', '2026-09-12 08:00'),
                ('entry_today', 60.39, 5.32, 'Bergen', 'Synthetic quay', 'default',
                 'Light rain', 12.0, 53.6, 'rain', 78, 8.2, '2026-09-14 09:05', '2026-09-14 09:00');
         INSERT INTO plugin_location_cache
             (latitude, longitude, place_name, place_details, reverse_geocoded_at)
         VALUES (60.39, 5.32, 'Bergen', 'Synthetic quay', '2026-09-14 09:04');
         INSERT INTO sync_location_tombstones (entry_uuid, deleted_at)
         VALUES ('entry_deleted_fixture', '2026-09-13 10:00');
         INSERT INTO sync_entry_thread_title_tombstones (thread_root_uuid, deleted_at)
         VALUES ('entry_old_thread', '2026-09-13 10:00');
         INSERT INTO sync_entry_thread_summary_tombstones (thread_root_uuid, deleted_at)
         VALUES ('entry_old_thread', '2026-09-13 10:00');
         INSERT INTO sync_image_tombstones
             (entry_uuid, asset_hash, position, caption, alt_text, deleted_at)
         VALUES ('entry_deleted_fixture', 'asset-old', 0, NULL, 'Old synthetic image', '2026-09-13 10:00');"###,
        child_id = child_id
    );
    connection.execute_batch(&relation_sql)?;
    Ok(())
}

fn seed_fts(connection: &Connection, profile: FixtureProfile) -> rusqlite::Result<()> {
    if matches!(profile.fts, FixtureFts::Absent) {
        return Ok(());
    }
    let text_expression = if matches!(
        profile.schema,
        FixtureSchema::Unsupported(UnsupportedRequiredColumn::Text)
    ) {
        "COALESCE(text_plain, '')"
    } else {
        "COALESCE(NULLIF(text_plain, ''), text, '')"
    };
    connection.execute(
        &format!(
            "INSERT INTO entries_fts(rowid, text)
             SELECT rowid, {text_expression} FROM entries"
        ),
        [],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_labels_are_stable_enough_for_evidence() {
        assert_eq!(
            FixtureProfile::full().label(),
            "schema=Full;fts=Fts5;ids=Stable"
        );
        assert_eq!(
            FixtureProfile::unsupported_text().label(),
            "schema=Unsupported(Text);fts=Fts5;ids=Stable"
        );
    }

    #[test]
    fn seeded_clock_date_is_safe_for_short_input() {
        assert_eq!(SeededClock::new("2026-02-29 23:59").date(), "2026-02-29");
        assert_eq!(SeededClock::new("unknown").date(), "unknown");
    }
}
