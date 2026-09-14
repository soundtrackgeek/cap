# cap

A planned command-line companion for [Capsule](https://github.com/soundtrackgeek/capsule_tauri).
Capture a journal entry from the terminal, using Capsule's active database,
location settings and weather, with a little color-cli ceremony when it saves.

Implementation is in progress on `codex/cap-integration`. Capture is not
connected yet, but the standalone effects foundation is runnable for synthetic
visual QA. Do not use the development build for journal capture yet.

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
The development build supports `--help`, `--version`, and JSON help/usage errors;
feature commands are connected only after their work packages pass review.
Tests use synthetic temporary databases; the live journal is not a test fixture.
The shared Capsule core is fetched from a reviewed Git revision recorded in
Cargo.toml/Cargo.lock; a local Capsule or Python checkout is not needed to build.
The bounded Capsule compatibility matrix can be run with
`cargo test --test fixture_matrix --locked`; its schema/row snapshot and
isolation evidence is recorded in [docs/evidence/fixtures.md](docs/evidence/fixtures.md).

The effects crate can be previewed without Capsule or Python:

```powershell
cargo run -p cap-effects --example color_cli_demo
```

It uses owned Unicode-safe layouts, source-backed palettes/gradients, bounded
fade/shimmer frames, capability-aware ANSI/plain output, and terminal-state
restoration. The example text is synthetic and is never saved.
