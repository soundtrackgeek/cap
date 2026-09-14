//! Terminal capability negotiation, output-mode precedence and guarded I/O.

use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use crossterm::terminal;
use serde::{Deserialize, Serialize};

use crate::effect::{final_frame, frame_at, AnimationPlan, EffectConfig, EffectFrame};
use crate::layout::{layout_text, DEFAULT_LAYOUT_WIDTH};
use crate::render::{render_frame, render_row, ColorMode};

const RESET: &[u8] = b"\x1b[0m";
const HIDE_CURSOR: &[u8] = b"\x1b[?25l";
const SHOW_CURSOR: &[u8] = b"\x1b[?25h";
const ENTER_ALT: &[u8] = b"\x1b[?1049h";
const LEAVE_ALT: &[u8] = b"\x1b[?1049l";
const CLEAR_LINE: &[u8] = b"\x1b[2K";

/// Enable/check virtual-terminal processing before emitting literal ANSI.
/// Crossterm performs the Windows `SetConsoleMode` update for us; non-Windows
/// terminals already consume ANSI sequences directly.
pub fn prepare_ansi_output() -> bool {
    #[cfg(windows)]
    {
        crossterm::ansi_support::supports_ansi()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// A snapshot of terminal/environment capabilities. Construct this once per
/// command and pass it down; renderers never read process environment while
/// generating a frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCapabilities {
    pub is_tty: bool,
    pub width: usize,
    pub term: Option<String>,
    pub term_dumb: bool,
    pub no_color: bool,
    pub ci: bool,
    pub truecolor: bool,
    pub ansi256: bool,
    pub ansi16: bool,
    pub ansi_support: bool,
}

impl Default for TerminalCapabilities {
    fn default() -> Self {
        Self {
            is_tty: false,
            width: 80,
            term: None,
            term_dumb: false,
            no_color: false,
            ci: false,
            truecolor: false,
            ansi256: false,
            ansi16: false,
            ansi_support: false,
        }
    }
}

impl TerminalCapabilities {
    /// Detect stdout/environment capabilities without emitting output.
    pub fn detect() -> Self {
        let is_tty = io::stdout().is_terminal();
        let width = terminal::size()
            .map(|(columns, _)| usize::from(columns))
            .unwrap_or(80);
        let term = std::env::var("TERM").ok();
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        let ci = std::env::var_os("CI").is_some_and(|value| !value.is_empty());
        let colorterm = std::env::var("COLORTERM").ok();
        let wt_session = std::env::var_os("WT_SESSION").is_some_and(|value| !value.is_empty());
        let mut capabilities = Self::from_environment_snapshot(
            is_tty,
            width,
            term.as_deref(),
            colorterm.as_deref(),
            wt_session,
            no_color,
            ci,
        );
        if is_tty && !prepare_ansi_output() {
            capabilities.ansi_support = false;
            capabilities.truecolor = false;
            capabilities.ansi256 = false;
            capabilities.ansi16 = false;
        }
        capabilities
    }

    /// Construct a deterministic capability snapshot. `WT_SESSION` is treated
    /// as Windows Terminal's truecolor/VT signal even when `TERM` and
    /// `COLORTERM` are absent, which is common on native Windows.
    pub fn from_environment_snapshot(
        is_tty: bool,
        width: usize,
        term: Option<&str>,
        colorterm: Option<&str>,
        wt_session: bool,
        no_color: bool,
        ci: bool,
    ) -> Self {
        let term_dumb = term.is_some_and(|value| value.eq_ignore_ascii_case("dumb"));
        let colorterm_lower = colorterm.unwrap_or_default().to_ascii_lowercase();
        let term_lower = term.unwrap_or_default().to_ascii_lowercase();
        // A supplied snapshot is deliberately deterministic. `detect()` adds
        // the live Windows VT probe; this constructor is also safe to use in
        // tests that emulate Windows Terminal without a native console.
        let ansi_support = true;
        let truecolor = (wt_session
            || colorterm_lower.contains("truecolor")
            || colorterm_lower.contains("24bit")
            || term_lower.contains("direct")
            || term_lower.contains("truecolor"))
            && ansi_support;
        let ansi256 = (truecolor || term_lower.contains("256color")) && ansi_support;
        Self {
            is_tty,
            width: width.max(1),
            term: term.map(str::to_owned),
            term_dumb,
            no_color,
            ci,
            truecolor,
            ansi256,
            ansi16: is_tty && !term_dumb && ansi_support,
            ansi_support,
        }
    }

    /// Deterministic constructor useful for tests and synthetic demos.
    pub fn synthetic(
        is_tty: bool,
        width: usize,
        truecolor: bool,
        ansi256: bool,
        term_dumb: bool,
        no_color: bool,
        ci: bool,
    ) -> Self {
        Self {
            is_tty,
            width: width.max(1),
            term: Some(if term_dumb { "dumb" } else { "xterm" }.to_owned()),
            term_dumb,
            no_color,
            ci,
            truecolor,
            ansi256: ansi256 || truecolor,
            ansi16: is_tty && !term_dumb,
            ansi_support: true,
        }
    }

    pub fn preferred_color_mode(&self) -> ColorMode {
        if self.truecolor {
            ColorMode::TrueColor
        } else if self.ansi256 {
            ColorMode::Ansi256
        } else if self.ansi16 {
            ColorMode::Ansi16
        } else {
            ColorMode::Plain
        }
    }

    pub fn compact_width(&self) -> usize {
        self.width.saturating_sub(1).clamp(1, DEFAULT_LAYOUT_WIDTH)
    }
}

/// Explicit colour request from a CLI/config layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    TrueColor,
    Ansi256,
    Ansi16,
    Never,
}

