//! Native terminal adapter for the writer.
//!
//! The event loop itself remains small because editing semantics live in
//! [`super::input`]. This module owns raw/alternate/bracketed-paste/focus
//! setup, bounded redraws and unconditional restoration on every exit path.

use std::io::{self, Write};

use capsule_core::contracts::CaptureRequest;
use crossterm::{
    cursor::{MoveTo, Show},
    event::{
        self, DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
        Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::{
    preferences::EffectivePresentation,
    ui::themes::Border,
    writer::input::{InputEvent, Key, WriterMachine},
};

/// Owns all terminal modes used by an interactive writer. Drop is a second
/// restoration path for I/O errors, panics and Ctrl+C.
pub struct TerminalSession<'a, W: Write> {
    writer: &'a mut W,
    raw_enabled: bool,
    alternate_entered: bool,
    bracketed_enabled: bool,
    focus_enabled: bool,
    restored: bool,
}

impl<'a, W: Write> TerminalSession<'a, W> {
    pub fn new(writer: &'a mut W) -> io::Result<Self> {
        let mut session = Self {
            writer,
            raw_enabled: false,
            alternate_entered: false,
            bracketed_enabled: false,
            focus_enabled: false,
            restored: false,
        };
        let setup = (|| {
            if !terminal::is_raw_mode_enabled()? {
                terminal::enable_raw_mode()?;
                session.raw_enabled = true;
            }
            execute!(session.writer, EnterAlternateScreen)?;
            session.alternate_entered = true;
            execute!(session.writer, EnableBracketedPaste)?;
            session.bracketed_enabled = true;
            execute!(session.writer, EnableFocusChange)?;
            session.focus_enabled = true;
            execute!(session.writer, Clear(ClearType::All), MoveTo(0, 0), Show)?;
            session.writer.flush()
        })();
        if let Err(error) = setup {
            let _ = session.restore();
            return Err(error);
        }
        Ok(session)
    }

    pub fn writer(&mut self) -> &mut W {
        self.writer
    }

    pub fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        let mut first_error = None;
        if self.bracketed_enabled {
            if let Err(error) = execute!(self.writer, DisableBracketedPaste) {
                first_error.get_or_insert(error);
            }
            self.bracketed_enabled = false;
        }
        if self.focus_enabled {
            if let Err(error) = execute!(self.writer, DisableFocusChange) {
                first_error.get_or_insert(error);
            }
            self.focus_enabled = false;
        }
        if self.alternate_entered {
            if let Err(error) = execute!(self.writer, Show, LeaveAlternateScreen) {
                first_error.get_or_insert(error);
            }
            self.alternate_entered = false;
        } else if let Err(error) = execute!(self.writer, Show) {
            first_error.get_or_insert(error);
        }
        if let Err(error) = self.writer.flush() {
            first_error.get_or_insert(error);
        }
        if self.raw_enabled {
            if let Err(error) = terminal::disable_raw_mode() {
                first_error.get_or_insert(error);
            }
            self.raw_enabled = false;
        }
        self.restored = true;
        first_error.map_or(Ok(()), Err)
    }
}

impl<W: Write> Drop for TerminalSession<'_, W> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

/// Translate one crossterm event. Unsupported mouse/colour events return
/// `None`; callers simply continue polling. Key releases are ignored so a
/// terminal's key-repeat stream cannot duplicate text.
pub fn read_input_event() -> io::Result<Option<InputEvent>> {
    match event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Release => Ok(None),
        Event::Key(key) => Ok(map_key(key).map(InputEvent::Key)),
        Event::Paste(text) => Ok(Some(InputEvent::Paste(text))),
        Event::FocusGained => Ok(Some(InputEvent::Focus(true))),
        Event::FocusLost => Ok(Some(InputEvent::Focus(false))),
        Event::Resize(width, height) => Ok(Some(InputEvent::Resize { width, height })),
        _ => Ok(None),
    }
}

