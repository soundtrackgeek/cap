# cap implementation plan

Plan version: 0.1.0 · 2026-09-14
Authority for intended behavior: [SPEC.md](SPEC.md).
Status: ready for a later implementation session; no agents, worktrees or feature
branches have been created by this planning revision.

## 1. Delivery model

The orchestrator owns architecture, interface contracts, task dispatch, review,
integration and release evidence. Feature tasks run as separate Codex tasks using
`gpt-5.6-luna` with `max` reasoning, each in its own Git worktree. Use at most three
active implementation tasks at once. The point of worktrees is isolated ownership
and reviewable commits, not three agents modifying the same integration checkout.

Two releases keep the basic promise usable early:

- **R1 / initial executable:** everyday capture with safe shared writes, location
  and weather, recovery, color-cli save magic, metadata/read commands, themes,
  machine output, diagnostics and installation. Complete all R1 gates before
  calling it usable with the real journal.
- **R2 / full planned experience:** writer, memory rediscovery, calendar, garden,
  stats/milestones, and automatic desktop refresh. Complete R2 before calling the
  entire specification finished.

This repository's `0.1.0` is a documentation revision. Do not assume it establishes
the executable's release number or Capsule's next version. The orchestrator assigns
SemVer versions using each repository's current history when integrating changes.

## 2. Repository and ownership boundaries

| Repository | Role | Planned changes |
| --- | --- | --- |
| `C:\_code\cap` | CLI application, effects and user-facing documentation | Cargo workspace, application modules, effects crate, fixtures, installation, SPEC/PLAN follow-through. |
| `C:\_code\capsule_tauri` | Authoritative shared journal behavior | Extract `crates/capsule-core`, keep desktop adapters, improve shared commit/context contracts, add narrowly scoped external-change refresh. |
| `C:\_code\deepseek_test\color-cli` | Source of rendering behavior | Read/reference only for the initial plan; no required upstream changes. Preserve revision and attribution in cap. |

Before implementation, inspect current branch/status, fetch without overwriting
local work, reread each repository's AGENTS.md, and record the exact starting SHAs.
The source snapshot in SPEC.md is evidence, not permission to reset a newer checkout.
Do not use a production DB, config, backup folder or media root as a test destination.

The orchestrator creates integration branches, proposed names
`codex/cap-integration` in cap and `codex/cap-core-integration` in Capsule. Each
worker gets an exact reviewed starting commit and one assigned feature branch.
Worktree names below are logical suffixes; record the actual paths returned by
Codex. Never guess a worktree path or use a queued client task ID as a ready task ID.

Root `Cargo.toml`, `Cargo.lock`, central command registration, shared DTOs,
README.md, CHANGELOG.md, version files and CI are shared files. The orchestrator
holds the editing token for them unless explicitly transferring it to one worker.
Workers submit required edits with their handoff; the orchestrator applies them in
a serialized integration window. AGENTS.md documentation/version/commit/push
requirements remain mandatory: shared ownership is coordination, not an exemption.
Do not mark a feature complete until its required documentation is committed and
pushed with the integrated work. If a worker's own commit requires those files,
transfer the token and rebase that worker before it edits them.

Every worker must be told: **You are not alone in the codebase. Do not revert other
people's edits. Work only in your assigned worktree/files and adapt to the reviewed
contracts. Ask the orchestrator before crossing an ownership boundary.**

## 3. Contracts to freeze before parallel work

WP00 creates compiling interfaces and synthetic examples for these boundaries.
The names below are the design contract; the orchestrator may refine Rust types
before dispatch, then records the final API in `docs/contracts.md`.

