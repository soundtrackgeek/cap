# Implementation contracts — revision 1

This document freezes ownership and boundary rules for the first implementation
wave. SPEC.md remains the user-visible behavior contract.

## Stable CLI boundary

`src/contracts.rs` owns `OutputEnvelope<T>`, `CliError`, `SaveState`, and `Page<T>`.
Schema version is 1, JSON field names camelCase, save state values snake_case.
No core or renderer calls stdout directly for machine output. `src/output.rs`
writes exactly one JSON object plus newline. The orchestrator owns these files.
The central typed clap grammar is in `src/cli.rs`. Workers implement command
handlers with `fn run(args: &CommandArgs, global: &GlobalOptions) ->
Result<CommandOutput, AppError>` (or a documented variant for multiple actions).
`CommandOutput` and `AppError` are in `src/app.rs`. Return JSON data plus human text,
optional quiet UUID and warnings; the root main emits the envelope. Creation may
emit a guarded immediate human acknowledgement after commit, never in JSON/quiet.
The orchestrator owns command registration in `app::execute`, `main`, and `lib`.
Ask before changing the CLI structs. `--json --help` is itself a JSON envelope.
Commands not yet connected return an explicit implementation error, never Saved.
Creation handlers set `CommandOutput.committed = true` after a known commit; main
preserves successful save status if its final output stream breaks. A failed
post-commit enrichment/detail/receipt step must return committed output + warning,
not a generic AppError. Unknown commit state uses exit 6 and recovery data.
`src/input.rs` provides `from_words`, `read_add`, `read_utf8`, `normalize` and the
1 MiB input limit. WP05 consumes these tested adapters and assumes ownership when
dispatched; it must preserve authored whitespace and reject invalid UTF-8.

`src/cancellation.rs` exposes `install() -> Result<Arc<AtomicBool>, String>` and
`requested() -> bool`. Install after input acquisition, before an operation with
cooperative cancellation. Effects poll the token; raw writer Ctrl+C events may
set the same token. A confirmed commit remains successful on interruption;
pre-commit interruption retains the recovery record. Never exit inside the signal
callback or depend on Drop running after an unhandled process termination.

## Shared Capsule core

WP01 owns `crates/capsule-core` in the Capsule repository. Expose the extracted
`db`, `backup`, `entries`, `location`, `models` modules publicly as appropriate,
with explicit-path entry points. Preserve existing desktop models/serialization.
Do not create a dependency from this core onto the cap CLI.

The core worker proposes concrete `ResolvedCapsule`, `CapabilityReport`, capture
request/receipt and context policy/result APIs to this task before freezing them.
Those APIs must accept explicit settings/paths rather than mutate process env.
WP02 later owns capture/identity/backup changes, WP03 context/provider changes.
No duplicate entry SQL, whole Tauri dependency, or absolute source-file includes.

## Effects boundary

WP04 owns `crates/cap-effects` except its manifest is already scaffolded and may
be changed by WP04 for crate-local needs. Its public API uses owned strings,
primitive colors/cell sizes and std I/O, never DB models. Provide pure timed frame
generation plus guarded terminal output. Export concrete APIs and examples in its
crate README so the receipt/personality workers can consume them. Ask before
changing root dependencies or shared CLI DTOs. Synthetic demos must be marked.

## Execution and review

The orchestrator task ID is `01a0a088-f06f-7fd1-8024-efdb9922bf6f` on host `local`.
Use the app's send-message tool to report API proposals, ownership requests,
commit/test evidence, or concrete dependency needs. Do not repeatedly send idle
status. Peers may coordinate directly once their IDs are in implementation-status.
Never ask a peer to modify outside that peer's assigned ownership.

Each worker owns its worktree and feature branch. README/CHANGELOG changes needed
by AGENTS.md are authorized in that worktree; announce them in the handoff so the
orchestrator resolves concurrent integration deliberately. Root manifest/lockfile
ownership requires a specific token except initial core extraction in Capsule.