fn map_key(key: crossterm::event::KeyEvent) -> Option<Key> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('s') | KeyCode::Char('S') if control => Some(Key::CtrlS),
        KeyCode::Char('c') | KeyCode::Char('C') if control => Some(Key::CtrlC),
        KeyCode::Char(character) if !control => Some(Key::Char(character)),
        KeyCode::Enter => Some(Key::Enter),
        KeyCode::Tab => Some(Key::Char('\t')),
        KeyCode::Backspace => Some(Key::Backspace),
        KeyCode::Delete => Some(Key::Delete),
        KeyCode::Left => Some(Key::Left),
        KeyCode::Right => Some(Key::Right),
        KeyCode::Up => Some(Key::Up),
        KeyCode::Down => Some(Key::Down),
        KeyCode::Home => Some(if control {
            Key::DocumentStart
        } else {
            Key::Home
        }),
        KeyCode::End => Some(if control { Key::DocumentEnd } else { Key::End }),
        KeyCode::PageUp => Some(Key::DocumentStart),
        KeyCode::PageDown => Some(Key::DocumentEnd),
        KeyCode::Esc => Some(Key::Escape),
        _ => None,
    }
}

/// Draw one bounded writer frame. The text and metadata are sanitized at
/// render time, and no body/caret animation is performed. `ambient_phase` is
/// `None` while typing, unfocused, reduced/off, or on an unsupported terminal.
pub fn render_frame<W: Write>(
    writer: &mut W,
    machine: &WriterMachine,
    request: &CaptureRequest,
    presentation: &EffectivePresentation,
    ambient_phase: Option<f64>,
) -> io::Result<()> {
    render_frame_with_notice(
        writer,
        machine,
        request,
        presentation,
        None,
        ambient_phase,
        None,
    )
}

/// Render with an optional target and transient warning. The compact wrapper
/// above keeps a stable API for callers that do not need either.
pub fn render_frame_with_notice<W: Write>(
    writer: &mut W,
    machine: &WriterMachine,
    request: &CaptureRequest,
    presentation: &EffectivePresentation,
    target: Option<usize>,
    ambient_phase: Option<f64>,
    notice: Option<&str>,
) -> io::Result<()> {
    let width = usize::from(machine.width()).max(20);
    let height = usize::from(machine.height()).max(5);
    let border = if presentation.output.color() == cap_effects::ColorMode::Plain {
        Border::ascii()
    } else {
        presentation.theme.border()
    };
    let horizontal_len = width.saturating_sub(2);
    let inner_width = width.saturating_sub(4).max(4);
    let body_width = inner_width;
    let body_capacity = height.saturating_sub(6).max(1);
    let (cursor_line, _) = machine.buffer().cursor_position();
    let first_line = cursor_line.saturating_sub(body_capacity.saturating_sub(1));

    execute!(writer, Clear(ClearType::All), MoveTo(0, 0))?;
    write!(
        writer,
        "{}{}{}\r\n",
        border.top_left,
        border.horizontal.to_string().repeat(horizontal_len),
        border.top_right
    )?;

    let destination = cap_effects::sanitize_text(&request.database_path.to_string_lossy());
    let header = format!(
        "{}  {}",
        if machine.dirty() {
            "CAPSULE WRITER *"
        } else {
            "CAPSULE WRITER"
        },
        truncate_cells(&destination, inner_width.saturating_sub(18))
    );
    write_panel_row(writer, border.vertical, &header, inner_width)?;

    for index in 0..body_capacity {
        let line_index = first_line + index;
        let line = machine.buffer().line(line_index).unwrap_or_default();
        let rendered = truncate_cells(&cap_effects::sanitize_text(line), body_width);
        write!(writer, "{}", border.vertical)?;
        if let Some(phase) = ambient_phase {
            write_rail(writer, presentation, phase, border.vertical)?;
        } else {
            writer.write_all(border.vertical.to_string().as_bytes())?;
        }
        write!(
            writer,
            "{:width$} {}\r\n",
            rendered,
            border.vertical,
            width = body_width
        )?;
    }

    let target = target.or(presentation
        .writer_target_override
        .map(|target| target as usize));
    let target = target.map(|target| format!(" / target {target}"));
    let target = target.as_deref().unwrap_or("");
    let footer = format!(
        "{} words{} · {} · {}",
        machine.word_count(),
        target,
        if machine.focused() {
            "focused"
        } else {
            "paused"
        },
        "Ctrl+S save · Ctrl+C keep draft"
    );
    write_panel_row(writer, border.vertical, &footer, inner_width)?;
    write!(
        writer,
        "{}{}{}\r\n",
        border.bottom_left,
        border.horizontal.to_string().repeat(horizontal_len),
        border.bottom_right
    )?;

    if let Some(notice) = notice.filter(|value| !value.is_empty()) {
        write!(
            writer,
            "\r\nWarning: {}",
            cap_effects::sanitize_text(notice)
        )?;
    }

    let cursor_cells = machine.buffer().cell_column();
    let cursor_row = 2 + cursor_line.saturating_sub(first_line);
    let cursor_row = cursor_row.min(height.saturating_sub(1));
    let cursor_col = (2 + cursor_cells).min(width.saturating_sub(1));
    execute!(writer, MoveTo(cursor_col as u16, cursor_row as u16))?;
    writer.flush()
}