| Contract | Owner after foundation | Required contents / invariant |
| --- | --- | --- |
| `ResolvedCapsule` | WP01 then shared-core maintainer | Explicit DB/config/backup paths, canonical file identity, setting provenance, safe diagnostics. Avoid process-global env mutation in command execution/tests. |
| `CapabilityReport` | WP01 | Supported required columns, optional relation/FTS/context tables, write support and reasons; read-only construction. |
| `CaptureRequest` | WP02 | Normalized text/format, metadata, request time, reserved UUID, capture ID, frozen DB binding. Renderer-independent; no terminal escapes added. |
| `CommitReceipt` | WP02 | Confirmed UUID, capture ID, known display number, saved time, backup audit, committed state; available immediately after commit. |
| `ContextPolicy` / `ContextResult` | WP03 | Settings snapshot, network/cache policy, overall deadline; separate location/weather outcomes, persisted values, timestamps and warnings. |
| `CaptureOutcome` | WP02 / orchestrator | `not_committed`, `committed`, or `unknown`; structured errors cannot turn a known commit into an ordinary retryable failure. |
| `JournalReader` | WP06 | Bounded queries and stable IDs; no repair, schema changes, backup creation or hidden writes. |
| `EffectFrame` / `TerminalCapabilities` | WP04 | Pure frame calculation, display-cell geometry, seeded/time-injected rendering, color/motion capabilities. |
| `ReceiptModel` | WP05 | Only actual save/context/query facts; no DB access from renderer. Supports fictional demo models marked as such. |
| `OutputEnvelope<T>` | Orchestrator | Versioned SPEC envelope, stable codes and exit mapping; one stdout object in JSON mode. |

Keep workflow sequencing in `src/app.rs`: input → resolve/preflight → pending
record → shared commit → immediate receipt → context → final output. Effects and
optional stats cannot write entries. Provider calls cannot run inside the entry
transaction. JSON mode buffers the final object; the early saved line is for
human output only.

A contract change after dispatch needs a short message specifying the problem,
proposed signature, consumer impact and test migration. The orchestrator accepts
or rejects it and messages every affected worker before it lands.

## 4. Work packages

### WP00 — Foundation and compatibility fixtures

Owner: orchestrator. Branch suffix: `foundation`. Depends on: none.

Owned files: cap root manifests/lockfile, `src/main.rs`, `src/app.rs`,
`src/contracts.rs`, `src/output.rs`, `docs/contracts.md`,
`tests/fixtures/`, `docs/evidence/baseline.md`.

Tasks:

1. Confirm `cap` does not collide with an installed command/alias; preserve the
   user's chosen command name and surface collisions during installation.
2. Scaffold a native Rust workspace with `clap`, Serde, shared-core integration
   slots and `crates/cap-effects`. Pin versions compatible with Capsule's existing
   rusqlite/native SQLite linkage; avoid two incompatible `links=sqlite3` crates.
3. Freeze command grammar, envelope, error/cancellation states and interfaces above.
4. Build synthetic Capsule fixtures from existing backend test schemas: normal,
   missing optional tables, bad/legacy IDs, hidden entries, FTS present/absent,
   continuation links, location/weather, older required-column variants and
   intentionally unsupported schemas. No exported personal entries.
5. Add an isolated test harness that redirects every DB/config/backup/cache/draft
   path and replaces network calls. Establish a clean-checkout build check.

Acceptance: scaffold builds; contract fixtures parse; a failing isolation guard
prevents tests using a non-temporary destination. Record baseline source SHAs and
current test commands. Do not add mock “Saved” behavior wired to normal capture.

### WP01 — Extract a headless Capsule core and preserve desktop behavior

Luna task: `Capsule core extraction`.
Branch suffix: `core`. Repository: Capsule. Depends on: WP00 contracts.

Owned files: new `crates/capsule-core/`, initial moves/re-exports for
`src-tauri/src/{db,backup,entries,location,models}.rs`, narrowly required Tauri
adapter/import changes. Manifest/version files require the shared-file token.

Tasks:

1. Map the transitive dependencies before moving modules. Extract the small
   headless boundary; move OS shell-opening operations back to desktop adapters.
2. Preserve tests and public desktop behavior; provide explicit-path APIs for
   capture/query/context instead of test-only path functions or env changes.
3. Add diagnostic path resolution with provenance, strict explicit override
   handling, allowlisted settings, file identity and supported-schema inspection.
4. Split pure read queries from the repair-capable desktop convenience wrappers.
   Preserve existing repair behavior where the desktop expects it.
5. Provide shared request/receipt seams that WP02/WP03 can implement without
   depending on Tauri. Move the existing save implementation; do not rewrite SQL
   during a supposedly mechanical extraction.

Acceptance:

- Core builds/tests without Tauri, WebView, tray or shell lifecycle dependencies.
- Existing Capsule create/edit/search/backup/location tests pass through adapters;
  model serialization used by the frontend remains compatible.
- Resolver fixtures prove every precedence branch, missing explicit paths,
  malformed settings and independent `--db` binding.
