# Fixture matrix evidence — 2026-09-14

This evidence records the bounded synthetic Capsule fixtures used by WP00 and
the read-only foundations for WP12. The fixture harness never opens, copies,
or exports a production journal. Every database, configuration file, cache,
draft directory, media root, sync path and subprocess temporary directory is
created below a unique `tempfile::TempDir`.

## Profiles

`tests/support/mod.rs` exposes a composable `FixtureProfile` with independent
schema, FTS and ID dimensions:

| Dimension | Variants | Coverage |
| --- | --- | --- |
| Schema | `Full`, `OptionalTablesAbsent`, `Unsupported(...)` | Required entry projection; tags; continuation/thread metadata; history; media; location/weather/cache and sync tombstones; missing optional relations; deliberate missing required `created_at`, `text` or `uuid`. |
| FTS | `Fts5`, `Legacy`, `Absent` | SQLite FTS5 virtual table, the legacy plain `entries_fts` table used by Capsule's older fixtures, and no FTS table. |
| IDs | `Stable`, `MissingColumn`, `Nullable`, `Duplicate` | AUTOINCREMENT IDs, no `id` column, NULL IDs, and repeated positive IDs. Numeric references remain visible so repair/read behavior can be tested without silently repairing the fixture. |

The normal profile contains five synthetic entries: four visible and one
hidden. Three continuation links form a root → middle → child → today chain;
thread title/summary, tags, history, two image attachments, two persisted
location/weather observations, a geocoding cache row, and sync tombstones are
present. `Fixture::new()` retains the two-row foundation compatibility shape
used by `tests/foundation.rs`; matrix tests use `Fixture::from_profile` for the
full five-row profile.

## Isolation and snapshots

`Fixture::assert_owned` canonicalizes existing path components and resolves the
nearest existing parent for paths that do not exist yet. This rejects parent
traversal and symlink escapes while accepting a new child below the fixture
root. `Fixture::command` clears the inherited environment and sets only the
Capsule-supported path overrides (`CAPSULE_DB_PATH`,
`CAPSULE_CONFIG_PATH`, `CAPSULE_PATH_SETTINGS_PATH`, `CAPSULE_BACKUP_DIR`,
`CAPSULE_IMAGES_MEDIA_ROOT`, `CAPSULE_SYNC_PATH`, `CAPSULE_HOME`) plus the
cap-local state root and safe runtime loader variables. Gist credentials,
provider API keys and proxy variables are removed.

`Fixture::snapshot` captures `sqlite_master` schema objects and sorted logical
rows (including SQLite FTS shadow tables and NULL/integer/real/text/blob
values). Callers can compare a before/after snapshot with
`assert_snapshot_unchanged` to prove that a read path did not repair IDs,
create a table, refresh FTS or otherwise write.

`Fixture::hold_write_lock` holds a disposable `BEGIN IMMEDIATE` transaction.
The matrix test uses a second connection with a short timeout to verify a busy
lock and then releases the guard before a write is attempted. It does not fake
capture faults or assert behavior that the current CLI has not implemented.

## Verification

The bounded matrix currently has thirteen tests (including helper self-checks):

```powershell
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
```

On this worktree, the focused matrix and existing foundation tests passed. The
CLI remains a bootstrap binary, so capture/recovery fault injection, provider
network behavior, independent-process writes, desktop interoperability and
native Windows Terminal evidence remain intentionally outstanding for WP02,
WP03, WP05, WP06, WP10 and WP12 integration review.
