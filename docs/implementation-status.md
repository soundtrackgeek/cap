# cap implementation status

Updated: 2026-09-14. Orchestrator: `01a0a088-f06f-7fd1-8024-efdb9922bf6f`.

Foundation, fixtures, effects, and headless-core extraction are integrated. R1/R2
feature commands are still being implemented. No live journal is a test fixture.

| Package | Task ID | Worktree | Branch | Status |
| --- | --- | --- | --- | --- |
| WP00 | orchestrator | `C:\_code\cap` | `codex/cap-integration` | accepted foundation; integration ongoing |
| WP01 | `01a0a09a-d509-7832-867b-480a29f340f4` | `C:\Users\jtill\.codex\worktrees\2cda\capsule_tauri` | `codex/cap-core` | accepted `d1a02b8`; idle |
| WP04 | `01a0a09b-ad1c-7873-91c7-bda97168c0d3` | `C:\Users\jtill\.codex\worktrees\0e5f\cap` | `codex/cap-effects` | accepted `08537d7` as `a0a46b2`; idle |
| WP00 fixtures | `01a0a09d-f1d2-77e2-a79f-ad802a30f727` | `C:\Users\jtill\.codex\worktrees\258f\cap` | `codex/cap-fixtures` | accepted `1a6cb4b` as `48fad28`; idle |
| WP07 | `01a0a0bc-810d-7e42-a5d9-8bc6d7767da9` | `C:\Users\jtill\.codex\worktrees\48b0\cap` | `codex/cap-personality` | active; base `a0a46b2` |
| WP02 | `01a0a0be-0e57-7f41-9968-52050b5d0ddb` | `C:\Users\jtill\.codex\worktrees\f687\capsule_tauri` | `codex/cap-capture-core` | active; base `d1a02b8` |
| WP03 | `01a0a0be-81fd-7f41-b949-a88d04f9eaf8` | `C:\Users\jtill\.codex\worktrees\638b\capsule_tauri` | `codex/cap-context` | active; base `d1a02b8` |

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

WP05, WP06, WP08–WP12 and the release/native acceptance gates remain outstanding.