/// Explicit motion request from a CLI/config layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MotionChoice {
    #[default]
    Auto,
    Full,
    Reduced,
    Off,
}

/// Flags consumed by [`resolve_output_mode`]. JSON/quiet/plain are mutually
/// exclusive at the CLI boundary; if more than one is set, the SPEC order is
/// still deterministic: JSON > quiet > plain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OutputRequest {
    pub json: bool,
    pub quiet: bool,
    pub plain: bool,
    pub color: ColorChoice,
    pub motion: MotionChoice,
}

/// Effective output state after applying SPEC precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedOutputMode {
    Json,
    Quiet,
    Human {
        color: ColorMode,
        motion: MotionMode,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionMode {
    Full,
    Reduced,
    Off,
}

impl ResolvedOutputMode {
    pub const fn is_machine(self) -> bool {
        matches!(self, Self::Json | Self::Quiet)
    }

    pub const fn color(self) -> ColorMode {
        match self {
            Self::Human { color, .. } => color,
            Self::Json | Self::Quiet => ColorMode::Plain,
        }
    }

    pub const fn motion(self) -> MotionMode {
        match self {
            Self::Human { motion, .. } => motion,
            Self::Json | Self::Quiet => MotionMode::Off,
        }
    }
}

/// Apply `--json > --quiet > --plain > explicit color/motion > settings >
/// automatic capabilities` without allowing machine/plain modes to leak ANSI
/// or animation.
pub fn resolve_output_mode(
    request: OutputRequest,
    caps: &TerminalCapabilities,
) -> ResolvedOutputMode {
    if request.json {
        return ResolvedOutputMode::Json;
    }
    if request.quiet {
        return ResolvedOutputMode::Quiet;
    }
    if request.plain {
        return ResolvedOutputMode::Human {
            color: ColorMode::Plain,
            motion: MotionMode::Off,
        };
    }

    let automatic_static = !caps.is_tty || caps.term_dumb || caps.ci;
    let color = match request.color {
        ColorChoice::Never => ColorMode::Plain,
        ColorChoice::Always => caps.preferred_color_mode().non_plain_fallback(),
        ColorChoice::TrueColor => {
            if caps.truecolor {
                ColorMode::TrueColor
            } else {
                caps.preferred_color_mode().non_plain_fallback()
            }
        }
        ColorChoice::Ansi256 => {
            if caps.ansi256 {
                ColorMode::Ansi256
            } else {
                caps.preferred_color_mode().non_plain_fallback()
            }
        }
        ColorChoice::Ansi16 => {
            if caps.ansi16 {
                ColorMode::Ansi16
            } else {
                ColorMode::Plain
            }
        }
        ColorChoice::Auto => {
            if automatic_static || caps.no_color {
                ColorMode::Plain
            } else {
                caps.preferred_color_mode()
            }
        }
    };

    if color == ColorMode::Plain {
        return ResolvedOutputMode::Human {
            color,
            motion: MotionMode::Off,
        };
    }
    let motion = match request.motion {
        MotionChoice::Off => MotionMode::Off,
        MotionChoice::Reduced => MotionMode::Reduced,
        MotionChoice::Full => MotionMode::Full,
        MotionChoice::Auto if automatic_static => MotionMode::Off,
        MotionChoice::Auto => MotionMode::Full,
    };
    ResolvedOutputMode::Human { color, motion }
}

trait NonPlainFallback {
    fn non_plain_fallback(self) -> ColorMode;
}

impl NonPlainFallback for ColorMode {
    fn non_plain_fallback(self) -> ColorMode {
        if self == ColorMode::Plain {
            // An explicit colour request is still allowed to produce a basic
            // ANSI sequence when capability probing is inconclusive.
            ColorMode::Ansi16
        } else {
            self
        }
    }
}

/// Which terminal state the guard should own and restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardOptions {
    pub hide_cursor: bool,
    pub reset_color: bool,
    pub alternate_screen: bool,
    pub raw_mode: bool,
}

