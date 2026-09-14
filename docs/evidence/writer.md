# Writer verification

2026-09-14, Windows ConPTY, integrated cap 0.2.0-dev.28. The helper
`examples/lab_command.rs` clears child environment and binds every Capsule path
to generated `cap lab 53160-1789417484844646800` under the OS temp directory.
This is console evidence, not a Windows Terminal screenshot or physical-device
focus/resize recording. No real journal or Capsule configuration was used.

- Keyboard input saved exactly `A quiet Ålesund evening.\nSecond line.`.
- Bracketed paste inserted `Pasted first line.\nSecond Å line.` as text, without
  saving. Ctrl+C kept its recovery draft and the child returned 130.
- A new process prompted to resume; Ctrl+S saved the same reserved UUID and
  original request time. `recent --json` returned the exact Unicode/newline body.
- A real configured PowerShell child editor wrote a seven-word multiline body;
  the writer saved it through the same capture path and removed its owned temp
  only after durable recovery storage.
- The launcher checked console input/output modes before and after each command:
  `[Some(503), Some(7)]` was restored for save, cancel, resume and editor handoff.
  The outer PowerShell wrapper may report exit 1 for the child's exit 130; the
  launcher records the actual child code.

Automated regressions cover grapheme editing, paste/control-key separation,
target/Gauntlet rules, final-edit persistence before destination validation,
editor-file retention after a real child edit plus failed persistence, immutable
submission, non-TTY no-mutation, recovery retry and long Unicode viewport width.
Ambient frames contain only rail updates; authored body/header text and whole
screen clearing are absent. Idle persistence updates the saved-draft indicator.

Known limits: no claim of physical keyboard-layout coverage, every third-party
editor's behavior, live provider availability or Windows Terminal visual approval.