- Reopening cap reader fixtures read-only cannot invoke ID repair or create files.
- Dependency graph and reviewed upstream SHA are recorded for cap integration.

Handoff includes moved-file map and any behavior changes. A straight source
extraction is reviewed separately from later concurrency/context improvements.

### WP02 — Transactional capture, backups and recoverable identity

Luna task: `Capsule durable capture core`.
Branch suffix: `capture-core`. Repository: Capsule. Depends on: WP01 accepted.

Owned files: shared-core `capture.rs`, `identity.rs`, create/repair transaction
sections of `entries.rs`, backup reservation/retention sections of `backup.rs`,
related fixture tests. No provider/renderer changes.

Tasks:

1. Implement externally reserved Capsule-compatible UUID support and capture
   request validation. Check collisions under the write transaction; do not
   regenerate identity behind a pending recovery record.
2. Use bounded immediate transactions around identity/numeric-ID allocation,
   entry/tag/continuation/FTS writes and resequencing. Retain all related references.
3. Return a committed receipt without relying on a post-commit detail query or
   successful weather capture. Provide explicit uncertain-outcome errors.
4. Make backup naming/reservation and pruning safe across two clients. Resolve
   competing backup/restore/file-replacement behavior with bounded coordination
   and identity revalidation; keep SQLite backup verification mandatory.
5. Support lookup/reconciliation of a reserved UUID against normalized request
   fields, including a missing/different DB identity.

Acceptance:

- Faults before backup, during backup, before insert, during FTS/resequence, at
  commit, and after commit produce the correct saved/not-saved/unknown outcome.
- Two processes create entries concurrently with no duplicate identities, lost
  tags, corrupt search results or broken continuation references.
- A held write lock exits within the overall budget; no multiplied retry delay.
- Matching retries resolve once; conflicting same-ID requests fail; independent
  identical-text entries both survive.
- Verified backups are distinct and usable; concurrent retention cannot remove an
  active backup. Tests cover resequencing and stale file handles after replacement.

The orchestrator reviews this package before any feature writes to a real journal.

### WP03 — Capsule location/weather as a bounded shared service

Luna task: `Capsule context capture`.
Branch suffix: `context`. Repository: Capsule. Depends on: WP01 accepted.
May run with WP02: ownership is limited to context files, with frozen DTOs.

Owned files: shared-core `location.rs`, `context.rs`, `providers/`, context cache
interfaces and provider fixtures. Any shared connection helper change is requested
from WP02/orchestrator, not edited concurrently.

Tasks:

1. Extract settings/provider interpretation from side-effectful attachment code.
   Return structured location and weather outcomes instead of silent `false`/`None`
   or raw `eprintln!` as the only explanation.
2. Add injected HTTP, clock and cache. Enforce one 8-second deadline across all
   lookups/fallbacks; remove duplicate weather attempts. Honor cancel/offline/skip.
3. Preserve fixed-place precedence, IP behavior, provider choices, units,
   persisted timestamps, geocoding cache and location source values.
4. Accept optional cache storage supplied by cap; expose observation age and
   provider/place keys. Desktop defaults must remain compatible.
5. Add explicit missing-context enrichment: fresh backup, only missing fields,
   deleted-entry detection, and entry-time weather rules. Never fetch current
   weather for an old entry then label it historical.

Acceptance: fixtures cover fixed place, disabled capture, unsupported method,
IP/fixed geocode failure, both weather providers, timeout, malformed response,
zero network offline mode, valid/expired/wrong-place cache, partial metadata and
old-entry retry. Source/provider fields read back through Capsule's normal detail
model match. No journal content appears in HTTP requests. Entry writes remain
successful when context is unavailable.

### WP04 — Real color-cli port and terminal foundation

Luna task: `cap color-cli engine`.
Branch suffix: `effects`. Repository: cap. Depends on: WP00.
Can run while WP01 is in progress; use only synthetic render models.

Owned files: `crates/cap-effects/src/`, its tests, `tests/fixtures/color-cli/`,
`docs/provenance/color-cli.md`. Root manifest changes through the orchestrator.

Tasks:

1. Read the pinned `colorcli.py`; port its eight actual palettes, five gradient
   modes, smoothstep reveal and shimmer formula. Capture golden fixtures from the
   original pure functions without changing the upstream project.
