//! Grapheme-aware terminal layout and control-sequence sanitisation.

use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The maximum width used by compact receipt callers unless they request a
/// different bound.  The layout code accepts any positive width for tests and
/// explicit demos.
pub const DEFAULT_LAYOUT_WIDTH: usize = 40;

/// A single terminal row, including optional centering padding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutRow {
    /// Grapheme clusters in display order.  A cluster may contain multiple
    /// Unicode scalar values (for example `e` plus a combining acute accent).
    pub graphemes: Vec<String>,
    /// Display-cell width of this row, including centering padding.
    pub cell_width: usize,
    /// Number of cells before the first non-padding grapheme.
    pub left_padding: usize,
}

impl LayoutRow {
    pub fn text(&self) -> String {
        self.graphemes.concat()
    }

    pub fn is_empty(&self) -> bool {
        self.graphemes.is_empty() || self.graphemes.iter().all(String::is_empty)
    }
}

/// Sanitised and wrapped text ready for pure frame generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextLayout {
    pub rows: Vec<LayoutRow>,
    pub width: usize,
    pub total_cells: usize,
}

impl TextLayout {
    pub fn plain_lines(&self) -> Vec<String> {
        self.rows.iter().map(LayoutRow::text).collect()
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

/// Remove terminal controls from user/provider text before it reaches a TTY.
///
/// Newlines are retained for layout. Tabs are normalised to one space because
/// their width depends on the cursor column. Other C0 controls are discarded.
/// CSI, OSC and other string escape sequences are removed in their entirety,
/// including OSC payloads terminated by BEL or ST (`ESC \\`).
pub fn sanitize_text(input: &str) -> String {
    let mut chars = input.chars().peekable();
    let mut output = String::with_capacity(input.len());

    while let Some(ch) = chars.next() {
        match ch {
            '\x07' => {}
            '\x1b' => sanitize_escape(&mut chars),
            '\u{009b}' => skip_csi(&mut chars),
            '\u{009d}' | '\u{0090}' | '\u{009e}' | '\u{009f}' => skip_string_control(&mut chars),
            '\r' => {
                // A carriage return can otherwise overwrite a prior receipt
                // line. Treat CR as a line break, matching Capsule's text
                // normalisation while keeping terminal output safe.
                if chars.peek() != Some(&'\n') {
                    output.push('\n');
                }
            }
            '\n' => output.push('\n'),
            '\t' => output.push(' '),
            c if c.is_control() => {}
            c => output.push(c),
        }
    }
    output
}

fn sanitize_escape<I>(chars: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = char>,
{
    match chars.next() {
        Some('[') => skip_csi(chars),
        Some(']') | Some('P') | Some('^') | Some('_') | Some('X') => skip_string_control(chars),
        Some('(') | Some(')') | Some('*') | Some('+') | Some('-') | Some('.') => {
            // ESC designation sequences consume one additional character.
            chars.next();
        }
        Some(_) | None => {}
    }
}

fn skip_csi<I>(chars: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = char>,
{
    // CSI ends at a final byte in the range 0x40..=0x7e.  Parameter and
    // intermediate bytes are all ignored. If a malformed sequence has no
    // final byte, the remaining control-looking text is still discarded.
    for c in chars.by_ref() {
        if ('@'..='~').contains(&c) {
            break;
        }
    }
}

fn skip_string_control<I>(chars: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = char>,
{
    while let Some(c) = chars.next() {
        if c == '\x07' || c == '\u{009c}' {
            break;
        }
        if c == '\x1b' && chars.peek() == Some(&'\\') {
            chars.next();
            break;
        }
        // An ESC inside an unterminated string is still part of the payload;
        // continue looking for BEL/ST.
    }
}

/// Return a grapheme's terminal-cell width, treating unprintable/unknown
/// clusters as a one-cell replacement.
pub fn grapheme_width(grapheme: &str) -> usize {
    let width = UnicodeWidthStr::width(grapheme);
    if width > 0 {
        width
    } else if grapheme
        .chars()
        .all(|character| UnicodeWidthChar::width(character).unwrap_or(0) == 0)
    {
        0
    } else {
        // Unknown printable clusters get one cell rather than silently
        // disappearing from the layout.
        1
    }
}

/// Wrap sanitised text to a terminal-cell width and optionally centre rows.
///
/// Wrapping never splits a grapheme cluster. A wide cluster that cannot fit in
/// a one-cell layout is represented by `�`, preserving the no-overflow
/// guarantee for deliberately tiny test widths.
pub fn layout_text(input: &str, requested_width: usize, center: bool) -> TextLayout {
    let width = requested_width.max(1);
    let sanitized = sanitize_text(input);
    let trimmed = sanitized.trim_end_matches('\n');
    let mut rows: Vec<Vec<String>> = Vec::new();

    for raw_line in trimmed.split('\n') {
        if raw_line.is_empty() {
            rows.push(Vec::new());
            continue;
        }

        let mut current: Vec<String> = Vec::new();
        let mut current_width = 0usize;
        for grapheme in raw_line.graphemes(true) {
            let mut owned = grapheme.to_owned();
            let mut cluster_width = grapheme_width(grapheme);
            if cluster_width > width {
                // Keep a visible, safe marker instead of emitting a glyph
                // that would necessarily overflow this row.
                owned = "�".to_owned();
                cluster_width = 1;
            }
            if current_width > 0 && current_width + cluster_width > width {
                rows.push(std::mem::take(&mut current));
                current_width = 0;
            }
            current.push(owned);
            current_width += cluster_width;
        }
        rows.push(current);
    }

    while rows.last().is_some_and(Vec::is_empty) {
        rows.pop();
    }
    if rows.is_empty() {
        rows.push(Vec::new());
    }

    let mut layout_rows = Vec::with_capacity(rows.len());
    let mut total_cells = 0usize;
    for row in rows {
        let content_width = row.iter().map(|g| grapheme_width(g)).sum::<usize>();
        let left_padding = if center {
            width.saturating_sub(content_width) / 2
        } else {
            0
        };
        let right_padding = if center {
            width.saturating_sub(content_width + left_padding)
        } else {
            0
        };
        let mut graphemes = Vec::with_capacity(row.len() + left_padding + right_padding);
        graphemes.extend(std::iter::repeat_n(" ".to_owned(), left_padding));
        graphemes.extend(row);
        graphemes.extend(std::iter::repeat_n(" ".to_owned(), right_padding));
        let row_width = content_width + left_padding + right_padding;
        total_cells += row_width;
        layout_rows.push(LayoutRow {
            graphemes,
            cell_width: row_width,
            left_padding,
        });
    }

    TextLayout {
        rows: layout_rows,
        width,
        total_cells,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_csi_osc_bel_and_preserves_safe_lines() {
        let text =
            "ok\x1b[31m red\x1b[0m\nurl\x1b]8;;https://example.test\x07click\x1b]8;;\x07\x07";
        assert_eq!(sanitize_text(text), "ok red\nurlclick");
    }

    #[test]
    fn strips_st_terminated_osc_and_unknown_escape() {
        assert_eq!(sanitize_text("a\x1b]title\x1b\\b\x1b7c"), "abc");
        assert_eq!(sanitize_text("a\u{009d}payload\u{009c}b"), "ab");
    }

    #[test]
    fn graphemes_combining_emoji_and_cjk_use_cell_width() {
        let layout = layout_text("e\u{301} 🦀 東京", 8, false);
        assert_eq!(layout.rows[0].text(), "e\u{301} 🦀 東");
        assert_eq!(layout.rows[1].text(), "京");
        assert_eq!(layout.rows[0].cell_width, 7);
        assert_eq!(layout.rows[1].cell_width, 2);
    }

    #[test]
    fn long_words_wrap_without_splitting_clusters() {
        let layout = layout_text("abcdef", 3, false);
        assert_eq!(layout.plain_lines(), vec!["abc", "def"]);
        let tiny = layout_text("界", 1, false);
        assert_eq!(tiny.plain_lines(), vec!["�"]);
        assert!(tiny.rows.iter().all(|row| row.cell_width <= 1));
    }

    #[test]
    fn centering_is_cell_based_and_trailing_newlines_are_trimmed() {
        let layout = layout_text("hi\n\n", 6, true);
        assert_eq!(layout.rows.len(), 1);
        assert_eq!(layout.rows[0].text(), "  hi  ");
        assert_eq!(layout.rows[0].left_padding, 2);
        assert_eq!(layout.rows[0].cell_width, 6);
    }
}
