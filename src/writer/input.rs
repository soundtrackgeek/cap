//! Deterministic writer input state machine.
//!
//! Terminal adapters translate crossterm events into these small semantic
//! events. Keeping editing decisions here makes paste, Unicode and control
//! key behavior testable without a live console.

use super::buffer::{BufferError, TextBuffer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    DocumentStart,
    DocumentEnd,
    CtrlS,
    CtrlC,
    Escape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Key(Key),
    Paste(String),
    Focus(bool),
    Resize { width: u16, height: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriterAction {
    Continue,
    Save,
    Exit,
    /// Raw Ctrl+C requests cancellation while retaining the current draft.
    Interrupt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterMachine {
    buffer: TextBuffer,
    dirty: bool,
    focused: bool,
    width: u16,
    height: u16,
}

impl WriterMachine {
    pub fn new(text: impl Into<String>, width: u16, height: u16) -> Self {
        Self {
            buffer: TextBuffer::new(text),
            dirty: false,
            focused: true,
            width: width.max(1),
            height: height.max(1),
        }
    }

    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }

    pub fn buffer_mut(&mut self) -> &mut TextBuffer {
        &mut self.buffer
    }

    pub fn text(&self) -> &str {
        self.buffer.as_str()
    }

    pub fn word_count(&self) -> usize {
        self.buffer.word_count()
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_persisted(&mut self) {
        self.dirty = false;
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn apply(&mut self, event: InputEvent) -> Result<WriterAction, BufferError> {
        match event {
            InputEvent::Paste(text) => {
                if !text.is_empty() {
                    self.buffer.insert_text(&text)?;
                    self.dirty = true;
                }
                Ok(WriterAction::Continue)
            }
            InputEvent::Focus(focused) => {
                self.focused = focused;
                Ok(WriterAction::Continue)
            }
            InputEvent::Resize { width, height } => {
                self.width = width.max(1);
                self.height = height.max(1);
                Ok(WriterAction::Continue)
            }
            InputEvent::Key(key) => self.apply_key(key),
        }
    }

    fn apply_key(&mut self, key: Key) -> Result<WriterAction, BufferError> {
        match key {
            Key::CtrlS => Ok(WriterAction::Save),
            Key::CtrlC => Ok(WriterAction::Interrupt),
            Key::Escape => Ok(WriterAction::Exit),
            Key::Char(character) if !character.is_control() => {
                self.buffer.insert_char(character)?;
                self.dirty = true;
                Ok(WriterAction::Continue)
            }
            Key::Enter => {
                self.buffer.insert_char('\n')?;
                self.dirty = true;
                Ok(WriterAction::Continue)
            }
            Key::Backspace => {
                self.dirty |= self.buffer.backspace();
                Ok(WriterAction::Continue)
            }
            Key::Delete => {
                self.dirty |= self.buffer.delete();
                Ok(WriterAction::Continue)
            }
            Key::Left => {
                self.buffer.move_left();
                Ok(WriterAction::Continue)
            }
            Key::Right => {
                self.buffer.move_right();
                Ok(WriterAction::Continue)
            }
            Key::Up => {
                self.buffer.move_vertical(-1);
                Ok(WriterAction::Continue)
            }
            Key::Down => {
                self.buffer.move_vertical(1);
                Ok(WriterAction::Continue)
            }
            Key::Home => {
                self.buffer.move_home();
                Ok(WriterAction::Continue)
            }
            Key::End => {
                self.buffer.move_end();
                Ok(WriterAction::Continue)
            }
            Key::DocumentStart => {
                self.buffer.move_document_start();
                Ok(WriterAction::Continue)
            }
            Key::DocumentEnd => {
                self.buffer.move_document_end();
                Ok(WriterAction::Continue)
            }
            // Other control characters never become authored text. Pasted
            // control bytes are intentionally handled by `Paste` above as one
            // literal segment rather than interpreted as key commands.
            Key::Char(_) => Ok(WriterAction::Continue),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_ctrl_s_ctrl_c_and_paste_have_distinct_actions() {
        let mut machine = WriterMachine::new("hello", 80, 24);
        assert_eq!(
            machine.apply(InputEvent::Key(Key::Enter)).unwrap(),
            WriterAction::Continue
        );
        machine
            .apply(InputEvent::Paste("world\nCtrl+S".to_owned()))
            .unwrap();
        assert_eq!(machine.text(), "hello\nworld\nCtrl+S");
        assert_eq!(
            machine.apply(InputEvent::Key(Key::CtrlS)).unwrap(),
            WriterAction::Save
        );
        assert_eq!(
            machine.apply(InputEvent::Key(Key::CtrlC)).unwrap(),
            WriterAction::Interrupt
        );
        assert_eq!(
            machine.apply(InputEvent::Key(Key::Escape)).unwrap(),
            WriterAction::Exit
        );
    }

    #[test]
    fn focus_and_resize_are_state_only_and_navigation_is_grapheme_safe() {
        let mut machine = WriterMachine::new("e\u{301}x", 20, 4);
        machine.apply(InputEvent::Key(Key::Home)).unwrap();
        machine.apply(InputEvent::Key(Key::Right)).unwrap();
        assert_eq!(machine.buffer().cursor_position(), (0, 1));
        machine
            .apply(InputEvent::Focus(false))
            .expect("focus event");
        machine
            .apply(InputEvent::Resize {
                width: 0,
                height: 0,
            })
            .expect("resize event");
        assert!(!machine.focused());
        assert_eq!((machine.width(), machine.height()), (1, 1));
    }
}
