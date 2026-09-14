# Console and presentation review

2026-09-14, Windows, development `0.2.0-dev.26`. All content was synthetic.

- Native console processes exercised capture, recall, garden and calendar with
  full motion and forced color. Capture printed its Saved UUID before the seal.
  The stream showed shell travel, an opening capsule, growing plants, static
  final entry text and cursor restoration; all processes exited successfully.
- This run exposed two defects: applying the 40-cell receipt limit to richer
  memory views, and clearing future rows before a larger final frame. Memory
  views now allow 120 cells, limited by actual width minus one. Redraw clears
  only previous owned rows. A tall intermediate scene finishes statically.
  A second native console run passed after these corrections with a preceding
  scrollback sentinel, a full-width garden and one final recall body.
- Pure-frame checks prove shell positions change, the final shell closes,
  rain frames differ over time, and no authored body enters the save animation.
  Cached weather displays its fetched timestamp. Hidden previews and ASCII
  decoration settings are tested at narrow width.
- Save/recall use at most 650ms, garden 400ms, calendar a static frame. The
  final garden legend is appended once after its small grow-in scene. JSON,
  plain and quiet process tests cover every memory command with no ANSI.
- Executable capture tests cover real threshold crossing, once-only replay,
  no-context enrichment without another entry, nullable-ID repair after a backup
  preserving the original rows, and refusal of ambiguous duplicate IDs.
- A numeric continuation alias is resolved to its UUID before pending storage;
  recovery reads that frozen identity and successfully retries after a blocked
  backup destination is repaired.

These are native console and automated checks, not a Windows Terminal visual
recording or proof of native Capsule focus/scroll behavior. The desktop visual
gate remains pending because the interactive desktop was locked/unavailable.
Provider motion uses saved facts; live provider credentials were not exercised.
