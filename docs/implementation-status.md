# cap implementation status

Updated: 2026-09-14. Orchestrator: `01a0a088-f06f-7fd1-8024-efdb9922bf6f`.

Foundation, fixtures, effects, personality, and headless-core extraction are integrated. R1/R2
feature commands are still being implemented. No live journal is a test fixture.

| Package | Task ID | Worktree | Branch | Status |
| --- | --- | --- | --- | --- |
| WP00 | orchestrator | `C:\_code\cap` | `codex/cap-integration` | accepted foundation; integration ongoing |
| WP01 | `01a0a09a-d509-7832-867b-480a29f340f4` | `C:\Users\jtill\.codex\worktrees\2cda\capsule_tauri` | `codex/cap-core` | accepted `d1a02b8`; idle |
| WP04 | `01a0a09b-ad1c-7873-91c7-bda97168c0d3` | `C:\Users\jtill\.codex\worktrees\0e5f\cap` | `codex/cap-effects` | accepted `08537d7` as `a0a46b2`; idle |
| WP00 fixtures | `01a0a09d-f1d2-77e2-a79f-ad802a30f727` | `C:\Users\jtill\.codex\worktrees\258f\cap` | `codex/cap-fixtures` | accepted `1a6cb4b` as `48fad28`; idle |
| WP07 | `01a0a0bc-810d-7e42-a5d9-8bc6d7767da9` | `C:\Users\jtill\.codex\worktrees\48b0\cap` | `codex/cap-personality` | accepted `66dcfe4` + `ce5dd49`; idle |
| WP02 | `01a0a0be-0e57-7f41-9968-52050b5d0ddb` | `C:\Users\jtill\.codex\worktrees\f687\capsule_tauri` | `codex/cap-capture-core` | active; base `d1a02b8` |
| WP03 | `01a0a0be-81fd-7f41-b949-a88d04f9eaf8` | `C:\Users\jtill\.codex\worktrees\638b\capsule_tauri` | `codex/cap-context` | active; base `d1a02b8` |
| WP06 | `01a0a0e4-8c56-7431-ad98-2185e98d377f` | `C:\Users\jtill\.codex\worktrees\7b2c\cap` | `codex/cap-reads` | active; base `3b5182f` |

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

WP05, WP06, WP08–WP12 and the release/native acceptance gates remain outstanding.
