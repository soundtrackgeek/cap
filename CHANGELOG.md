# Changelog

## 0.2.0-dev.33 - 2026-09-15

### Fixed

- Unknown commands such as `cap test` now report `Command not recognized`, show
  the same help as `cap --help`, and exit 2 without creating an entry or recovery
  state. Quick capture requires `cap add`.
- Bare `cap` now shows help instead of opening the writer or capturing piped
  input. Use `cap add --stdin` for pipes and `cap write` for the editor.

## 0.2.0-dev.32 - 2026-09-15

### Added

- `cap update` installs newer versions from the Git repository, preferring
  verified Windows release downloads and falling back to an exact-commit source
  build. `cap update --check` checks without installing.
- Friendly magenta update notices after interactive journal commands, with
  cached background checks and respect for offline, quiet, JSON, plain, and
  no-color output. Saving never waits for the update network check.
- Required tagged GitHub releases with Windows archives and SHA-256 files for
  every version bump, documented in `AGENTS.md`.

## 0.2.0-dev.31 - 2026-09-15

### Fixed

- Quick captures and new writer drafts now use Capsule's normal `entry_` plus
  eight lowercase base-36 characters, checking existing entries for collisions.
  Retries retain their original reserved UUID to preserve recovery identity.

## 0.2.0-dev.30 - 2026-09-15

### Fixed

- Backup verification no longer blocks capture because of existing orphaned
  location/media metadata. The shared core compares foreign-key diagnostics
  against the same source snapshot and retains SQLite integrity checks.
- Backup failures show their underlying cause, state that the entry was not
  saved, and provide the command to retry its retained recovery record.

## 0.2.0-dev.29 - 2026-09-15

### Added

- Comma-separated tags for `cap add --tags life,outdoors,gratitude,exercise`,
  with repeatable `--tag`/`--tags` options and PowerShell completion for each tag.

## 0.2.0-dev.28 - 2026-09-14

### Added

- Isolated console launcher with environment clearing and Windows console-mode
  restoration checks, plus writer/recovery process acceptance coverage.
- Windows CI installation lifecycle and fresh-shell capture checks.
- Verified standalone dev.28 archive and per-user installation with checksums,
  exact synthetic readback and persisted-PATH resolution from a fresh shell.

### Fixed

- Connect explicit/no-argument interactive writer routing and freeze editable
  drafts before recovery retries. Preserve editor output until durable storage.
- Keep long Unicode lines and the caret visible, honor actual terminal width,
  redraw only the ambient rail, and update the saved-draft indicator when idle.

### Changed

- Deliver only the CLI after the user's clarification; restore Capsule's source
  checkout to its original master and retain optional desktop work on branches.

## 0.2.0-dev.27 - 2026-09-14

### Fixed

- Concurrent memory-state tests verify that every successful update survives,
  while allowing the documented busy timeout under contention. A held-lock
  regression verifies unchanged state and successful continuation after release.

### Added

- Complete CLI latency evidence for 20 fresh processes at each of 1k, 10k and
  100k synthetic entries, including exact binary hash and optional warnings.

## 0.2.0-dev.26 - 2026-09-14

### Added

- Moving capsule halves, weather motion from saved conditions, once-only real
  milestone glints, a timed recall unseal, and seven drawn garden plants.
- Executable coverage for all five memory routes, milestone crossings and
  no-context enrichment; direct terminal input dependencies for the writer.

### Fixed

- Memory views use available terminal width and redraw only rows they own.
  Long final bodies preserve earlier scrollback; tall scenes finish statically.
- Receipts honor hidden previews/ASCII icons and label cached observations with
  their original fetched timestamp. Enrichment validates presentation first.
- Capture permits shared-core backup-guarded nullable-ID repair, while ambiguous
  duplicate IDs remain refused. Continuation display aliases freeze to UUIDs.

## 0.2.0-dev.25 - 2026-09-14

### Added

- Draft-backed `cap write` with Unicode/grapheme-safe multiline editing,
  bracketed paste, 500 ms atomic recovery drafts, explicit resume/discard,
  frozen database identity and backup policy, Gauntlet word-target enforcement,
  and safe argument-array external-editor hand-off.

### Fixed

- Made Windows install/uninstall mutations preflight and recoverable across
  binary, metadata, receipt, profile and PATH destinations, with deterministic
  locked-destination coverage.
- Bound `-SkipBuild` packaging to a successful release provenance stamp and
  recorded explicit dirty-checkout state in package manifests.
- Isolated smoke child processes to a minimal environment with asynchronous
  output capture and a bounded timeout.

