use crate::{app::AppError, cli::AddArgs};
use std::{ffi::OsString, fs::File, io::Read, path::Path};

pub const MAX_INPUT_BYTES: usize = 1024 * 1024;

pub fn from_words(words: &[OsString]) -> Result<String, AppError> {
    let words = words
        .iter()
        .map(|word| {
            word.to_str().ok_or_else(|| {
                invalid("Entry arguments contain invalid Unicode; use a UTF-8 file.")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    normalize(&words.join(" "))
}

pub fn read_add(
    args: &AddArgs,
    stdin: &mut impl Read,
    stdin_is_terminal: bool,
) -> Result<String, AppError> {
    let source_count = usize::from(args.file.is_some())
        + usize::from(args.stdin)
        + usize::from(!args.text.is_empty());
    if source_count > 1 {
        return Err(invalid(
            "Provide only one content source: words, --file, or --stdin.",
        ));
    }
    if let Some(path) = &args.file {
        if path == Path::new("-") {
            return read_utf8(stdin);
        }
        let mut file = File::open(path)
            .map_err(|e| invalid(format!("Unable to read {}: {e}", path.display())))?;
        return read_utf8(&mut file);
    }
    if !args.text.is_empty() {
        return normalize(&args.text.join(" "));
    }
    if stdin_is_terminal && !args.stdin {
        return Err(invalid(
            "Entry text is required. Use cap write for an interactive session.",
        ));
    }
    read_utf8(stdin)
}

pub fn read_utf8(input: &mut impl Read) -> Result<String, AppError> {
    let mut bytes = Vec::new();
    input
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| invalid(format!("Unable to read entry input: {e}")))?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(invalid("Entry input exceeds the 1 MiB UTF-8 limit."));
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    let text = std::str::from_utf8(bytes).map_err(|_| {
        invalid("Entry input is not valid UTF-8. Save the file as UTF-8 and retry.")
    })?;
    normalize(text)
}

pub fn normalize(text: &str) -> Result<String, AppError> {
    if text.len() > MAX_INPUT_BYTES {
        return Err(invalid("Entry input exceeds the 1 MiB UTF-8 limit."));
    }
    if text.trim().is_empty() {
        return Err(invalid("Entry text is required."));
    }
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::new("INVALID_INPUT", message, 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn file_content_keeps_whitespace_and_only_normalizes_newlines() {
        let mut bytes = Cursor::new(b"\xef\xbb\xbf  first\r\n\rnext  \r\n".to_vec());
        assert_eq!(read_utf8(&mut bytes).unwrap(), "  first\n\nnext  \n");
    }
    #[test]
    fn invalid_encoding_and_blank_input_fail_without_replacement() {
        assert!(read_utf8(&mut Cursor::new(vec![0xff])).is_err());
        assert!(read_utf8(&mut Cursor::new(b" \n\t\r")).is_err());
    }
    #[test]
    fn oversized_input_is_bounded_before_decoding() {
        let mut input = Cursor::new(vec![b'x'; MAX_INPUT_BYTES + 10]);
        assert!(read_utf8(&mut input).is_err());
        assert_eq!(input.position(), (MAX_INPUT_BYTES + 1) as u64);
    }
    #[test]
    fn positional_words_preserve_literal_hyphens_and_shell_quoted_spaces() {
        assert_eq!(
            from_words(&["write-the-entry-here".into(), "two  spaces".into()]).unwrap(),
            "write-the-entry-here two  spaces"
        );
    }
    #[test]
    fn conflicting_sources_fail_before_a_file_is_opened() {
        let args = AddArgs {
            file: Some("missing-file.md".into()),
            text: vec!["words".into()],
            ..AddArgs::default()
        };
        let error = read_add(&args, &mut Cursor::new(b""), true).unwrap_err();
        assert!(error.detail.message.contains("one content source"));
    }
}