fn write_panel_row<W: Write>(
    writer: &mut W,
    vertical: char,
    text: &str,
    width: usize,
) -> io::Result<()> {
    let rendered = truncate_cells(text, width);
    write!(
        writer,
        "{} {:<width$} {}\r\n",
        vertical,
        rendered,
        vertical,
        width = width
    )
}

fn write_rail<W: Write>(
    writer: &mut W,
    presentation: &EffectivePresentation,
    phase: f64,
    glyph: char,
) -> io::Result<()> {
    let rgb = cap_effects::palette_color_for(presentation.theme.palette(), phase);
    match presentation.output.color() {
        cap_effects::ColorMode::TrueColor => write!(
            writer,
            "\x1b[38;2;{};{};{}m{}\x1b[0m",
            rgb.0, rgb.1, rgb.2, glyph
        ),
        cap_effects::ColorMode::Ansi256 => write!(
            writer,
            "\x1b[38;5;{}m{}\x1b[0m",
            cap_effects::ansi256_index(rgb),
            glyph
        ),
        cap_effects::ColorMode::Ansi16 => {
            let (code, bright) = cap_effects::ansi16_code(rgb);
            let code = if bright { code + 60 } else { code };
            write!(writer, "\x1b[{}m{}\x1b[0m", code, glyph)
        }
        cap_effects::ColorMode::Plain => writer.write_all(glyph.to_string().as_bytes()),
    }
}

fn truncate_cells(text: &str, width: usize) -> String {
    let width = width.max(1);
    let layout = cap_effects::layout_text(text, width, false);
    let row = layout
        .rows
        .first()
        .map(|row| row.text())
        .unwrap_or_default();
    if layout.rows.len() > 1 {
        let suffix = "…";
        let suffix_width = cap_effects::grapheme_width(suffix);
        let content = cap_effects::layout_text(text, width.saturating_sub(suffix_width), false)
            .rows
            .first()
            .map(|row| row.text())
            .unwrap_or_default();
        format!("{content}{suffix}")
    } else {
        row
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        preferences::{IconMode, PreviewVisibility, WriterDisplay},
        ui::themes::Theme,
        writer::input::WriterMachine,
    };
    use chrono::{FixedOffset, TimeZone};
    use std::path::PathBuf;

    fn request() -> CaptureRequest {
        CaptureRequest::new(
            "hello",
            "cap_writer_test",
            "entry_writer_test",
            PathBuf::from("C:\\lab\\capsule.db"),
            FixedOffset::east_opt(3600)
                .unwrap()
                .with_ymd_and_hms(2026, 9, 14, 12, 0, 0)
                .unwrap(),
        )
    }

    fn presentation() -> EffectivePresentation {
        EffectivePresentation {
            theme: Theme::Aurora,
            output: cap_effects::ResolvedOutputMode::Human {
                color: cap_effects::ColorMode::Plain,
                motion: cap_effects::MotionMode::Off,
            },
            icon_mode: IconMode::Ascii,
            preview_visibility: PreviewVisibility::Never,
            writer_display: WriterDisplay::Compact,
            writer_target_override: None,
            editor_executable: None,
            editor_args: Vec::new(),
        }
    }

    #[test]
    fn plain_frame_contains_metadata_without_ansi_or_body_animation() {
        let machine = WriterMachine::new("hello\nworld", 50, 12);
        let mut output = Vec::new();
        render_frame(&mut output, &machine, &request(), &presentation(), None).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("CAPSULE WRITER"));
        assert!(output.contains("hello"));
        assert!(output.contains("Ctrl+S save"));
        assert!(!output.contains("\x1b[38;"));
    }

    #[test]
    fn input_mapping_keeps_ctrl_s_and_ctrl_c_out_of_authored_text() {
        let save = crossterm::event::KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        let ctrl_c = crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(map_key(save), Some(Key::CtrlS));
        assert_eq!(map_key(ctrl_c), Some(Key::CtrlC));
    }
}