## 0.2.0-dev.24 - 2026-09-14

### Added

- Integrated durable capture/recovery and all five memory commands against the
  combined shared core, with independent executable acceptance checks for text
  fidelity, metadata, retry binding, failed backups, and input validation.

## 0.2.0-dev.23 - 2026-09-14

### Added

- Added locked Windows release packaging with SHA-256 manifests, provenance and
  notices, plus safe per-user install/update/uninstall scripts.
- Added isolated synthetic smoke guidance and temporary-root installation
  checks; PATH changes preserve existing entries and never activate completion
  profiles unless explicitly requested.

## 0.2.0-dev.22 - 2026-09-14

### Added

- Twenty-sample shared-core benchmark with separate backup, identity/repair,
  transaction and resequencing checkpoint timings on synthetic journals.

## 0.2.0-dev.21 - 2026-09-14

### Fixed

- Live effects gallery leaves all eight source palettes visible after its
  animation, and gradient demos show the five placements on multiple rows.
- The gallery heading wraps within a narrow terminal.

## 0.2.0-dev.20 - 2026-09-14

### Added

- Repeatable independent-process core capture probe using owned synthetic labs,
  kill checkpoints, concurrent writers and database integrity checks.

### Fixed

- `cap status` now retries missing receipt and explicit capture-ID binding
  persistence after a confirmed journal commit, including body-free committed
  pending records left by an earlier storage failure.

## 0.2.0-dev.19 - 2026-09-14

### Added

- Connected quick capture from positional text, files and stdin with metadata,
  dry-run, JSON/quiet/plain output, immediate saved acknowledgements and
  post-commit location/weather enrichment.
- Added atomic cap-local pending records, receipts, explicit-ID bindings,
  conservative discard tombstones, status/recover commands, frozen-request
  replay for future writers and a 15-minute identity-bound weather cache.
- Added process coverage for idempotency/conflicts, database replacement,
  simultaneous same-ID captures, lock contention, crash/unknown commit recovery,
  failed receipt/binding writes, tombstones and pending-draft inspection.
- Added a fixture-root-authorized, non-default `test-hooks` feature for the
  executable recovery fault matrix.

### Changed

- Added a 150 ms TTY-only precommit working indicator and a cap-effects seal
  reveal capped at 650 ms after confirmed commit, with narrow-layout wrapping,
  weather-derived accents and terminal cursor restoration.

### Fixed

- Explicit presentation validation now precedes capture state/backup work;
  database replacement maps to exit 4 while missing/setup errors remain exit 3.
- Unknown/committed outcomes remain retry-safe when local receipt or binding
  persistence fails; expired receipts are removed only under a per-record lock
  with a durable binding.

## 0.2.0-dev.18 - 2026-09-14

### Changed

- Pin the reviewed combined capture/context core with bounded save coordination,
  pre-backup identity checks and honest context persistence outcomes.
- Begin the memory experience package in a separate Luna worktree.
### Memory integration notes

### Changed

- Added guarded pure-frame streaming for memory unseal, garden, and calendar
  views, with non-TTY/static and narrow-width safeguards.
- Bounded recall/body projections and shared milestone/calendar integration now
  preserve complete aggregate counts without buffering authored bodies.

## [0.2.0-dev.17] - 2026-09-14

### Added

- Implemented read-only memory experiences for recall, on-this-day, calendar,
  stats, and the seven-day writing garden, with synthetic date/visibility tests.
- Added database-identity-bound atomic cap state for non-repeating recall and
  true daily/weekly milestone crossing receipts (50/500 words) that fail as
  warnings when optional state or metrics are unavailable.

## [0.2.0-dev.16] - 2026-09-14

### Added

- Connected all R1 read commands to the executable and pinned the reviewed
  read API in the shared Capsule core. Capture remains disabled while its
  mutation/context acceptance corrections are completed.

## [0.2.0-dev.15] - 2026-09-14

### Fixed

- Windows preferences publish through one same-directory rename, preventing
  concurrent readers from seeing a missing file or default settings during updates.
- Bounded handling of Windows sharing/delete-pending errors preserves the last
  valid preferences when another program prevents replacement.

### Added

- Stronger concurrent-read and blocked-replacement regression coverage.

## [0.2.0-dev.14] - 2026-09-14

### Changed

- Updated specification and plan status to reflect the running implementation,
  with independent capture-process review evidence and remaining gates.

### Fixed

- Output-only effects preserve processed input so Ctrl+C reaches the CLI handler;
  terminal guards retain an existing caller-owned raw mode.

