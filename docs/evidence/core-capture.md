# Shared capture process evidence

2026-09-14, Windows, Rust 1.95.0, synthetic journals only.
Reviewed core: `8881d301cf15f0bb175f69acd0f0be8c6ba5fba8`.

Run `cargo run --locked --example core_process_probe` from the cap repository.
The coordinator creates its own temporary labs, validates child paths and markers,
clears child environments, and kills/reaps its own held-lock/checkpoint processes.
No existing database path is accepted. The checked-in fixture contains invented
entries; it is not a personal journal export.

Passed cases:

- Eight independent writers: distinct UUIDs, entry count, tags, continuation
  references, FTS contents, integrity and verified backup retention agree.
- Kill after backup, before commit and after commit: read-only reconciliation
  followed by repeated replay produces exactly one saved entry per request.
- Same-path replacement before capture and after preflight: refusal without
  mutation; the latter also proves no backup/retention happens first.
- Backup destination failure: no insert.
- Combined sidecar and SQLite contention: the actual capture call returned typed
  `database_busy` after **15,001 ms**; complete child invocation **15,022 ms**.
  No row was inserted, and retry succeeded after killing the lock holder.

The test allows 500 ms of scheduling overhead around the 15-second operation
deadline and separately checks a 16.5-second end-to-end bound. It records both
timings so process startup cannot conceal a slow capture call.

Review uncovered two earlier failures. Candidate `8ae93e4` stacked waits and took
about 19.5 seconds. Candidate `4272d53` removed stacked budgets but its capture
call still took 16,164 ms under Windows contention. SQLite's default timeout counts
requested sleeps; the final correction uses scoped handlers driven by monotonic
time. See [SQLite's timeout contract](https://www.sqlite.org/c3ref/busy_timeout.html).

The combined core passes 98 unit tests and strict Clippy, including context
replacement-before-backup, abandoned provider payloads, actual 500-ms residual
context contention and successful retry when a competing writer releases early.
These tests establish process-crash behavior, not sudden power-loss immunity.
The existing SQLite WAL `synchronous=NORMAL` policy remains unchanged.

The probe's optional `--bench` uses three independent debug process captures at
1k, 10k and 100k synthetic entries. Filesystem cache is uncontrolled. Those figures
are preliminary shared-core measurements; the installed CLI benchmark and native
Capsule/Windows Terminal gates are separate acceptance work.