2. Build pure frame generation with injected time/seed and separate terminal I/O.
3. Implement grapheme-aware cell layout, truecolor/256/16/plain capabilities,
   control-sequence sanitization, 40-column layout and resize fallback.
4. Implement a terminal guard restoring cursor, color and raw/alternate modes
   after exit, exception, Ctrl+C and render errors.
5. Implement explicit mode precedence and bounded frame timing from SPEC.

Acceptance: reference colors/formulas match within documented tolerance; tests
cover emoji, combining marks, CJK, long lines, literal ESC/OSC/BEL content, redirected
stdout, NO_COLOR, TERM=dumb, static reduced motion and resizing. Provide real
Windows Terminal evidence of fade/shimmer and cursor restoration. cap must run
without Python or the color-cli checkout after installation.

### WP05 — Quick capture command, receipts and local recovery

Luna task: `cap capture experience`.
Branch suffix: `capture-cli`. Repository: cap.
Depends on: WP02 + WP03 integrated into a pinned core revision; WP04 accepted.

Owned files: `src/commands/add.rs`, `src/input.rs`, `src/recovery/`,
`src/context_cache.rs`, `src/ui/receipt.rs`, `src/ui/weather.rs`, capture process tests.
Orchestrator wires command registration/output using the frozen contracts.

Tasks:

1. Implement positional/default capture, explicit add, file/stdin and metadata.
   Validate shell-facing ambiguity cases; preserve content under SPEC normalization.
2. Implement the pending-record/receipt state machine with atomic files and
   per-record locking, database binding, capture IDs and retention policy.
3. Implement status/recover/enrich commands and explicit safe replay rules.
4. Connect shared commit/context calls to the immediate saved line and final
   receipt. Enrichment cancellation cannot retroactively fail an entry save.
5. Build the capsule seal and weather stamp from WP04 primitives. Keep body text
   static and constrain all default decoration to 650 ms.
6. Implement create dry-run, JSON, quiet, plain and broken-pipe behavior.

Acceptance: launch the actual executable in process tests; prove DB rows, search,
metadata and receipts agree. Kill at checkpoints around pending-file write,
commit/receipt/context completion and retry through recovery without duplicate
entries. Cover simultaneous recovery, storage-full/permission errors, redirected
input/output, long content, reserved command names, literal `--`, invalid UTF-8,
empty content and size limits. A failed effect/provider never causes a second save.

### WP06 — Useful reads and clear diagnostics

Luna task: `cap journal reads`.
Branch suffix: `reads`. Repository: cap. Depends on: WP01 accepted.
Core query additions use a separate Capsule worktree owned by this same task if
needed; it may not alter core mutation/context files.

Owned files: `src/commands/{show,today,recent,search,tags,moods,context,doctor}.rs`,
`src/query.rs`, read/query integration tests; shared-core `read.rs` and read-only
search entry points under explicit cross-repository ownership.

Tasks:

1. Provide bounded list/search/discovery and exact UUID/number lookup with stable
   output models. Reuse Capsule syntax and FTS fallback semantics.
2. Implement doctor and context reports showing path source, schema capabilities,
   backup configuration, disabled/missing context settings and terminal support.
   Do not dump raw config, tokens, entry text or precise coordinates by default.
3. Guarantee the CLI's queries never invoke legacy automatic ID repairs. Unsupported
   ID/column layouts produce an actionable capability warning/error.
4. Build static readable list/detail layouts on the effects abstraction. Search
   returns a useful empty result with exit 0.

Acceptance: fixture DB logical snapshots are unchanged after every read, including
legacy-ID paths. Open tests with SQLite query-only connections and assert no backup
or cache files are created. Pagination, same-number/UUID resolution, hidden-entry
protection, search escaping and fallback results agree with Capsule definitions.

### WP07 — Themes, playground, local configuration and completions

Luna task: `cap personality controls`.
Branch suffix: `personality`. Repository: cap. Depends on: WP04 accepted.

Owned files: `src/commands/{theme,fx,config,completions}.rs`, `src/preferences.rs`,
`src/ui/themes.rs`, `assets/themes/`, mode/config/completion tests.

Tasks:

1. Ship aurora, neon, c64, amber and paper presets and palette previews. Effects
   demos are marked synthetic and require no DB/config/network access.
