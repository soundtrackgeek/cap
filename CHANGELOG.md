# Changelog

## [0.2.0-dev.5] - 2026-09-14

### Added

- Bounded UTF-8 entry input adapters with BOM handling, exact whitespace retention,
  Capsule newline normalization and content-source validation.

## [0.2.0-dev.4] - 2026-09-14

### Added

- Typed command grammar and handler/output contracts, including explicit literal
  entry syntax, content-source conflicts, bounded pagination and JSON parse/help.
- Process checks ensuring command errors cannot fall through to journal capture.

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