impl Default for GuardOptions {
    fn default() -> Self {
        Self {
            hide_cursor: true,
            reset_color: true,
            alternate_screen: false,
            raw_mode: false,
        }
    }
}

impl GuardOptions {
    pub fn for_animation(_caps: &TerminalCapabilities) -> Self {
        // Output-only effects do not consume keyboard events. Keeping cooked
        // input lets the process Ctrl+C handler receive the interrupt signal.
        Self::default()
    }

    pub fn for_demo(caps: &TerminalCapabilities) -> Self {
        Self {
            alternate_screen: caps.is_tty,
            raw_mode: false,
            ..Self::default()
        }
    }
}

/// RAII terminal state guard. Drop attempts the same reset path as normal
/// completion, so render errors and unwinding do not leave the cursor hidden or
/// the alternate screen active. Raw mode is disabled only if this guard enabled
/// it.
pub struct TerminalGuard<'a, W: Write> {
    writer: &'a mut W,
    cursor_hidden: bool,
    color_dirty: bool,
    alternate_entered: bool,
    raw_enabled: bool,
    restored: bool,
}

impl<'a, W: Write> TerminalGuard<'a, W> {
    pub fn new(writer: &'a mut W, options: GuardOptions) -> io::Result<Self> {
        let mut guard = Self {
            writer,
            cursor_hidden: false,
            color_dirty: false,
            alternate_entered: false,
            raw_enabled: false,
            restored: false,
        };
        if options.raw_mode && !crossterm::terminal::is_raw_mode_enabled()? {
            crossterm::terminal::enable_raw_mode()?;
            guard.raw_enabled = true;
        }
        let setup = (|| {
            if options.alternate_screen {
                guard.writer.write_all(ENTER_ALT)?;
                guard.alternate_entered = true;
            }
            if options.reset_color {
                guard.writer.write_all(RESET)?;
                guard.color_dirty = true;
            }
            if options.hide_cursor {
                guard.writer.write_all(HIDE_CURSOR)?;
                guard.cursor_hidden = true;
            }
            guard.writer.flush()
        })();
        if let Err(error) = setup {
            let _ = guard.restore();
            return Err(error);
        }
        Ok(guard)
    }

    pub fn writer(&mut self) -> &mut W {
        self.writer
    }

    pub fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        let mut first_error = None;
        if self.color_dirty {
            if let Err(error) = self.writer.write_all(RESET) {
                first_error.get_or_insert(error);
            }
            self.color_dirty = false;
        }
        if self.cursor_hidden {
            if let Err(error) = self.writer.write_all(SHOW_CURSOR) {
                first_error.get_or_insert(error);
            }
            self.cursor_hidden = false;
        }
        if self.alternate_entered {
            if let Err(error) = self.writer.write_all(LEAVE_ALT) {
                first_error.get_or_insert(error);
            }
            self.alternate_entered = false;
        }
        if let Err(error) = self.writer.flush() {
            first_error.get_or_insert(error);
        }
        if self.raw_enabled {
            if let Err(error) = crossterm::terminal::disable_raw_mode() {
                first_error.get_or_insert(error);
            }
            self.raw_enabled = false;
        }
        self.restored = true;
        first_error.map_or(Ok(()), Err)
    }

    pub const fn is_restored(&self) -> bool {
        self.restored
    }
}

impl<W: Write> Drop for TerminalGuard<'_, W> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

/// Outcome of a bounded animation loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationResult {
    pub frames_rendered: usize,
    pub resized: bool,
    pub cancelled: bool,
    pub elapsed: Duration,
}

/// Render a synthetic/user-provided message according to strict output modes.
/// This is the guarded I/O entry point; all colour and timing decisions are
/// still computed by the pure functions in `effect` and `render`.
pub fn animate_text<W: Write>(
    writer: &mut W,
    text: &str,
    config: &EffectConfig,
    request: OutputRequest,
    caps: &TerminalCapabilities,
) -> io::Result<AnimationResult> {
    animate_text_with_cancel(writer, text, config, request, caps, || false)
}

