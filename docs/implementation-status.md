# cap implementation status

Updated: 2026-09-14. Orchestrator: `01a0a088-f06f-7fd1-8024-efdb9922bf6f`.

Foundation, fixtures, effects, personality, journal reads, capture/recovery, memory commands, and headless-core extraction are integrated. R1/R2
feature commands are still being implemented. No live journal is a test fixture.

| Package | Task ID | Worktree | Branch | Status |
| --- | --- | --- | --- | --- |
| WP00 | orchestrator | `C:\_code\cap` | `codex/cap-integration` | accepted foundation; integration ongoing |
| WP01 | `01a0a09a-d509-7832-867b-480a29f340f4` | `C:\Users\jtill\.codex\worktrees\2cda\capsule_tauri` | `codex/cap-core` | accepted `d1a02b8`; idle |
| WP04 | `01a0a09b-ad1c-7873-91c7-bda97168c0d3` | `C:\Users\jtill\.codex\worktrees\0e5f\cap` | `codex/cap-effects` | accepted `08537d7` as `a0a46b2`; idle |
| WP00 fixtures | `01a0a09d-f1d2-77e2-a79f-ad802a30f727` | `C:\Users\jtill\.codex\worktrees\258f\cap` | `codex/cap-fixtures` | accepted `1a6cb4b` as `48fad28`; idle |
| WP07 | `01a0a0bc-810d-7e42-a5d9-8bc6d7767da9` | `C:\Users\jtill\.codex\worktrees\48b0\cap` | `codex/cap-personality` | accepted `66dcfe4` + `ce5dd49`; idle |
| WP02 | `01a0a0be-0e57-7f41-9968-52050b5d0ddb` | `C:\Users\jtill\.codex\worktrees\f687\capsule_tauri` | `codex/cap-capture-core` | accepted `528eadc7`; integrated in `4272d53`; idle |
| WP03 | `01a0a0be-81fd-7f41-b949-a88d04f9eaf8` | `C:\Users\jtill\.codex\worktrees\638b\capsule_tauri` | `codex/cap-context` | accepted `1d38cf1` plus root persistence corrections in `279e1cb`/`4272d53`; idle |
| WP06 | `01a0a0e4-8c56-7431-ad98-2185e98d377f` | `C:\Users\jtill\.codex\worktrees\7b2c\cap` | `codex/cap-reads` | accepted `8d1d469` as `6d045b2`; core `6e7f792` + `0066097` |
| WP09 | `01a0a120-56cf-7de0-9d0e-907556af4f99` | `C:\Users\jtill\.codex\worktrees\0135\cap` | `codex/cap-memories` | accepted `94915c6`; core integrated in `4888a2a` |
| WP05 | `01a0a126-0c9c-7ab3-9685-4268e34fc1f9` | `C:\Users\jtill\.codex\worktrees\46e3\cap` | `codex/cap-capture-cli` | accepted `e91f901` + `7d8192a`; root motion review active |
| WP10 | `01a0a127-151c-7b80-9134-33a63e426279` | `C:\Users\jtill\.codex\worktrees\ac36\capsule_tauri` | `codex/cap-external-refresh` | integrated `7c0c9e4` + `0972289`; scroll follow-up active |

Every worker uses `gpt-5.6-luna` with `max` reasoning. Owned files and dependencies
are specified in PLAN.md and each task brief. WP02/WP03 communicate directly about
the shared backup/transaction boundary. Root owns final registration and review.

Accepted evidence:

- Core worker: 42 core tests; 76 desktop tests passed, one existing provider smoke
  ignored; core/desktop Clippy and desktop check passed. Required cleanup completed.
- Root: pinned Git core compiles; synthetic core-boundary tests verify projection,
  path authority, missing config/schema, safe diagnostics and unchanged DB content.
- Effects: 24 unit and two source-reference tests; root native PTY demonstration
  completed in 2260 ms / 137 frames, with cursor restoration. This is console
  evidence, not a Windows Terminal visual recording.
- Full cap workspace tests and Clippy pass after integrating these components.
- Personality: 32 root-linked unit tests and three additional process scenarios
  pass (five tests including fixture self-checks). The process tests verify
  restart persistence, no Capsule mutations, fictional preview isolation and
  plain/quiet/JSON/forced-color output. Native console FX streamed successfully
  and restored its cursor; Windows Terminal visual evidence remains outstanding.
