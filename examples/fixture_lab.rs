//! Create a persistent synthetic lab for native/installation verification.
//! This development example only creates a NEW directory below the OS temp dir.
//! It never accepts a database path or reads the user's Capsule configuration.

use rusqlite::{params, Connection};
use serde_json::json;
use std::{
    error::Error,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() -> Result<(), Box<dyn Error>> {
    let count = std::env::args()
        .nth(1)
        .map(|value| value.parse::<u32>())
        .transpose()?
        .unwrap_or(5);
    if !(5..=100_000).contains(&count) {
        return Err("Fixture entry count must be between 5 and 100000".into());
    }
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!("cap lab {}-{nonce}", std::process::id()));
    fs::create_dir(&root)?;
    for directory in [
        "backups",
        "state",
        "media",
        "sync",
        "appdata",
        "localappdata",
        "profile",
        "temp",
    ] {
        fs::create_dir(root.join(directory))?;
    }
    let db = root.join("capsule.db");
    let mut connection = Connection::open(&db)?;
    connection.execute_batch(include_str!("../tests/fixtures/capsule.sql"))?;
    let transaction = connection.transaction()?;
    for index in 6..=count {
        let uuid = format!("entry_lab_{index:06}");
        let text =
            format!("Synthetic lab memory {index}. A quiet walk by the harbor before dinner.");
        transaction.execute(
            "INSERT INTO entries (uuid, created_at, updated_at, text, text_plain, content_format, hidden)
             VALUES (?1, '2026-09-14 12:00:00', '2026-09-14 12:00:00', ?2, ?2, 'markdown', 0)",
            params![uuid, text],
        )?;
        transaction.execute(
            "INSERT INTO entries_fts(rowid,text) VALUES (?1,?2)",
            params![transaction.last_insert_rowid(), text],
        )?;
    }
    transaction.commit()?;
    connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_checkpoint(TRUNCATE);")?;
    drop(connection);
    let config = root.join("config.json");
    fs::write(
        &config,
        serde_json::to_vec_pretty(&json!({
            "location.auto_capture":false,
            "location.use_default_location":true,
            "location.default_location_name":"Synthetic Harbor",
            "location.weather_provider":"open_meteo"
        }))?,
    )?;
    let settings = root.join("path_settings.json");
    fs::write(
        &settings,
        serde_json::to_vec_pretty(&json!({
            "databasePath":db, "backupDirectory":root.join("backups"),
            "imageMediaRoot":root.join("media"), "syncPath":root.join("sync"),
            "autoSyncEnabled":false, "minimizeToTrayOnClose":false
        }))?,
    )?;
    let lab = json!({
        "synthetic":true, "root":root, "entryCount":count,
        "environment": {
            "CAPSULE_DB_PATH":db, "CAPSULE_CONFIG_PATH":config,
            "CAPSULE_PATH_SETTINGS_PATH":settings, "CAPSULE_BACKUP_DIR":root.join("backups"),
            "CAPSULE_IMAGES_MEDIA_ROOT":root.join("media"), "CAPSULE_SYNC_PATH":root.join("sync"),
            "CAPSULE_HOME":root, "CAP_CONFIG_HOME":root.join("state"),
            "APPDATA":root.join("appdata"), "LOCALAPPDATA":root.join("localappdata"),
            "USERPROFILE":root.join("profile"), "HOME":root.join("profile"),
            "TEMP":root.join("temp"), "TMP":root.join("temp")
        }
    });
    fs::write(root.join("lab.json"), serde_json::to_vec_pretty(&lab)?)?;
    println!("{}", serde_json::to_string(&lab)?);
    Ok(())
}