2. Implement cap-local settings only: theme, color, motion, icon mode, preview
   visibility, writer display/target override and editor executable/argument array.
   Reject unrelated Capsule configuration keys.
3. Apply CLI/env/config/capability precedence consistently across commands. Quiet
   and plain do not merely call color-cli's ANSI-emitting `print_plain` equivalent.
4. Emit PowerShell completion scripts without modifying the user's profile;
   support metadata completion only when explicitly enabled and read-only.

Acceptance: previews show real theme differences; all output-mode combinations
work without ANSI leakage. Theme changes survive restart and do not change Capsule
settings. Completion output parses in PowerShell and quoted entry text still works.

### WP08 — Draft-backed interactive writer

Luna task: `cap terminal writer`.
Branch suffix: `writer`. Repository: cap. Depends on: R1 capture/recovery accepted.

Owned files: `src/commands/write.rs`, `src/writer/`, writer input tests and manual
terminal scenarios. Reuse WP05 recovery services rather than a second draft format.

Tasks: implement no-argument TTY routing, multiline editing/paste, save/exit,
500-ms draft persistence, resume/discard, metadata footer, ambient rail and word
target/Gauntlet behavior. Add safe editor process launch using executable/argument
arrays, unique temporary files and failed-editor recovery. JSON/non-TTY invocations
must not enter an interactive session; return a clear input error with the envelope.

Acceptance: real keyboard/paste tests in Windows Terminal cover Enter, Ctrl+S,
Ctrl+C, Unicode, terminal resize, an editor path with spaces, failed/blank editor
output, a crash/relaunch and changed active DB. Unsaved content is recovered and
a saved draft cannot publish twice. Writer preserves the user's terminal state.

### WP09 — Time machine, calendar, garden and true milestones

Luna task: `cap memory experiences`.
Branch suffix: `memories`. Repository: cap. Depends on: WP06 + WP07 accepted.

Owned files: `src/commands/{recall,on_this_day,calendar,stats,garden}.rs`,
`src/insights.rs`, `src/ui/{unseal,calendar,garden}.rs`, synthetic time/stat fixtures.
Core stats exposure is a separately owned Capsule worktree for this task:
`crates/capsule-core/src/{stats,mood_sentiment}.rs` and corresponding desktop
re-exports. Do not edit WP08 writer or WP10 refresh files.

Tasks:

1. Reuse shared calendar/stat definitions; document local-date, visibility and
   current streak semantics. A streak remains current if the latest writing day
   is today or yesterday. Do not silently diverge from Capsule; reconcile any
   observed difference as a reviewed shared behavior change.
2. Implement recall/non-repeat state and on-this-day with honest empty results.
3. Implement month heatmap, count/word stats and the specified garden thresholds.
   Desktop-written entries contribute exactly as CLI-written entries do.
4. Add deterministic daily/weekly milestone crossing detection and once-only
   glints with cap-local atomic state. Feed events into receipts through contracts,
   with a strict query budget; stats failure cannot fail a save.

Acceptance: seeded fixtures cover midnight, leap day, DST dates, no entries,
hidden entries, imported older entries, multiple entries per day and repeated
recall with one/many candidates. Verify metric parity against Capsule on the same
fixture. No gamification XP, badge, or quest rows are changed. Provide narrow and
wide terminal evidence; no unbounded full-journal animation.

### WP10 — Capsule notices external changes without losing UI state

Luna task: `Capsule external journal refresh`.
Branch suffix: `external-refresh`. Repository: Capsule.
Depends on: R1 interoperability baseline and accepted shared core.

Owned files: a focused `src-tauri/src/external_changes.rs` service,
`src/lib/externalChanges.ts`, related hook/tests, scoped `src/App.tsx`,
`src/backend.ts`, models/command wiring changes under shared-file coordination.

Tasks: detect changes through a persistent read-only connection and same-connection
`data_version` comparisons, plus path/file replacement handling. Recheck on focus;
while relevant views are visible, poll at a modest 2-second interval and debounce
bursts for 250 ms. Refresh affected entry/search/dashboard/calendar models while
preserving selected UUID, filtering, ordering, scroll position and all draft state.
Do not start cap or require a cap daemon. Do not perform file mtime-only detection
that misses WAL writes.

