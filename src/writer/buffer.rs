//! Unicode/grapheme-safe editing primitives for the interactive writer.
//!
//! The buffer intentionally stores one UTF-8 `String` and a byte cursor.  The
//! cursor is always snapped to a grapheme boundary, so editing operations can
//! never split a combining sequence, emoji ZWJ sequence, or other user-visible
//! cluster.  Rendering code may project the same text into terminal cells, but
//! it never has to repair an invalid cursor.

use std::{fmt, ops::Range};

use unicode_segmentation::UnicodeSegmentation;

pub const MAX_BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BufferError {
    TooLarge { limit: usize },
    InvalidCursor,
}

impl fmt::Display for BufferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { limit } => write!(
                formatter,
                "writer text exceeds the {limit} byte UTF-8 limit"
            ),
            Self::InvalidCursor => formatter.write_str("writer cursor is not on a UTF-8 boundary"),
        }
    }
}

impl std::error::Error for BufferError {}

/// A line-oriented cursor over Unicode grapheme clusters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBuffer {
    text: String,
    cursor: usize,
    /// A terminal-cell column retained while moving repeatedly up/down.  It
    /// is reset by horizontal edits and direct cursor placement.
    preferred_column: Option<usize>,
}

impl TextBuffer {
    pub fn new(text: impl Into<String>) -> Self {
        let mut text = text.into();
        // Draft files and bracketed paste may use CRLF/old-Mac newlines. Keep
        // the same canonical newline contract as cap add.
        text = normalize_newlines(&text);
        if text.len() > MAX_BUFFER_BYTES {
            // A persisted draft can only be created through `insert_text`, but
            // keeping this constructor infallible is useful for tests and for
            // a defensive editor read. Truncate at a UTF-8 boundary and let
            // the caller surface the bounded-input warning on the next edit.
            text.truncate(previous_char_boundary(&text, MAX_BUFFER_BYTES));
        }
        let cursor = text.len();
        Self {
            text,
            cursor,
            preferred_column: None,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn into_string(self) -> String {
        self.text
    }

    pub fn cursor_byte(&self) -> usize {
        self.cursor
    }

    pub fn len_bytes(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }

    pub fn set_cursor_byte(&mut self, requested: usize) -> Result<(), BufferError> {
        if requested > self.text.len() || !self.text.is_char_boundary(requested) {
            return Err(BufferError::InvalidCursor);
        }
        self.cursor = snap_grapheme_boundary(&self.text, requested);
        self.preferred_column = None;
        Ok(())
    }

    pub fn insert_char(&mut self, character: char) -> Result<(), BufferError> {
        if character == '\r' {
            return self.insert_text("\n");
        }
        self.insert_text(&character.to_string())
    }

    /// Insert a pasted segment as text. No key-like characters are interpreted
    /// here; bracketed paste is mapped to this operation by the terminal loop.
    pub fn insert_text(&mut self, text: &str) -> Result<(), BufferError> {
        let normalized = normalize_newlines(text);
        let next_len = self.text.len().saturating_add(normalized.len());
        if next_len > MAX_BUFFER_BYTES {
            return Err(BufferError::TooLarge {
                limit: MAX_BUFFER_BYTES,
            });
        }
        self.text.insert_str(self.cursor, &normalized);
        self.cursor += normalized.len();
        self.preferred_column = None;
        Ok(())
    }

    pub fn backspace(&mut self) -> bool {
        let Some(start) = previous_grapheme_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
        self.preferred_column = None;
        true
    }

    pub fn delete(&mut self) -> bool {
        let Some(end) = next_grapheme_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.text.replace_range(self.cursor..end, "");
        self.preferred_column = None;
        true
    }

    pub fn move_left(&mut self) -> bool {
        let Some(previous) = previous_grapheme_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.cursor = previous;
        self.preferred_column = None;
        true
    }

    pub fn move_right(&mut self) -> bool {
        let Some(next) = next_grapheme_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.cursor = next;
        self.preferred_column = None;
        true
    }

    pub fn move_home(&mut self) -> bool {
        let (line, _) = self.cursor_position();
        let range = self.line_range(line);
        let changed = self.cursor != range.start;
        self.cursor = range.start;
        self.preferred_column = None;
        changed
    }

    pub fn move_end(&mut self) -> bool {
        let (line, _) = self.cursor_position();
        let range = self.line_range(line);
        let changed = self.cursor != range.end;
        self.cursor = range.end;
        self.preferred_column = None;
        changed
    }

    pub fn move_document_start(&mut self) -> bool {
        let changed = self.cursor != 0;
        self.cursor = 0;
        self.preferred_column = None;
        changed
    }

    pub fn move_document_end(&mut self) -> bool {
        let changed = self.cursor != self.text.len();
        self.cursor = self.text.len();
        self.preferred_column = None;
        changed
    }

    /// Move vertically while retaining a terminal-cell column as long as the
    /// user keeps pressing up/down. Combining clusters have zero width and are
    /// kept with the grapheme that owns them.
    pub fn move_vertical(&mut self, delta: isize) -> bool {
        if delta == 0 {
            return false;
        }
        let (line, _) = self.cursor_position();
        let lines = self.line_ranges();
        let target = if delta.is_negative() {
            line.saturating_sub(delta.unsigned_abs())
        } else {
            line.saturating_add(delta as usize)
        }
        .min(lines.len().saturating_sub(1));
        if target == line {
            return false;
        }

        let desired = self.preferred_column.unwrap_or_else(|| self.cell_column());
        self.preferred_column = Some(desired);
        let range = lines[target].clone();
        self.cursor = byte_at_cell_column(&self.text, range, desired);
        true
    }

    /// Return zero-based `(line, grapheme-column)` coordinates.
    pub fn cursor_position(&self) -> (usize, usize) {
        let line = self.text.get(..self.cursor).map_or(0, |prefix| {
            prefix.bytes().filter(|byte| *byte == b'\n').count()
        });
        let line_start = self.line_range(line).start;
        let grapheme_column = self.text[line_start..self.cursor].graphemes(true).count();
        (line, grapheme_column)
    }

    /// Return the terminal-cell column for the current line.
    pub fn cell_column(&self) -> usize {
        let (line, _) = self.cursor_position();
        let range = self.line_range(line);
        self.text[range.start..self.cursor]
            .graphemes(true)
            .map(cap_effects::grapheme_width)
            .sum()
    }

    pub fn line_count(&self) -> usize {
        self.line_ranges().len()
    }

    pub fn line(&self, index: usize) -> Option<&str> {
        let range = self.line_ranges().get(index)?.clone();
        Some(&self.text[range])
    }

    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.text.split('\n')
    }

    fn line_ranges(&self) -> Vec<Range<usize>> {
        let mut ranges = Vec::new();
        let mut start = 0;
        for (offset, character) in self.text.char_indices() {
            if character == '\n' {
                ranges.push(start..offset);
                start = offset + character.len_utf8();
            }
        }
        ranges.push(start..self.text.len());
        ranges
    }

    fn line_range(&self, line: usize) -> Range<usize> {
        self.line_ranges()
            .get(line)
            .cloned()
            .unwrap_or(self.text.len()..self.text.len())
    }
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn previous_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn snap_grapheme_boundary(text: &str, requested: usize) -> usize {
    if requested >= text.len() {
        return text.len();
    }
    let mut previous = 0;
    for (offset, _) in text.grapheme_indices(true) {
        if offset == requested {
            return requested;
        }
        if offset > requested {
            break;
        }
        previous = offset;
    }
    previous
}

fn previous_grapheme_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor == 0 {
        return None;
    }
    text[..cursor]
        .grapheme_indices(true)
        .next_back()
        .map(|(offset, _)| offset)
}

fn next_grapheme_boundary(text: &str, cursor: usize) -> Option<usize> {
    text[cursor..]
        .grapheme_indices(true)
        .nth(1)
        .map(|(offset, _)| cursor + offset)
        .or_else(|| (cursor < text.len()).then_some(text.len()))
}

fn byte_at_cell_column(text: &str, range: Range<usize>, desired: usize) -> usize {
    let mut offset = range.start;
    let mut cells = 0;
    for (relative, grapheme) in text[range.clone()].grapheme_indices(true) {
        let width = cap_effects::grapheme_width(grapheme);
        if cells >= desired || (width > 0 && cells.saturating_add(width) > desired) {
            break;
        }
        cells = cells.saturating_add(width);
        offset = range.start + relative + grapheme.len();
    }
    offset.min(range.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backspace_and_delete_remove_whole_graphemes() {
        let family = "👨‍👩‍👧‍👦";
        let mut buffer = TextBuffer::new(format!("a{family}b"));
        assert!(buffer.move_left());
        assert!(buffer.backspace());
        assert_eq!(buffer.as_str(), "ab");
        let mut delete = TextBuffer::new(format!("{family}b"));
        assert!(delete.move_home());
        assert!(delete.delete());
        assert_eq!(delete.as_str(), "b");
    }

    #[test]
    fn combining_and_crlf_paste_are_normalized_without_key_interpretation() {
        let mut buffer = TextBuffer::new("" as &str);
        buffer.insert_text("e\u{301}\r\nCtrl+S").unwrap();
        assert_eq!(buffer.as_str(), "e\u{301}\nCtrl+S");
        assert_eq!(buffer.word_count(), 2);
    }

    #[test]
    fn direct_cursor_placement_preserves_exact_boundaries() {
        let mut buffer = TextBuffer::new("ab");
        buffer.set_cursor_byte(1).unwrap();
        assert_eq!(buffer.cursor_byte(), 1);
        buffer.set_cursor_byte(2).unwrap();
        assert_eq!(buffer.cursor_byte(), 2);
    }

    #[test]
    fn oversized_insert_is_rejected_without_mutating_text() {
        let mut buffer = TextBuffer::new("ok");
        let before = buffer.as_str().to_owned();
        let oversized = "x".repeat(MAX_BUFFER_BYTES);
        assert!(matches!(
            buffer.insert_text(&oversized),
            Err(BufferError::TooLarge { .. })
        ));
        assert_eq!(buffer.as_str(), before);
    }

    #[test]
    fn vertical_motion_keeps_line_and_cell_safety() {
        let mut buffer = TextBuffer::new("one\n界界\nthree");
        buffer.move_document_end();
        assert!(buffer.move_vertical(-2));
        assert_eq!(buffer.cursor_position().0, 0);
        assert!(buffer.cell_column() <= 5);
    }
}
