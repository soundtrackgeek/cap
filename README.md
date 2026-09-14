# cap

A planned command-line companion for [Capsule](https://github.com/soundtrackgeek/capsule_tauri).
Capture a journal entry from the terminal, using Capsule's active database,
location settings and weather, with a little color-cli ceremony when it saves.

Implementation is in progress on `codex/cap-integration`. The foundation builds,
but capture is not connected yet. The commands below describe the intended
interface; do not use the development build for journal capture yet.

```powershell
cap Had a lovely walk by the water
cap add --mood content --tag life -- 'A quiet evening outside.'
Get-Content -Raw .\today.md | cap
```

- [SPEC.md](SPEC.md): behavior, command contract, shared data architecture,
  recovery, color-cli reuse, visual effects, accessibility and release criteria.
- [PLAN.md](PLAN.md): feature ownership, Luna task/worktree waves, review loops,
  integration gates and evidence required before release.
- [CHANGELOG.md](CHANGELOG.md): repository history.

Implementation is planned around a native Rust `cap.exe`, a shared headless
Capsule core, and a Rust port of the palettes/fade/shimmer from
[color-cli](https://github.com/soundtrackgeek/color-cli).

Development: `cargo test --workspace` and `cargo build`. Work package progress is
tracked in [docs/implementation-status.md](docs/implementation-status.md).
Tests use synthetic temporary databases; the live journal is not a test fixture.
The bounded Capsule compatibility matrix can be run with
`cargo test --test fixture_matrix --locked`; its schema/row snapshot and
isolation evidence is recorded in [docs/evidence/fixtures.md](docs/evidence/fixtures.md).