Acceptance: run the desktop against a disposable DB, scroll/select/filter, create
an external CLI entry with weather, then verify refresh and preserved state. Repeat
with an unsaved composer, active search, tray/focus return, rapid external writes
and switched DB path. Record actual installed/built desktop version. Browser mock
tests alone do not prove native detection.

### WP11 — Windows installation and packaging

Luna task: `cap Windows delivery`.
Branch suffix: `delivery`. Repository: cap. Depends on: R1 integration candidate.

Owned files: `scripts/install.ps1`, `scripts/uninstall.ps1`, `scripts/smoke.ps1`,
release packaging and installation docs; workflow/manifests via shared-file token.

Tasks: build a release `cap.exe` and checksum manifest; install to a per-user bin
directory such as `%LOCALAPPDATA%\Programs\cap\bin`; add only that directory to
user PATH, preserving existing entries. Detect existing commands, running binaries,
and prior installs. Support update/uninstall without removing journal or recovery
data. Shell profile/completion activation is opt-in. Lock the reviewed shared-core
Git revision and remove temporary local dependency patches from release inputs.

Acceptance: fresh clean checkout builds with no sibling repositories/Python;
installed `cap --help` and `cap --json doctor` run from an unrelated directory and
a fresh PowerShell session. Synthetic add/read works using explicit fixture paths.
Verify paths containing spaces, update, uninstall and PATH preservation. Archive
includes required notices/provenance; do not invent upstream licensing.

### WP12 — Integration verification and release review

Owner: orchestrator, with bounded Luna verification tasks only for independent
test matrices. Branch suffix: `verification`. Depends on: applicable release WPs.

Owned files: `tests/integration/`, fault/concurrency harnesses,
`docs/evidence/R1.md`, `docs/evidence/R2.md`, final README/CHANGELOG/version updates,
CI, release notes and final dependency pin.

Tasks: run the gates below, inspect full diffs, replay primary scenarios, review
SQLite semantics and output state machines, and compare promised visual behavior
with actual terminal evidence. Return specific failures to their original owners.
Do not accept a worker summary as proof of integration or physical/native testing.

## 5. Parallel waves and integration order

| Wave | Active work | Gate before moving on |
| --- | --- | --- |
| 0 | Orchestrator WP00 | Compiling contracts, isolated fixtures, recorded source baseline. |
| 1 | WP01 core + WP04 effects | Headless extraction/desktop regression passed; real rendering primitives accepted. |
| 2 | WP02 durable core + WP03 context + WP07 personality | Compatible commit/context boundary; safe transactions; terminal modes accepted. |
| 3 | WP05 capture experience + WP06 reads | Integrated binary saves, enriches, recovers and queries fixtures correctly. |
| 4 | WP11 delivery + orchestrator WP12 R1 | All R1 gates; installed CLI and Capsule interoperability demonstrated. |
| 5 | WP08 writer + WP09 memories + WP10 desktop refresh | Each richer feature independently accepted; shared-file changes serialized. |
| 6 | Orchestrator WP12 R2 + packaging refresh | Full SPEC acceptance; no unverified features described as shipped. |

WP06 can begin earlier when a slot is free and its read-only API is frozen. Do
not parallelize WP01 extraction with edits to the old/new core files. Do not let
WP05 depend on an unreviewed local patch that never becomes a pinned upstream SHA.
If a wave blocks on contracts, reduce concurrency rather than asking Luna to guess.

Cross-repository changes are integrated upstream first: review Capsule core,
commit/push the approved branch, then update cap's pinned revision. The desktop
consumer and its tests must use the same reviewed shared logic. Release gating
must explicitly distinguish testing the old installed Capsule binary from testing
the newly refactored desktop build.

## 6. Orchestrator task lifecycle

1. List saved projects and select the exact repository; inspect `isGitRepository`.
   Use Codex task creation with worktree environment, model `gpt-5.6-luna`, thinking
   `max`, an explicit title, and the assigned reviewed branch/starting state.
   Create actual user-visible tasks only when the implementation run is requested.
2. Record package → task ID → host → worktree path → branch → base SHA → owned
   files in `docs/implementation-status.md`. A queued worktree's client ID is
   tracked separately until a real task ID is available.
3. Send the task brief below, including acceptance criteria and the exact contract
   revision. State other agents' ownership and read-only source boundaries.
4. Use bounded multi-task waits and their cursors to receive completion/attention
   snapshots. Avoid repeated full-history reads. Inspect output only where needed.
