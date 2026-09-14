//! Persistent cap-local weather cache.
//!
//! Capsule's shared context service owns provider interpretation and enforces
//! the fifteen-minute freshness window.  This adapter only supplies a durable
//! cache implementation.  Every document is bound to the exact database
//! identity captured for the invocation, so a copied/replaced journal can
//! never borrow another journal's weather observation.

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use capsule_core::{
    context::{ContextCache, WeatherCacheKey},
    contracts::WeatherObservation,
    db::FileIdentity,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const CACHE_VERSION: u32 = 1;
const MAX_AGE_SECONDS: i64 = 15 * 60;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheDocument {
    version: u32,
    database_path: String,
    database_identity: FileIdentity,
    observations: BTreeMap<String, WeatherObservation>,
}

/// A path-bound implementation of the shared `ContextCache` trait.
#[derive(Debug, Clone)]
pub struct PersistentContextCache {
    path: PathBuf,
    database_path: String,
    database_identity: FileIdentity,
}

impl PersistentContextCache {
    pub fn at_path(
        path: impl Into<PathBuf>,
        database_path: &Path,
        database_identity: FileIdentity,
    ) -> Self {
        Self {
            path: path.into(),
            database_path: database_path
                .to_string_lossy()
                .replace('/', "\\")
                .to_ascii_lowercase(),
            database_identity,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn key_for(key: &WeatherCacheKey) -> String {
        // Preserve the exact IEEE-754 coordinate bits. Rounding to a fixed
        // decimal width could make nearby places share a weather observation.
        format!(
            "{:016x}:{:016x}:{}:{}",
            key.latitude.to_bits(),
            key.longitude.to_bits(),
            key.provider.len(),
            key.provider
        )
    }

    fn load(&self) -> Option<CacheDocument> {
        let bytes = fs::read(&self.path).ok()?;
        let document = serde_json::from_slice::<CacheDocument>(&bytes).ok()?;
        if document.version != CACHE_VERSION
            || document.database_path != self.database_path
            || !document
                .database_identity
                .same_file(&self.database_identity)
        {
            return None;
        }
        Some(document)
    }

    fn put_observation(
        &self,
        key: &WeatherCacheKey,
        observation: &WeatherObservation,
    ) -> io::Result<()> {
        let mut document = self.load().unwrap_or_else(|| CacheDocument {
            version: CACHE_VERSION,
            database_path: self.database_path.clone(),
            database_identity: self.database_identity.clone(),
            observations: BTreeMap::new(),
        });
        // The shared service treats a missing observation timestamp as
        // unusable. Avoid persisting one that could not be safely aged.
        if observation.fetched_at.is_none() {
            return Ok(());
        }
        let now = Utc::now();
        document.observations.retain(|_, value| {
            value.fetched_at.is_some_and(|fetched| {
                (0..=MAX_AGE_SECONDS).contains(&now.signed_duration_since(fetched).num_seconds())
            })
        });
        document
            .observations
            .insert(Self::key_for(key), observation.clone());
        let bytes = serde_json::to_vec_pretty(&document)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        atomic_write(&self.path, &bytes)
    }
}

impl ContextCache for PersistentContextCache {
    fn get(&self, key: &WeatherCacheKey, now: DateTime<Utc>) -> Option<WeatherObservation> {
        let observation = self.load()?.observations.get(&Self::key_for(key))?.clone();
        let fetched_at = observation.fetched_at?;
        let age = now.signed_duration_since(fetched_at).num_seconds();
        (0..=MAX_AGE_SECONDS).contains(&age).then_some(observation)
    }

    fn put(&self, key: &WeatherCacheKey, observation: &WeatherObservation) {
        let _ = self.put_observation(key, observation);
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "cache path has no parent"))?;
    fs::create_dir_all(parent)?;
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("weather.json");
    let temporary = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), counter));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
        let wide = |value: &Path| {
            value
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        };
        let source = wide(temporary);
        let target = wide(destination);
        #[allow(non_snake_case)]
        unsafe extern "system" {
            fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
        }
        for attempt in 0..200 {
            let moved = unsafe {
                MoveFileExW(
                    source.as_ptr(),
                    target.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            };
            if moved != 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            if !matches!(error.raw_os_error(), Some(5 | 32 | 33)) || attempt == 199 {
                return Err(error);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        unreachable!("bounded cache replacement loop always returns")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn identity(directory: &Path) -> (PathBuf, FileIdentity) {
        let db = directory.join("capsule.db");
        fs::write(&db, b"db").unwrap();
        let identity = FileIdentity::for_path(&db);
        (db, identity)
    }

    #[test]
    fn cache_round_trip_is_bound_to_database_and_fifteen_minutes() {
        let directory = tempfile::tempdir().unwrap();
        let (db, identity) = identity(directory.path());
        let cache = PersistentContextCache::at_path(
            directory.path().join("weather.json"),
            &db,
            identity.clone(),
        );
        let key = WeatherCacheKey::new(60.0, 5.0, "open_meteo");
        let fetched = Utc.with_ymd_and_hms(2026, 9, 14, 12, 0, 0).unwrap();
        let observation = WeatherObservation {
            provider: Some("open_meteo".to_string()),
            condition: Some("Clear".to_string()),
            icon: Some("clear".to_string()),
            temp_c: Some(12.0),
            temp_f: Some(53.6),
            humidity: None,
            wind_kph: None,
            fetched_at: Some(fetched),
        };
        cache.put(&key, &observation);
        assert!(cache
            .get(&key, fetched + chrono::Duration::minutes(15))
            .is_some());
        assert!(cache
            .get(
                &key,
                fetched + chrono::Duration::minutes(15) + chrono::Duration::seconds(1)
            )
            .is_none());
        let other = PersistentContextCache::at_path(
            directory.path().join("weather.json"),
            &db,
            FileIdentity {
                stable_id: Some("different".to_string()),
                ..identity
            },
        );
        assert!(other.get(&key, fetched).is_none());
    }

    #[test]
    fn sub_decimal_coordinate_changes_do_not_share_a_cache_key() {
        let first = WeatherCacheKey::new(60.000000001, 5.0, "open_meteo");
        let second = WeatherCacheKey::new(60.000000002, 5.0, "open_meteo");
        assert_ne!(
            PersistentContextCache::key_for(&first),
            PersistentContextCache::key_for(&second)
        );
    }

    #[test]
    fn malformed_or_future_version_documents_are_cache_misses() {
        let directory = tempfile::tempdir().unwrap();
        let (db, identity) = identity(directory.path());
        let path = directory.path().join("weather.json");
        let cache = PersistentContextCache::at_path(&path, &db, identity);
        let key = WeatherCacheKey::new(60.0, 5.0, "open_meteo");
        fs::write(&path, b"not-json").unwrap();
        assert!(cache.get(&key, Utc::now()).is_none());
        fs::write(
            &path,
            br#"{"version":99,"databasePath":"x","databaseIdentity":{},"observations":{}}"#,
        )
        .unwrap();
        assert!(cache.get(&key, Utc::now()).is_none());
    }

    #[test]
    fn put_prunes_expired_observations_and_ignores_write_failures() {
        let directory = tempfile::tempdir().unwrap();
        let (db, identity) = identity(directory.path());
        let path = directory.path().join("weather.json");
        let cache = PersistentContextCache::at_path(&path, &db, identity.clone());
        let old_key = WeatherCacheKey::new(60.0, 5.0, "open_meteo");
        let new_key = WeatherCacheKey::new(61.0, 6.0, "open_meteo");
        let old = WeatherObservation {
            provider: Some("open_meteo".to_string()),
            condition: Some("Rain".to_string()),
            icon: None,
            temp_c: Some(4.0),
            temp_f: None,
            humidity: None,
            wind_kph: None,
            fetched_at: Some(Utc::now() - chrono::Duration::hours(1)),
        };
        let fresh = WeatherObservation {
            fetched_at: Some(Utc::now()),
            ..old.clone()
        };
        cache.put(&old_key, &old);
        cache.put(&new_key, &fresh);
        let document = cache.load().unwrap();
        assert!(!document
            .observations
            .contains_key(&PersistentContextCache::key_for(&old_key)));
        assert!(document
            .observations
            .contains_key(&PersistentContextCache::key_for(&new_key)));

        let blocked_parent = directory.path().join("blocked");
        fs::write(&blocked_parent, b"file").unwrap();
        let failing =
            PersistentContextCache::at_path(blocked_parent.join("weather.json"), &db, identity);
        failing.put(&new_key, &fresh);
        assert!(failing.get(&new_key, Utc::now()).is_none());
    }
}
