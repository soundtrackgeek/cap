# cap-effects

Pure rendering primitives and guarded terminal output for `cap`. The crate
does not open Capsule databases, read journal models, or require Python/color-cli
at runtime.

## API shape

1. `sanitize_text` and `layout_text` remove terminal controls and wrap by
   Unicode grapheme/cell width. The resulting `TextLayout` owns all strings.
2. `EffectConfig`, `AnimationPlan`, `frame_at`, and `final_frame` calculate
   deterministic frames from an injected elapsed time/seed (`FrameInput`). `EffectConfig::quick_save`
   scales the source timeline into the 650 ms receipt budget; `explicit_demo`
   allows the three-second gallery budget.
3. `render_frame` (or the convenience `render_text`) converts an `EffectFrame`
   to `TrueColor`, `Ansi256`, `Ansi16`, or `Plain` text. No ANSI styling is
   stored in the frame or journal model.
4. `resolve_output_mode` applies `json > quiet > plain > explicit settings >
   capabilities`. `animate_text` is the guarded I/O entry point; use
   `animate_text_with_cancel` with an `AtomicBool` callback owned by the CLI so
   Ctrl+C can unwind normally and restore the terminal.

The source-backed helpers `PALETTES`, `palette_color`, `lerp_rgb`, `pick_color`,
`hsv_to_rgb`, and `intensity` are public for receipts, themes, and reference
tests. `TerminalGuard` restores color, cursor visibility, alternate-screen, and
raw mode on normal return, I/O error, or unwinding.
Output-only animations keep processed input enabled so the caller's Ctrl+C
handler receives signals. A writer that consumes raw keyboard events opts into
raw mode explicitly; nested guards preserve raw mode already owned by a caller.

## Synthetic visual QA

The example deliberately uses synthetic text and never touches the journal:

```powershell
cargo run -p cap-effects --example color_cli_demo
```

Reference provenance and the captured Python outputs are in
[`docs/provenance/color-cli.md`](../../docs/provenance/color-cli.md) and
[`tests/fixtures/color-cli/reference.json`](../../tests/fixtures/color-cli/reference.json).