5. On completion, inspect the worktree diff and exact commit SHA, execute the
   relevant acceptance tests, and review error paths and user-facing behavior.
6. If rejected, send a concrete reproducible failure to the same task; give its
   owner another turn. Do not silently patch over it while the owner is working.
7. Accept only after the checks pass. Serialize shared-file docs/version changes,
   integrate by reviewed commits, resolve conflicts deliberately, and rerun tests
   affected by the integration. Push without force; preserve unrelated changes.
8. Record final evidence and close/mark completed workers. For collaboration
   subagents, call `interrupt_agent` after a final/idle/errored completion so they
   are not left marked working. Separate Codex tasks use their task lifecycle;
   do not pass their IDs to collaboration tools.

Feature branches can be pushed for review under the repository's workflow. Workers
must not merge to the default branch, force-push, tag a release or clean another
agent's directory. Keep builds in per-worktree target directories. Capsule's
AGENTS.md currently requires `cargo clean` from `src-tauri` before handoff even for
docs sessions, aligned app versions for releases, commit/push and matching release
tags for an actual app release. Recheck that file; clean only the owned build
directory and report failures. Do not create a Capsule installer release tag just
to publish a core dependency or a docs change.

## 7. Ready-to-use worker brief

```text
Implement WPxx from PLAN.md in your assigned worktree.

Repository: <exact path/project>
Branch and base commit: <assigned branch>, <reviewed SHA>
Contract revision: <SHA of docs/contracts.md and shared core>
Owned files: <explicit paths from the work package>
Dependencies already accepted: <IDs and commit SHAs>

You are not alone in the codebase. Do not revert other people's edits. Do not
edit files outside your ownership without coordinating with the orchestrator.
Use the frozen interfaces and adapt to reviewed changes from other workers.

Read SPEC.md, your WP section and the repository's AGENTS.md. Implement the
behavior and acceptance cases exactly. Use synthetic/disposable data and injected
network/clock dependencies. Do not write to the real Capsule journal, change its
settings, launch sync, publish a release, or make unrelated cleanup changes.

Request the shared-file editing token for required README, CHANGELOG, manifest,
lockfile or version changes. Complete the repo workflow through coordinated
integration; do not omit it. Commit and push only the assigned feature branch.

Return: exact commit SHA, changed files, behavior implemented, commands run and
results, evidence artifacts, deliberate limitations, and any remaining contract
questions. State clearly which native/UI/network scenarios were not tested.
```

Example review message:

```text
WP05 needs one correction before acceptance. With a fixture DB and a forced
weather timeout after COMMIT, the process exits 1. The row is present, so a caller
could retry and duplicate it. SPEC requires exit 0 and weather=unavailable after
a confirmed save. Add a process-level regression for this checkpoint, correct
the outcome mapping, and return the new commit plus test output. Preserve WP07's
output-mode changes; do not rewrite the shared parser.
```

Example accepted handoff:

```text
Accepted WP04 at <SHA>. Verified palette reference fixtures, 40-column Unicode
layout, no ANSI under --json/--plain, and cursor restoration in Windows Terminal.
Your effect API is now frozen at <contract SHA>. Stop editing; the orchestrator
will integrate this commit and assign any follow-up explicitly.
```

## 8. Verification gates and feature traceability

