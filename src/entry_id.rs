//! Capsule entry IDs: `entry_` followed by eight lowercase base-36 digits.

use std::{
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{anyhow, Result};
use capsule_core::db;
use chrono::Utc;

static LAST_SEED: AtomicU64 = AtomicU64::new(0);

pub(crate) fn new_entry_uuid(database_path: &Path) -> Result<String> {
    let now = Utc::now()
        .timestamp_nanos_opt()
        .unwrap_or_else(|| Utc::now().timestamp_micros() * 1_000) as u64;
    // Keep requests made within one clock tick distinct, including writer drafts.
    let previous = LAST_SEED
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
            Some(now.max(last.saturating_add(1)))
        })
        .expect("seed update always succeeds");
    let seed = now.max(previous.saturating_add(1));
    let connection = db::open_read_only_connection(database_path)?;
    select_available(seed, |candidate| {
        Ok(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE uuid = ?1)",
            [candidate],
            |row| row.get(0),
        )?)
    })
}

fn select_available(seed: u64, mut exists: impl FnMut(&str) -> Result<bool>) -> Result<String> {
    // Match Capsule's base36_8 encoding and bounded collision retry.
    for offset in 0..10_000_u64 {
        let candidate = encode(seed.wrapping_add(offset));
        if !exists(&candidate)? {
            return Ok(candidate);
        }
    }
    Err(anyhow!("Unable to generate a unique entry UUID."))
}

fn encode(mut value: u64) -> String {
    const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut buffer = *b"entry_00000000";
    for index in (6..14).rev() {
        buffer[index] = ALPHABET[(value % 36) as usize];
        value /= 36;
    }
    String::from_utf8(buffer.to_vec()).expect("base-36 alphabet is ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_capsule_encoding_including_padding_and_wraparound() {
        assert_eq!(encode(0), "entry_00000000");
        assert_eq!(encode(35), "entry_0000000z");
        assert_eq!(encode(36), "entry_00000010");
        assert_eq!(encode(36_u64.pow(8) - 1), "entry_zzzzzzzz");
        assert_eq!(encode(36_u64.pow(8)), "entry_00000000");
    }

    #[test]
    fn skips_existing_ids_and_fails_when_candidates_are_exhausted() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE entries(uuid TEXT UNIQUE);
             INSERT INTO entries VALUES ('entry_0000000z'), ('entry_00000010');",
        )
        .unwrap();
        let uuid = select_available(35, |candidate| {
            Ok(db.query_row(
                "SELECT EXISTS(SELECT 1 FROM entries WHERE uuid=?1)",
                [candidate],
                |row| row.get(0),
            )?)
        })
        .unwrap();
        assert_eq!(uuid, "entry_00000011");
        assert!(select_available(0, |_| Ok(true)).is_err());
        assert!(select_available(0, |_| Err(anyhow!("database unavailable"))).is_err());
    }
}
