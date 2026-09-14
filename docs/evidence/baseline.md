# Baseline — 2026-09-14

- cap planning baseline: `16851be2d2e77cacde99fe84c730ef4912b368af`.
- Capsule baseline: `5db5502388d5e62e48d75c2e5bbcfab41984118a` (0.37.0).
- color-cli reference: `c813f12f8578283b68fa124944c0078f15ccdec3` (1.0.0).
- Both source working trees were clean before implementation. No live journal was read.
- Installed Rust/Cargo: 1.95.0. No `cap` command resolved before installation.
- Integration branch: `codex/cap-integration`; upstream branch reserved:
  `codex/cap-core-integration` in Capsule.
- Fixture schema is adapted from Capsule entries tests, with synthetic visible/
  hidden entries and FTS5. All subprocess config/DB/backup/state paths are isolated.
- Extraction worker expands the required schema/capability matrix; broader fault,
  concurrency, context and UI evidence is pending.
