# Development latency measurements

Measured on 2026-09-14 on Windows, AMD Ryzen 5 7600X (six cores), C: NTFS,
32 GiB RAM. Other Luna tasks/builds were active; filesystem cache, antivirus and
scheduler load were not controlled. Every database was newly generated synthetic
data. Each sample used a new process. These are development measurements, not
claims about the user's real journal or a controlled cold disk cache.

## Shared core

Release probe at core `8881d301cf15f0bb175f69acd0f0be8c6ba5fba8`, 20 samples
per size. Full raw samples: [core-latency.json](core-latency.json).

| Initial entries | Initial DB bytes | Capture p50 / p95 | Backup + coordination p50 | Repair + reconciliation p50 | Resequence p50 |
| --- | --- | --- | --- | --- | --- |
| 1,000 | 569,344 | 89 / 112 ms | 56 ms | 16 ms | 1 ms |
| 10,000 | 4,190,208 | 295 / 409 ms | 231 ms | 22 ms | 17 ms |
| 100,000 | 41,230,336 | 2,448 / 4,969 ms | 2,049 ms | 114 ms | 122 ms |

Phase names identify actual injected checkpoints: backup includes coordination
and verification; finalization includes COMMIT and remaining backup/receipt guard
work. They are not isolated CPU timers. Per-phase medians do not sum to the total
median. The probe verifies final row counts, FTS, references, SQLite integrity and
all retained backups. Retention is five snapshots.

Verified backup dominates this run, especially at 100k entries. It must remain
enabled. A long save needs visible working feedback before its confirmed Saved
line. This core-only probe does not include CLI pending/receipt files, provider
context, milestone queries or terminal effects. The CLI measurements below
include the first two local-state operations.

## Complete CLI capture

Normal release `cap 0.2.0-dev.26`, source
`f7ba64d019434ee8a9f99093ead3ef86b9dbcf7a`, shared core
`4888a2a33ab65a1833cdf5e6cbdad38baa3d4030`. Executable SHA-256:
`67c0da878637baa0bfaf1c9a5ec2ff6b0304876178a574c559865f7793bcbeef`.
Full raw samples and per-command warnings: [cli-latency.json](cli-latency.json).

Each sample launches a new process with `--plain --no-context add`, isolated
environment and newly generated synthetic journal. Pending drafts, verified
backups, commit receipts and optional milestone checks are included. Provider
requests and terminal effects are excluded. Exact final counts and each unique
saved body are verified; no user journal is used.

| Initial entries | Initial DB bytes | Saved line p50 / p95 | Process exit p50 / p95 |
| --- | --- | --- | --- |
| 1,000 | 548,864 | 116 / 162 ms | 135 / 200 ms |
| 10,000 | 3,997,696 | 392 / 452 ms | 444 / 506 ms |
| 100,000 | 39,366,656 | 2,281 / 2,998 ms | 2,391 / 3,110 ms |

There are 20 samples per size. Saved-line timing measures transport arrival,
not the precise SQLite commit instant. All saves succeed. The smaller fixtures
produce no warnings. All 100k samples report an optional milestone query timeout
(`interrupted` or `metric query exceeded its deadline`): almost all generated
entries fall on the same date, so the weekly query reaches its 100ms deadline.
The capture stays saved; the optional glint is omitted. Neither the lock nor
query timeout was lengthened to make this measurement appear faster or cleaner.
This is a reproducible development baseline, not a controlled cold-cache claim.

## Help startup

Release `cap 0.2.0-dev.21`, integration `409d3fe`: 20 PowerShell/.NET
Process.Start invocations of `cap --help` from the OS temp directory, redirected
stdout read to completion, successful exit/content checked. Median 6.912 ms,
p95 7.601 ms, range 5.822–178.181 ms. The first sample was 178.181 ms and includes
first-use launcher/runtime overhead. This does not establish a sub-100ms
controlled cold-start guarantee. No database was opened.

Repeatable invocation for the core benchmark:

```powershell
cargo run --release --locked --example core_process_probe -- --bench
```

Use the actual CLI measurement tool documented in README for final end-to-end
results, and report context/effect timing separately.
