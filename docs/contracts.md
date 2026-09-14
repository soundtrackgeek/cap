# Implementation contracts — revision 1

This document freezes ownership and boundary rules for the first implementation
wave. SPEC.md remains the user-visible behavior contract.

## Stable CLI boundary

`src/contracts.rs` owns `OutputEnvelope<T>`, `CliError`, `SaveState`, and `Page<T>`.
Schema version is 1, JSON field names camelCase, save state values snake_case.
No core or renderer calls stdout directly for machine output. `src/output.rs`
writes exactly one JSON object plus newline. The orchestrator owns these files.
The current bootstrap parser is temporary; it must never claim an entry was saved.

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
