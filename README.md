# cap

A planned command-line companion for [Capsule](https://github.com/soundtrackgeek/capsule_tauri).
Capture a journal entry from the terminal, using Capsule's active database,
location settings and weather, with a little color-cli ceremony when it saves.

This repository currently contains the design and implementation plan. The
commands below describe the proposed interface; there is no executable to install
yet.

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

The planning work inspected source only; it did not inspect or modify journal
entries. Feature implementation and agent task creation are for a later session.
