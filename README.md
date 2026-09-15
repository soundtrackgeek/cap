# cap

The development build includes quick capture, a draft-backed writer, recovery,
location/weather, memory views and color-cli effects. Capture writes are guarded
by a verified backup, frozen database identity and cap-local recovery records.
Installation adds only the separate `cap.exe`; your existing Capsule app stays
unchanged. Capsule can be closed while you use cap.

New entries use Capsule's normal UUID format: `entry_` followed by eight
lowercase letters/digits (for example, `entry_d262eioo`). Quick capture and the
writer share the same generator. Existing pending captures retain their reserved
UUID when retried so recovery cannot create a duplicate entry.

Run `cargo run --locked --example core_process_probe` for the isolated shared-core
crash/concurrency check. It creates and removes its own temporary synthetic
journals; it accepts no existing journal path. `-- --bench` measures the core
on synthetic 1k/10k/100k-entry journals (20 process samples per size).

A command-line companion for [Capsule](https://github.com/soundtrackgeek/capsule_tauri), currently in development.
The Windows delivery scripts build a locked native release archive and install
it per-user without requiring administrator rights. Capture uses Capsule's
active database, location settings and weather, with a little color-cli
ceremony when it saves.

To install on Windows, download the ZIP from the
[releases page](https://github.com/soundtrackgeek/cap/releases), choose **Extract
All**, and double-click **install.bat** in the extracted folder. It installs
cap for your user account and adds it to PATH without administrator rights.
Open a new terminal and run `cap --help`. When cap is already installed, the
double-click installer shows its path and asks whether to replace it. Type `y`
to update, or press Enter to cancel without changing the existing installation.

The CLI features are integrated on `codex/cap-integration`. Executable capture,
recovery, writer input and delivery are checked with disposable synthetic journals.
Actual Windows Terminal visual review and native desktop coexistence remain
unverified; this is a development build, not a published Capsule desktop release.

```powershell
cap add Had a lovely walk by the water
cap write
cap garden
cap recall
cap theme set c64
cap update                 # install a newer version from the Git repository
cap update --check         # check without installing
cap add --mood content --tag life -- 'A quiet evening outside.'
cap add --mood good --tags life,outdoors,gratitude,exercise "Had a lovely walk"
Get-Content -Raw .\today.md | cap add --stdin
cap add --json --capture-id walk-2026-09-14 -- 'A caller-retryable note'
cap status --capture-id walk-2026-09-14
cap recover list
cap recover retry walk-2026-09-14
cap write                 # interactive draft-backed writer (TTY)
cap write --editor        # configured executable/argument-array editor
cap enrich <entry-uuid>
cap --db .\capsule.db recent
cap --db .\capsule.db search 'tag:work after:2026-01-01'
```

Quick capture requires `cap add`. Unknown commands such as `cap test` report
`Command not recognized`, display the same help as `cap --help`, and exit 2
without saving an entry. Bare `cap` shows help, even with piped input; it does
not start a draft or capture stdin. Use `cap write` explicitly for the editor.

`cap today`, `cap recent`, and `cap search` show complete entry text, including
paragraph breaks. Read output uses the available terminal width (with a one-cell
margin) and wraps longer lines to fit. Use `--limit` and `--offset` to page through
entries, or `cap show <id>` for an entry's full metadata and authored text.

`cap add --tags` accepts comma-separated tags. You can repeat or mix `--tags`
and `--tag` (for example, `--tags life,outdoors --tag gratitude`). Both spellings
split on commas; quote lists containing spaces, such as `--tags "life, fresh air"`.
Saving trims whitespace, ignores empty tags, and merges duplicates without regard
to case. Put metadata options before the entry text.

`cap update` checks the version on the repository's `master` branch and updates
the executable you ran. On Windows x64 it downloads the version's GitHub release
ZIP, verifies its SHA-256 and executable provenance, and installs it in place.
If that version has no published release (or no package for your platform), it
builds the exact source commit with Cargo; that fallback needs Rust, Git, and
native build tools on PATH. It never downgrades an equal or newer installed version.

After successful interactive journal commands such as `cap add`, a friendly
magenta notice offers `cap update` when a newer version is known. Checks run in
the background at most once a day; the first check's result appears on a later
command. Network failures stay quiet and retry after an hour. `--offline`,
`--quiet`, `--json`, dry runs, CI, and redirected output skip automatic checks
and notices. `--plain`, `--color never`, and `NO_COLOR` keep the notice uncolored.
The cache is `update-check.json` in cap's configuration directory. An explicit
`cap update --check` always checks online; `--offline update` reports an error.

Backup creation preserves existing orphaned metadata in older Capsule journals.
It verifies SQLite integrity and compares foreign-key diagnostics with the same
source snapshot before saving. It does not delete or repair the orphaned rows.
If a backup fails, no entry is inserted; the error includes the cause and a
`cap recover retry <capture-id>` command to save the retained text and metadata
after the cause is resolved. `cap recover list` shows pending captures.

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
Before publishing, run the actual-archive updater check in
[Windows delivery](docs/windows-install.md) as well as the workspace tests.
Rust 1.95.0 is pinned in rust-toolchain.toml; rustup installs it when needed.
The development build supports `--help`, `--version`, JSON help/usage errors,
cap-local personality controls from WP07, explicit `cap add` capture from words,
files or stdin, durable `status`/`recover` operations, explicit context
enrichment, and bounded read handlers for `show`, `today`, `recent`, `search`,
`tags`, `moods`, `context`, and `doctor`.
The read handlers bind an explicit Capsule database path, exclude hidden entries
by default, preserve structured-search fallback diagnostics, and never repair
legacy IDs or create backups/cache files. Capture uses one immediate saved UUID,
optional post-commit context, a 15-minute weather cache and a 30-day receipt
retention window; `--dry-run` performs no backup, mutation, context or cache
work, and `--no-context` skips provider/cache work entirely.
Location/weather GET requests retry transient connection failures, timeouts,
and HTTP 429/502/503/504 once within the same eight-second context budget.
Retries are spaced at least one second apart and honor `Retry-After`. A provider
outage can still leave context unavailable; the entry remains saved. The warning
includes the network cause and the exact `cap enrich <entry-uuid>` command to
retry missing metadata on that entry without saving its text again.
`cap write` is TTY-only (machine and redirected invocations fail clearly), keeps
multiline Unicode edits in cap-local recovery records after 500 ms idle and on
Ctrl+C, and asks explicitly whether to resume or discard a draft on the next
launch. Ctrl+S clears the editable marker before handing the frozen request to
the shared capture path; a saved draft cannot be published twice. `--editor`
passes the configured executable and argument array directly, expands `{file}`
as one argument, and accepts only successful, nonempty UTF-8 output. Draft
status shows the frozen Capsule database destination and word target. Native
Windows Terminal interaction remains a required manual release gate; unit tests
cover the deterministic buffer/state/editor contracts only.
Tests use synthetic temporary databases; the live journal is not a test fixture.
The WP09 memory handlers add read-only `recall`, `on-this-day`, `calendar`,
`stats`, and `garden` projections. They use shared Capsule metrics, local
calendar dates, visible entries by default, and a cap-local state file for
non-repeating recall and once-only daily/weekly glints. Garden tiers are
0 bare, 1–49 seed, 50–199 sprout, 200–499 leaf, and 500+ bloom words; the
daily and weekly glint thresholds are 50 and 500 words. These commands never
write journal, XP, badge, or quest rows. All five commands are registered.
Capture receipts show a once-only glint when that entry crosses a real daily
or weekly threshold. An unavailable optional statistic never changes save success.
Human memory views also expose pure time-injected frames and guarded TTY
streaming: the unseal cue is capped at 700ms, the seven-day garden grows in at
400ms, and the calendar uses a static grid capped at 300ms. JSON, quiet, plain,
redirected, reduced-motion, and narrow (40-column) output stay deterministic.
The live receipt closes two capsule halves and adds a 250ms weather accent only
when a condition was captured or cached, within the same 650ms save ceremony.
Recall opens the shell before its date and static body; garden draws seven small
plants above its word-count legend. Memory views use available width and preserve
scrollback; a scene taller than the terminal finishes without repeated redraws.
Set `preview_visibility never` to hide save previews, or `icon_mode ascii` for
ASCII decorations. Cached weather keeps its original observation timestamp.
The writer scrolls long lines to keep the caret visible, respects terminal cell
width for Unicode, and animates only its side rail while idle. Ctrl+S saves;
Ctrl+C keeps the draft and exits with code 130; Escape keeps it and exits normally.
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
`cargo run --example lab_command -- <generated-lab.json> <cap.exe> <arguments>`
keeps real console input/output while clearing the child environment and checking
console-mode restoration. It accepts only generated labs under the OS temp path.

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