/// Cancellable variant for a CLI that owns a process-level Ctrl+C handler.
/// The callback should be lock-free (for example an `AtomicBool::load`); when
/// it returns true the loop stops and the [`TerminalGuard`] restores terminal
/// state before this function returns.
pub fn animate_text_with_cancel<W: Write, F: Fn() -> bool>(
    writer: &mut W,
    text: &str,
    config: &EffectConfig,
    request: OutputRequest,
    caps: &TerminalCapabilities,
    should_cancel: F,
) -> io::Result<AnimationResult> {
    let mode = resolve_output_mode(request, caps);
    let layout = layout_text(text, caps.compact_width(), false);
    let plan = AnimationPlan::for_layout(&layout, config);
    if mode.is_machine() {
        return Ok(AnimationResult {
            frames_rendered: 0,
            resized: false,
            cancelled: false,
            elapsed: Duration::ZERO,
        });
    }
    let mut output_color = mode.color();
    if output_color.emits_ansi() && !caps.ansi_support {
        // A caller may have supplied a synthetic snapshot; on a real native
        // Windows console, do not emit raw escape bytes unless VT was enabled.
        output_color = ColorMode::Plain;
    }
    if mode.motion() != MotionMode::Full {
        writer.write_all(render_frame(&final_frame(&layout, config), output_color).as_bytes())?;
        writer.flush()?;
        return Ok(AnimationResult {
            frames_rendered: 1,
            resized: false,
            cancelled: false,
            elapsed: Duration::ZERO,
        });
    }

    let options = GuardOptions::for_animation(caps);
    let mut guard = TerminalGuard::new(writer, options)?;
    let started = Instant::now();
    let mut next_frame = 0usize;
    let mut previous_rows = 0usize;
    let mut resized = false;
    let mut cancelled = false;
    loop {
        if should_cancel() {
            cancelled = true;
            break;
        }
        let elapsed = started.elapsed();
        let elapsed_seconds = elapsed.as_secs_f64();
        let current_width = terminal::size()
            .map(|(columns, _)| usize::from(columns))
            .unwrap_or(caps.width);
        let frame = if current_width != caps.width {
            resized = true;
            // Reflow against the new width before finishing. Merely marking
            // the old layout static could still overflow after a narrow
            // terminal resize.
            let resized_layout = layout_text(
                text,
                current_width
                    .saturating_sub(1)
                    .clamp(1, DEFAULT_LAYOUT_WIDTH),
                false,
            );
            let mut frame = final_frame(&resized_layout, config);
            frame.resized = true;
            frame
        } else {
            frame_at(&layout, config, elapsed_seconds)
        };
        write_redraw(guard.writer(), &frame, output_color, previous_rows)?;
        previous_rows = frame.rows.len();
        next_frame = next_frame.saturating_add(1);
        if frame.done || resized || next_frame >= plan.max_frames {
            break;
        }
        let target = Duration::from_secs_f64(
            (next_frame as f64 * plan.frame_interval_seconds).min(plan.end_seconds),
        );
        let elapsed_now = started.elapsed();
        if target > elapsed_now {
            std::thread::sleep(target - elapsed_now);
        }
    }
    let elapsed = started.elapsed();
    guard.restore()?;
    Ok(AnimationResult {
        frames_rendered: next_frame,
        resized,
        cancelled,
        elapsed,
    })
}

