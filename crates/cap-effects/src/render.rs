//! Conversion of pure frames to ANSI or plain terminal text.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::effect::{final_frame, EffectConfig, EffectFrame, StyledCell};
use crate::layout::layout_text;
use crate::palette::Rgb;

/// The terminal colour protocol selected after capability negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    TrueColor,
    Ansi256,
    Ansi16,
    Plain,
}

impl ColorMode {
    pub const fn emits_ansi(self) -> bool {
        !matches!(self, Self::Plain)
    }
}

/// Render one frame. The result contains only the frame's own rows and never
/// cursor movement; an animation loop owns redraw/cursor policy separately.
pub fn render_frame(frame: &EffectFrame, mode: ColorMode) -> String {
    let mut output = String::new();
    for row in &frame.rows {
        output.push_str(&render_row(row, mode));
    }
    output
}

/// Render one row, including its terminating newline. Keeping this operation
/// separate lets a redraw clear each allocated line rather than repeatedly
/// clearing only the cursor's current line.
pub fn render_row(row: &[StyledCell], mode: ColorMode) -> String {
    let mut output = String::new();
    for cell in row {
        if cell.visible {
            append_cell(&mut output, cell, mode);
        } else {
            // Preserve cell geometry while a grapheme is unrevealed. A
            // zero-width combining cluster contributes no placeholder.
            output.extend(std::iter::repeat_n(' ', cell.cell_width));
        }
    }
    if mode.emits_ansi() {
        output.push_str("\x1b[0m");
    }
    output.push('\n');
    output
}

/// Render a fully revealed frame without any animation state.
pub fn render_static(frame: &EffectFrame, mode: ColorMode) -> String {
    render_frame(frame, mode)
}

/// Convenience path for a static themed line/receipt. It performs the same
/// sanitisation, cell layout and source-backed colour calculation as the
/// animation path, without requiring a caller to assemble those pieces.
pub fn render_text(text: &str, width: usize, config: &EffectConfig, mode: ColorMode) -> String {
    let layout = layout_text(text, width, false);
    render_frame(&final_frame(&layout, config), mode)
}

/// Render plain text from a frame, guaranteeing that no escape byte is
/// emitted even if a caller accidentally passes an ANSI mode.
pub fn render_plain(frame: &EffectFrame) -> String {
    render_frame(frame, ColorMode::Plain)
}

fn append_cell(output: &mut String, cell: &StyledCell, mode: ColorMode) {
    match mode {
        ColorMode::Plain => output.push_str(&cell.text),
        ColorMode::TrueColor => {
            let _ = write!(
                output,
                "\x1b[38;2;{};{};{}m{}",
                cell.color.0, cell.color.1, cell.color.2, cell.text
            );
        }
        ColorMode::Ansi256 => {
            let _ = write!(
                output,
                "\x1b[38;5;{}m{}",
                ansi256_index(cell.color),
                cell.text
            );
        }
        ColorMode::Ansi16 => {
            let (code, bright) = ansi16_code(cell.color);
            let _ = write!(
                output,
                "\x1b[{}{}m{}",
                if bright { 9 } else { 3 },
                code,
                cell.text
            );
        }
    }
}

/// Map RGB to the xterm 256-colour cube/greyscale ramp.
pub fn ansi256_index(rgb: Rgb) -> u8 {
    let channels = [rgb.0, rgb.1, rgb.2];
    if channels[0] == channels[1] && channels[1] == channels[2] {
        let value = channels[0];
        if value < 8 {
            return 16;
        }
        if value > 248 {
            return 231;
        }
        return (232.0 + (f64::from(value) - 8.0) / 247.0 * 24.0).round() as u8;
    }
    let to_cube = |channel: u8| (f64::from(channel) / 255.0 * 5.0).round() as u8;
    16 + 36 * to_cube(rgb.0) + 6 * to_cube(rgb.1) + to_cube(rgb.2)
}

/// Return the nearest ANSI-16 palette slot and whether its bright variant is
/// selected. The mapping is deterministic and does not change stored text.
pub fn ansi16_code(rgb: Rgb) -> (u8, bool) {
    const COLORS: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (128, 0, 0),
        (0, 128, 0),
        (128, 128, 0),
        (0, 0, 128),
        (128, 0, 128),
        (0, 128, 128),
        (192, 192, 192),
        (128, 128, 128),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (0, 0, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    let mut best = 0usize;
    let mut best_distance = u32::MAX;
    for (index, candidate) in COLORS.iter().enumerate() {
        let distance = channel_distance(rgb, *candidate);
        if distance < best_distance {
            best_distance = distance;
            best = index;
        }
    }
    if best < 8 {
        (best as u8, false)
    } else {
        ((best - 8) as u8, true)
    }
}

fn channel_distance(a: Rgb, b: Rgb) -> u32 {
    let dr = i32::from(a.0) - i32::from(b.0);
    let dg = i32::from(a.1) - i32::from(b.1);
    let db = i32::from(a.2) - i32::from(b.2);
    (dr * dr + dg * dg + db * db) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::{final_frame, EffectConfig};
    use crate::layout::layout_text;

    #[test]
    fn truecolor_and_plain_are_unambiguous() {
        let frame = final_frame(&layout_text("Hi", 10, false), &EffectConfig::default());
        let truecolor = render_frame(&frame, ColorMode::TrueColor);
        assert!(truecolor.contains("\x1b[38;2;"));
        let plain = render_plain(&frame);
        assert_eq!(plain, "Hi\n");
        assert!(!plain.contains('\x1b'));
    }

    #[test]
    fn render_text_convenience_path_sanitizes_and_wraps() {
        let plain = render_text(
            "ok\x1b[31m!",
            40,
            &EffectConfig::default(),
            ColorMode::Plain,
        );
        assert_eq!(plain, "ok!\n");
    }

    #[test]
    fn reduced_palettes_emit_valid_sequences() {
        let frame = final_frame(&layout_text("x", 10, false), &EffectConfig::default());
        let ansi256 = render_frame(&frame, ColorMode::Ansi256);
        assert!(ansi256.contains("\x1b[38;5;"));
        let ansi16 = render_frame(&frame, ColorMode::Ansi16);
        assert!(ansi16.contains("\x1b[3") || ansi16.contains("\x1b[9"));
    }

    #[test]
    fn color_mapping_stays_in_supported_ranges() {
        let _ = ansi256_index((255, 0, 255));
        assert_eq!(ansi16_code((255, 255, 255)), (7, true));
        assert_eq!(ansi16_code((0, 0, 0)), (0, false));
    }
}