- Windows CI passed for `cba8895`, including a release build from the pinned Git
  core without sibling repositories: [run 34870690578](https://github.com/soundtrackgeek/cap/actions/runs/34870690578).
- Native Capsule QA build (0.37.0 + core `d1a02b8`) succeeded with an isolated
  app identity, WebView directory and synthetic journal. UI verification is
  pending because the desktop returned access denied while showing a screen saver.
- Native console interruption uncovered output-only raw mode swallowing Ctrl+C.
  Root fixed the input-mode ownership and confirmed live frames before process
  completion, Ctrl+C cancellation/cursor restoration, and a separate targeted
  CTRL_BREAK child probe returning exit 130 after 816 ms. This proves console
  behavior, not a Windows Terminal visual recording.
- Windows CI also passed for `f9f614e`, including the cancellation correction:
  [run 34873434626](https://github.com/soundtrackgeek/cap/actions/runs/34873434626).
- Root's independent process review of WP02 candidate `8ae93e4` passed eight
  concurrent writers, kills after backup/before commit/after commit, read-only
  reconciliation and exact-once replay, same-path database replacement rejection,
  and backup-failure refusal. Integrity, references, FTS and retained backups were
  checked. The combined sidecar/SQLite timeout failed at 19.5 seconds against a
  15-second budget and was returned to the owner. The corrected immutable
  `528eadc7d3ff666f851449ec899c901c39a1f9f2` passed the entire independent
  process probe, including combined contention at 16,141 ms end to end
  (process startup included), typed busy/no mutation, and retry after process death.
- Combined core `4272d53` passed 97 tests and strict Clippy. Root added checks
  that abandoned context cannot claim unsaved weather and that enrichment rejects
  a replaced database before backup publication or retention.
- Separating startup from operation timing exposed remaining Windows timeout
  overshoot. Root fixed it in `8881d30`: the stronger permanent process probe
  passes at 15,001 ms inside capture, 15,022 ms end to end. Core now passes 98
  tests. See [complete capture evidence](evidence/core-capture.md).
- Preliminary shared-core measurements on new synthetic journals (three debug
  process samples, filesystem cache uncontrolled): 1k entries 98–111 ms; 10k
  311–313 ms; 100k 2536–2651 ms. These are not final CLI p50/p95 measurements.
- Expanded release core benchmark: 20 samples per size, p50/p95 of 89/112 ms
  (1k), 295/409 ms (10k), 2448/4969 ms (100k). Most measured time is verified
  backup work; [phase timings and limits](evidence/latency.md) distinguish the
  core from the still-pending CLI capture benchmark.
- Root shared `WriterPreferences` snapshot at Capsule `a2e7829` passed two
  safe/default/Gauntlet tests, six desktop settings regressions and core Clippy.
  WP08 can consume it after the final shared-core pin is integrated.
- WP06: read-only core/search adapters passed 91 core and 74 desktop tests
  (one live smoke ignored). Root pinned `0066097`, connected all eight read
  commands, and passed the full cap workspace plus four process checks (two
  scenarios and two fixture self-checks). Process tests exercise all eight
  commands, pagination, hidden entries, malformed presentation settings and
  explicit missing paths; journal snapshots remain unchanged.
- Root reproduced a Windows preference replacement race on repetition 14,
  corrected publication, and passed 30 strengthened repetitions, all seven
  preference tests and personality process checks. See [state-file evidence](evidence/windows-state.md).

The memory package may begin from the accepted read/presentation APIs while
capture corrections finish. This scheduling overlap does not waive any R1/R2
release gate. Capture remains disabled in the executable until its write and
context APIs pass combined acceptance.

WP10 implementation also overlaps the CLI work now that the shared core and
desktop regression tests pass. Native R1 interoperability remains a release gate;
this scheduling adjustment neither waives it nor substitutes mock UI evidence.

Capture/recovery and memory candidates are now integrated: cap `e91f901` and
`7d8192a`, memory `94915c6`, shared core `4888a2a`. Combined verification passes
111 core tests, 80 desktop tests (one live provider test ignored), 54 frontend
tests, and the cap workspace including seven independent capture acceptance
checks. Test-hook process checks pass 15 scenarios. These use synthetic data.

WP08 writer is next; WP11 delivery runs in task
`01a0a168-3923-73d1-b823-ad6c90db4596`, worktree
`C:\Users\jtill\.codex\worktrees\b3cd\cap`, branch `codex/cap-delivery`.
WP10 follow-up targets actual scroll restoration and deferred selection.
Root motion integration is complete in development: moving seal/weather,
once-only milestone receipt, streaming unseal and drawn garden. A native console
run found and verified fixes for memory width and scrollback redraw ownership;
see [presentation evidence](evidence/presentation.md). WP08 is active in task
`01a0a172-c0a1-7800-9252-96aa2b9b6b4f`, worktree
`C:\Users\jtill\.codex\worktrees\b988\cap`, branch `codex/cap-writer`.
Final writer/delivery integration, installation and native acceptance remain outstanding.