fn write_redraw<W: Write>(
    writer: &mut W,
    frame: &EffectFrame,
    color: ColorMode,
    previous_rows: usize,
) -> io::Result<()> {
    if previous_rows > 0 {
        write!(writer, "\x1b[{}A", previous_rows)?;
        let rows_to_clear = previous_rows.max(frame.rows.len());
        for index in 0..rows_to_clear {
            writer.write_all(CLEAR_LINE)?;
            if index + 1 < rows_to_clear {
                writer.write_all(b"\n")?;
            }
        }
        if rows_to_clear > 1 {
            write!(writer, "\x1b[{}A", rows_to_clear - 1)?;
        }
    }
    for row in &frame.rows {
        writer.write_all(CLEAR_LINE)?;
        writer.write_all(render_row(row, color).as_bytes())?;
    }
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_precedence_and_static_fallback_are_strict() {
        let tty = TerminalCapabilities::synthetic(true, 80, true, true, false, false, false);
        assert_eq!(
            resolve_output_mode(
                OutputRequest {
                    json: true,
                    quiet: true,
                    plain: true,
                    color: ColorChoice::Always,
                    motion: MotionChoice::Full,
                },
                &tty
            ),
            ResolvedOutputMode::Json
        );
        assert_eq!(
            resolve_output_mode(
                OutputRequest {
                    plain: true,
                    color: ColorChoice::Always,
                    motion: MotionChoice::Full,
                    ..OutputRequest::default()
                },
                &tty
            ),
            ResolvedOutputMode::Human {
                color: ColorMode::Plain,
                motion: MotionMode::Off
            }
        );
        let redirected =
            TerminalCapabilities::synthetic(false, 80, true, true, false, false, false);
        assert_eq!(
            resolve_output_mode(OutputRequest::default(), &redirected),
            ResolvedOutputMode::Human {
                color: ColorMode::Plain,
                motion: MotionMode::Off
            }
        );
    }

    #[test]
    fn no_color_and_term_dumb_disable_automatic_color() {
        let no_color = TerminalCapabilities::synthetic(true, 80, true, true, false, true, false);
        assert_eq!(
            resolve_output_mode(OutputRequest::default(), &no_color).color(),
            ColorMode::Plain
        );
        let dumb = TerminalCapabilities::synthetic(true, 80, true, true, true, false, false);
        assert_eq!(
            resolve_output_mode(OutputRequest::default(), &dumb).motion(),
            MotionMode::Off
        );
    }

    #[test]
    fn windows_terminal_signal_supplies_truecolor_without_term() {
        let windows_terminal = TerminalCapabilities::from_environment_snapshot(
            true, 120, None, None, true, false, false,
        );
        assert!(windows_terminal.truecolor);
        assert!(windows_terminal.ansi256);
        assert_eq!(
            windows_terminal.preferred_color_mode(),
            ColorMode::TrueColor
        );
    }

    #[test]
    fn guard_restores_cursor_and_color_on_explicit_restore_and_drop() {
        let mut output = Vec::new();
        {
            let mut guard = TerminalGuard::new(
                &mut output,
                GuardOptions {
                    raw_mode: false,
                    alternate_screen: false,
                    ..GuardOptions::default()
                },
            )
            .unwrap();
            guard.restore().unwrap();
            assert!(guard.is_restored());
        }
        let text = String::from_utf8(output).unwrap();
        assert!(text.starts_with("\x1b[0m\x1b[?25l"));
        assert!(text.ends_with("\x1b[0m\x1b[?25h"));
    }

    #[test]
    fn redirected_animation_is_plain_and_does_not_emit_ansi() {
        let caps = TerminalCapabilities::synthetic(false, 40, true, true, false, false, false);
        let mut output = Vec::new();
        let result = animate_text(
            &mut output,
            "synthetic",
            &EffectConfig::default(),
            OutputRequest::default(),
            &caps,
        )
        .unwrap();
        assert_eq!(result.frames_rendered, 1);
        let text = String::from_utf8(output).unwrap();
        assert!(!text.contains('\x1b'));
        assert!(text.contains("synthetic"));
    }

    #[test]
    fn cancellation_callback_restores_guarded_output() {
        let caps = TerminalCapabilities::synthetic(false, 40, true, true, false, false, false);
        let mut output = Vec::new();
        let result = animate_text_with_cancel(
            &mut output,
            "synthetic",
            &EffectConfig::explicit_demo(),
            OutputRequest {
                color: ColorChoice::Always,
                motion: MotionChoice::Full,
                ..OutputRequest::default()
            },
            &caps,
            || true,
        )
        .unwrap();
        assert!(result.cancelled);
        assert_eq!(result.frames_rendered, 0);
        let text = String::from_utf8(output).unwrap();
        assert!(text.starts_with("\x1b[0m\x1b[?25l"));
        assert!(text.ends_with("\x1b[0m\x1b[?25h"));
    }

    #[test]
    fn malicious_controls_are_removed_before_static_output() {
        let caps = TerminalCapabilities::synthetic(false, 40, false, false, false, false, false);
        let mut output = Vec::new();
        animate_text(
            &mut output,
            "safe\x1b]8;;https://evil.test\x07click\x1b[31m",
            &EffectConfig::default(),
            OutputRequest::default(),
            &caps,
        )
        .unwrap();
        let text = String::from_utf8(output).unwrap();
        assert_eq!(text, "safeclick\n");
        assert!(!text.contains('\x1b'));
    }
}