### Added

- Windows console interruption probe that targets only its synthetic child group
  and verifies the child's actual cancellation exit code.

## [0.2.0-dev.13] - 2026-09-14

### Added

- Added bounded read commands for exact entry lookup, today/recent lists,
  structured search, tag/mood discovery, context settings, and safe doctor
  diagnostics through the path-bound headless Capsule reader.
- Read command tests cover hidden-entry protection, unchanged fixture rows,
  metadata `items` output, FTS fallback diagnostics, and terminal sanitization.

## [0.2.0-dev.12] - 2026-09-14

### Added

- Integrated theme, preferences, synthetic FX and PowerShell completion commands
  with live guarded animation and process-level output/isolation verification.

### Fixed

- Keep terminal control sequences out of displayed editor preferences.

## [0.2.0-dev.11] - 2026-09-14

### Added

- Process-level capture benchmark using newly generated synthetic journals,
  persisted-row checks, bounded execution and first-output/total latency reports.

## [0.2.0-dev.10] - 2026-09-14

### Fixed

- Sanitize terminal control sequences in human diagnostics and usage errors.

## [0.2.0-dev.9] - 2026-09-14

### Added

- Persistent synthetic lab generator for native interoperability and performance
  checks, including isolated settings and optional 1k/10k/100k entry datasets.

## [0.2.0-dev.8] - 2026-09-14

### Added

- Windows CI with locked dependencies, formatting, Clippy, synthetic tests and a
  standalone release build.
- Version output includes the shared core revision; development pins the tested
  Rust 1.95.0 toolchain, matching the supported compiler declaration.

## [0.2.0-dev.7] - 2026-09-14

### Added

- Reviewed, revision-pinned headless Capsule dependency and shared cooperative
  interruption token for terminal sessions and capture phases.
- Integrated color-cli renderer and complete synthetic schema fixture matrix.

### Fixed

- Corrected the illustrative save receipt's word count in the specification.

## [0.2.0-dev.6] - 2026-09-14

### Added

- Five cap-local terminal themes (`aurora`, `neon`, `c64`, `amber`, `paper`)
  with fictional previews and ASCII/plain fallbacks.
- Synthetic FX gallery for all eight source palettes and five gradient modes,
  bounded through the shared `cap-effects` renderer with cancellation support.
- Atomic, deny-listed cap-local preferences for output modes, writer display
  and target, icon/preview policy, and editor executable/argument arrays.
- Profile-free PowerShell completions with explicit opt-in read-only metadata
  suggestions.

## [0.2.0-dev.5] - 2026-09-14

### Added

- Bounded UTF-8 entry input adapters with BOM handling, exact whitespace retention,
  Capsule newline normalization and content-source validation.

## [0.2.0-dev.4] - 2026-09-14

### Added

- Typed command grammar and handler/output contracts, including explicit literal
  entry syntax, content-source conflicts, bounded pagination and JSON parse/help.
- Process checks ensuring command errors cannot fall through to journal capture.

## [0.2.0-dev.3] - 2026-09-14

### Added

- Bounded Capsule fixture matrix covering full and optional-table schemas,
  FTS5/legacy/absent search tables, ID edge cases, relations and persisted
  location/weather records.
- Strict temporary-path ownership checks, cleared subprocess environments,
  deterministic seeded clocks, disposable SQLite lock guards and logical
  schema/row snapshots for read-only proof.
- Fixture-matrix evidence and focused verification command documentation.
## [0.2.0-dev.2] - 2026-09-14

### Added

- Rust `cap-effects` color-cli port with the eight source palettes, five
  gradients, smoothstep reveal, bounded shimmer timing, Unicode-cell layout,
  ANSI capability negotiation, sanitization, and RAII terminal restoration.
- Synthetic visual-QA example and pinned reference fixtures/provenance. No
  Python runtime or live Capsule journal is required.

## [0.2.0-dev.1] - 2026-09-14

### Added

- Rust workspace foundation, stable JSON envelope, effects crate boundary, and
  isolated synthetic Capsule fixtures for the coordinated implementation.
- Contract and implementation tracking documents. Capture remains unavailable
  until the shared backend and recovery work is integrated.

## [0.1.0] - 2026-09-14

### Added

- Source-grounded specification for the Capsule CLI, shared journal/context
  behavior, recovery, terminal effects and color-cli reuse.
- Feature implementation plan with Luna task/worktree ownership, dependencies,
  review loops, integration checks and staged release gates.
- Repository overview distinguishing the proposed interface from implemented
  functionality. This is a planning revision, not an executable release.
