# cap

The development build pins the reviewed shared capture and context core and
includes the quick-capture/recovery vertical slice. Capture writes are guarded
by a verified backup, frozen database identity and cap-local recovery records.

Run `cargo run --locked --example core_process_probe` for the isolated shared-core
crash/concurrency check. It creates and removes its own temporary synthetic
journals; it accepts no existing journal path. `-- --bench` measures the core
on synthetic 1k/10k/100k-entry journals (three process samples per size).

A command-line companion for [Capsule](https://github.com/soundtrackgeek/capsule_tauri), currently in development.
The Windows delivery scripts build a locked native release archive and install
it per-user without requiring administrator rights. Capture uses Capsule's
active database, location settings and weather, with a little color-cli
ceremony when it saves.

Implementation is in progress on `codex/cap-integration`. The executable capture
path is covered by disposable synthetic-journal process tests; live-journal and
native Windows terminal acceptance remain release work.

```powershell
cap Had a lovely walk by the water
cap add --mood content --tag life -- 'A quiet evening outside.'
Get-Content -Raw .\today.md | cap
cap add --json --capture-id walk-2026-09-14 -- 'A caller-retryable note'
cap status --capture-id walk-2026-09-14
cap recover list
cap recover retry walk-2026-09-14
cap enrich <entry-uuid>
cap --db .\capsule.db recent
cap --db .\capsule.db search 'tag:work after:2026-01-01'
```

- [SPEC.md](SPEC.md): behavior, command contract, shared data architecture,
  recovery, color-cli reuse, visual effects, accessibility and release criteria.
- [PLAN.md](PLAN.md): feature ownership, Luna task/worktree waves, review loops,
  integration gates and evidence required before release.
- [docs/windows-install.md](docs/windows-install.md): Windows packaging,
  installation, update/uninstall, PATH safety and isolated smoke verification.
- [CHANGELOG.md](CHANGELOG.md): repository history.

Implementation uses a native Rust `cap.exe`, a shared headless
Capsule core, and a Rust port of the palettes/fade/shimmer from
[color-cli](https://github.com/soundtrackgeek/color-cli).

Development: `cargo test --workspace` and `cargo build`. Work package progress is
tracked in [docs/implementation-status.md](docs/implementation-status.md).
Rust 1.95.0 is pinned in rust-toolchain.toml; rustup installs it when needed.
The development build supports `--help`, `--version`, JSON help/usage errors,
cap-local personality controls from WP07, quick capture from positional words,
files or stdin, durable `status`/`recover` operations, explicit context
enrichment, and bounded read handlers for `show`, `today`, `recent`, `search`,
`tags`, `moods`, `context`, and `doctor`.
The read handlers bind an explicit Capsule database path, exclude hidden entries
by default, preserve structured-search fallback diagnostics, and never repair
legacy IDs or create backups/cache files. Capture uses one immediate saved UUID,
optional post-commit context, a 15-minute weather cache and a 30-day receipt
retention window; `--dry-run` performs no backup, mutation, context or cache
work, and `--no-context` skips provider/cache work entirely.
Tests use synthetic temporary databases; the live journal is not a test fixture.
The WP09 memory handlers add read-only `recall`, `on-this-day`, `calendar`,
`stats`, and `garden` projections. They use shared Capsule metrics, local
calendar dates, visible entries by default, and a cap-local state file for
non-repeating recall and once-only daily/weekly glints. Garden tiers are
0 bare, 1–49 seed, 50–199 sprout, 200–499 leaf, and 500+ bloom words; the
daily and weekly glint thresholds are 50 and 500 words. These commands never
write journal, XP, badge, or quest rows. All five commands are registered;
capture-receipt glints and final motion polish are being integrated.
Human memory views also expose pure time-injected frames and guarded TTY
streaming: the unseal cue is capped at 700ms, the seven-day garden grows in at
400ms, and the calendar uses a static grid capped at 300ms. JSON, quiet, plain,
redirected, reduced-motion, and narrow (40-column) output stay deterministic.
The shared Capsule core is fetched from a reviewed Git revision recorded in
Cargo.toml/Cargo.lock; a local Capsule or Python checkout is not needed to build.
Preference updates use a flushed temporary file and same-directory replacement;
failed replacement keeps the previous settings. Windows concurrency evidence is
recorded in [the state-file checks](docs/evidence/windows-state.md).
The bounded Capsule compatibility matrix can be run with
`cargo test --test fixture_matrix --locked`; its schema/row snapshot and
isolation evidence is recorded in [docs/evidence/fixtures.md](docs/evidence/fixtures.md).

The effects crate can be previewed without Capsule or Python:

```powershell
cargo run -p cap-effects --example color_cli_demo
```

It uses owned Unicode-safe layouts, source-backed palettes/gradients, bounded
fade/shimmer frames, capability-aware ANSI/plain output, and terminal-state
restoration. The example text is synthetic and is never saved.

Native verification labs can be generated with `cargo run --example fixture_lab
--locked -- 5`. These are new synthetic databases in the OS temp directory; see
[native lab guidance](docs/evidence/native-lab.md) for the isolated launch contract.

Build development measurement tools with
`cargo build --release --examples --locked`, then run
`target\release\examples\benchmark_capture.exe target\release\cap.exe
target\release\examples\fixture_lab.exe`. It creates fresh synthetic journals,
checks each saved row, and reports first-output and total p50/p95 latency as JSON.
It does not accept an existing journal path or claim to measure a cold disk cache.

For the core alone, `cargo run --release --locked --example core_process_probe
-- --bench` measures 20 fresh processes per synthetic journal size, with separate
checkpoint timings. See [measured latency](docs/evidence/latency.md) for the
results and measurement limits.

Personality controls are local to cap and never write Capsule's configuration:

```powershell
cap theme list
cap theme preview neon
cap theme set aurora
cap config set editor.executable 'C:\Program Files\Editor\editor.exe'
cap config set editor.args '["--wait", "{file}"]'
cap fx all
cap completions powershell | Set-Content .\cap-completions.ps1
```

`cap fx all` leaves all eight color-cli palettes visible after its short reveal.
Use `cap fx vertical` (or `text`, `line`, `diagonal`, `rainbow`) to compare
the source gradient placements on a multiline sample.

`cap fx` uses synthetic text and does not open a journal, network provider, or
preferences file. Completion output is a script only; review and opt in to it
explicitly. Metadata suggestions require `CAP_COMPLETIONS_METADATA=1` and use
the same bounded read-only `tags`/`moods` services as the journal commands.

To verify console cancellation without journal access, build
`cargo build --example fx_interrupt_probe --locked` and run
`target\debug\examples\fx_interrupt_probe.exe target\debug\cap.exe` in a console.
The probe interrupts only its synthetic child process and checks exit code 130.

To make a local Windows archive from a reviewed checkout, run
`.\scripts\package.ps1 -OutputDirectory .\dist -ExpectedCoreRevision <sha>`.
The packager requires a clean checkout by default (use `-AllowDirty` only for a
clearly local development archive). The archive contains `cap.exe`, SHA-256 manifests, notices, provenance and
per-user `install.ps1`/`uninstall.ps1` scripts. Installation defaults to
`%LOCALAPPDATA%\Programs\cap\bin`; only that directory is added to user PATH,
and no shell profile is changed unless `-ActivateCompletions` is explicitly
requested. The delivery scripts install and update only `cap.exe` and their
receipt/metadata; they never install or update the Capsule desktop app or its
journal, recovery, settings, media, sync, or backup data. See
[docs/windows-install.md](docs/windows-install.md) for update, uninstall and
temporary-root smoke commands.