| Gate | Required evidence | Packages / release |
| --- | --- | --- |
| G01 Path/config authority | Resolver matrix; explicit missing path never creates/falls through; malformed overrides reported; zero secret leakage. | WP01, WP06; R1 |
| G02 Entry parity | CLI row read through Capsule list/detail/search: normalized text, UUID, time, tags, mood, title/summary, flags, FTS and continuation. | WP02, WP05, WP06; R1 |
| G03 Context parity | Fixed/IP/disabled, both providers, cache/timeout/offline and entry-time retry fixtures; persisted fields read through Capsule. | WP03, WP05; R1 |
| G04 Shared DB safety | Independent-process simultaneous writes; lock timeout; backup collision/prune; resequence/reference integrity; DB replacement/restore boundary. | WP02; R1 |
| G05 Recovery truth | Fault injection/kill before and after commit; unknown state reconciliation; no duplicate on replay; deliberate duplicate text remains possible. | WP02, WP05; R1 |
| G06 Parsing/output | Shell examples, quotes/reserved names/--, piped/file content, invalid input, machine envelopes/exit codes, NO_COLOR/plain/reduced/quiet. | WP00, WP05–WP07; R1 |
| G07 Visual quality | Actual Windows Terminal recording/screenshots: seal, fade/shimmer, weather stamp, five themes, narrow layout, resize and Ctrl+C restoration. | WP04, WP05, WP07; R1 |
| G08 Independent install | Clean checkout build with pinned core, no sibling/Python dependency; installed CLI from other folder; update/uninstall and PATH preservation. | WP11; R1 |
| G09 Desktop coexistence | Desktop closed, open + manual refresh, restart; fixture entry/details/search/weather verified on recorded desktop versions. | WP05, WP06, WP12; R1 |
| G10 Responsiveness | p50/p95 phase timings on 1k/10k/100k synthetic DBs; max decoration/deadlines; warm/cold and lock-contention results. | WP02–WP05, WP12; R1 and R2 |
| G11 Writer correctness | Real paste/keyboard/editor, target/Gauntlet, crash/resume and DB-switch scenarios with exact saved content. | WP08; R2 |
| G12 Memory accuracy | Fixture comparisons for recall/on-this-day/calendar/stats/garden/milestones including dates/visibility and no gamification mutations. | WP09; R2 |
| G13 Automatic refresh | Native external writes detected without losing scroll, selection, filters or unsaved draft; DB switch/replacement and tray/focus. | WP10; R2 |

Automated baseline after a relevant code change:

```powershell
# cap workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --locked
```

For Capsule changes, use its actual current scripts: frontend tests/build and
lint, Rust formatting, Clippy and Rust tests; run environment-sensitive Rust
fixtures serially when they mutate process globals. After extraction, include the
new core crate explicitly if it is not covered by the desktop manifest's tests.
Tests sharing global env must not run concurrently; prefer explicit config input.
Follow each repo's current AGENTS.md cleanup/release workflow after verification.

Use SQLite integrity/foreign-key checks after mutation/fault suites and compare
logical rows/references. Mere file existence, a successful SQL INSERT, or a CLI
exit code is insufficient proof that Capsule will display/search the entry.

Only an optional explicitly authorized live smoke test may write to the real
journal. All release gates must be runnable with synthetic fixtures or an
appropriately reviewed disposable copy. Do not silently change Capsule's active
production path for testing; launch the test app with isolated overrides.

## 9. Review checklist for the orchestrator

- Does `cap ordinary words` work without Capsule running and without a prompt?
- Is every “saved” claim tied to a confirmed commit and a stable UUID?
- Can network/effects failure, a broken pipe or a killed process cause duplicate
  insertion, lost draft text or a misleading exit code?
- Are Capsule's source/location/units/time/backup/FTS/reference semantics retained?
- Are reads truly read-only, including legacy schema paths?
- Is this a port of color-cli's actual visual behavior with source provenance?
- Is the ordinary receipt short, attractive and immediate, including on narrow
  terminals? Do all motion/color overrides really work?
- Do the optional experiences show real shared journal activity, without claiming
  unsupported XP awards or native testing that was never performed?
- Can a fresh checkout build and the installed binary run without development
  sibling paths, Python, a Tauri window or a server?
- Are README examples, SPEC status, CHANGELOG/version, tests and pushed commits
  consistent with what is actually implemented?

## 10. Completion and remaining decisions

The source-backed architectural direction is decided: native Rust, shared Capsule
core, Rust color-cli port, Windows-first delivery, bounded default celebration.
Agents do not need to reopen those choices. Foundation work decides exact crate
versions, final Rust DTO shapes, a supported schema capability matrix and the
coordinated backup/restore strategy. Packaging resolves attribution/license facts
and checks the actual `cap` command environment. All decisions are recorded with
evidence, not guessed from the planning snapshot.

At each release, update `docs/implementation-status.md` with accepted package IDs,
task/worktree/branch mapping, commits, gates, known limitations and next work. If a
gate is blocked, retain it as outstanding with the concrete reason; do not mark the
package complete because its happy path or unit tests passed.

The final user handoff includes installation/usage, a short real terminal demo,
the relevant release commit, Capsule core/app compatibility, verification summary
and untested platform/provider cases. Preserve accepted worktrees until integration
and push are verified, then clean only their verified paths. Do not leave agents
running after their work is complete.
