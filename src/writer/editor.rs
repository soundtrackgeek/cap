//! Safe external-editor hand-off for `cap write --editor`.
//!
//! The configured executable and argument array are passed directly to
//! [`std::process::Command`]. No shell, profile, or command-string parsing is
//! involved. A unique UTF-8 temporary file is removed only after the caller
//! has durably persisted successful output; retained failure files carry their
//! path in the returned error.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{preferences::EffectivePresentation, writer::buffer::MAX_BUFFER_BYTES};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorError {
    Unavailable(String),
    Launch(String),
    Failed {
        status: String,
        recovered: Option<String>,
        path: Option<PathBuf>,
        output_error: Option<String>,
    },
    Io {
        message: String,
        path: Option<PathBuf>,
    },
    InvalidOutput {
        message: String,
        path: Option<PathBuf>,
    },
}

impl std::fmt::Display for EditorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) | Self::Launch(message) => formatter.write_str(message),
            Self::Io { message, path } => {
                formatter.write_str(message)?;
                if let Some(path) = path {
                    write!(formatter, "; editor file retained at {}", path.display())?;
                }
                Ok(())
            }
            Self::InvalidOutput { message, path } => {
                if let Some(path) = path {
                    write!(
                        formatter,
                        "{message}; editor file retained at {}",
                        path.display()
                    )
                } else {
                    formatter.write_str(message)
                }
            }
            Self::Failed {
                status,
                path,
                output_error,
                ..
            } => {
                write!(
                    formatter,
                    "configured editor exited unsuccessfully ({status})"
                )?;
                if let Some(output_error) = output_error {
                    write!(formatter, "; {output_error}")?;
                }
                if let Some(path) = path {
                    write!(formatter, "; editor file retained at {}", path.display())?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for EditorError {}

/// Successful editor output remains associated with its temporary file until
/// the caller confirms that the editable draft was durably written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditedDocument {
    text: String,
    path: PathBuf,
}

impl EditedDocument {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn remove_temp(self) {
        let _ = fs::remove_file(self.path);
    }
}

/// Run the durable draft write while retaining ownership of the editor file.
/// The file is removed only after `persist` succeeds; on failure the document
/// is returned to the caller so its path can be surfaced and retried.
pub fn persist_document<T, E, F>(
    document: EditedDocument,
    persist: F,
) -> Result<T, (E, EditedDocument)>
where
    F: FnOnce(&str) -> Result<T, E>,
{
    match persist(document.text()) {
        Ok(value) => {
            document.remove_temp();
            Ok(value)
        }
        Err(error) => Err((error, document)),
    }
}

/// Launch the configured editor, returning only a validated nonempty UTF-8
/// document after a successful exit. The temporary file is owned by the
/// returned document until [`EditedDocument::remove_temp`] is called.
pub fn edit(
    presentation: &EffectivePresentation,
    initial: &str,
) -> Result<EditedDocument, EditorError> {
    if initial.len() > MAX_BUFFER_BYTES {
        return Err(EditorError::InvalidOutput {
            message: format!("writer text exceeds the {MAX_BUFFER_BYTES} byte UTF-8 limit"),
            path: None,
        });
    }
    let path = unique_temp_path();
    let mut retain_temp = false;
    let result = (|| {
        let (executable, args) = configured_command(presentation, &path)?;
        if let Err(error) = write_temp(&path, initial) {
            retain_temp = error.retained_path().is_some();
            return Err(error);
        }
        let status = Command::new(&executable)
            .args(&args)
            .status()
            .map_err(|error| {
                EditorError::Launch(format!(
                    "unable to launch editor {}: {error}",
                    executable.display()
                ))
            })?;
        if !status.success() {
            let status = status.code().map_or_else(
                || "terminated by signal".to_string(),
                |code| code.to_string(),
            );
            return match read_editor_output(&path) {
                Ok(text) => {
                    // A useful body written before a failing editor exit is
                    // still recoverable. Keep the owned temp until the command
                    // persists `recovered_text`; the caller can then remove it.
                    retain_temp = true;
                    Err(EditorError::Failed {
                        status,
                        recovered: Some(text),
                        path: Some(path.clone()),
                        output_error: None,
                    })
                }
                Err(mut error) => {
                    retain_temp = true;
                    let output_error = match &error {
                        EditorError::InvalidOutput { message, .. } => Some(message.clone()),
                        other => Some(other.to_string()),
                    };
                    attach_path(&mut error, &path);
                    Err(EditorError::Failed {
                        status,
                        recovered: None,
                        path: Some(path.clone()),
                        output_error,
                    })
                }
            };
        }
        match read_editor_output(&path) {
            Ok(text) => {
                // Ownership of a successful output transfers to the caller;
                // it must survive until the recovery record is durable.
                retain_temp = true;
                Ok(EditedDocument {
                    text,
                    path: path.clone(),
                })
            }
            Err(mut error) => {
                retain_temp = true;
                attach_path(&mut error, &path);
                Err(error)
            }
        }
    })();
    if !retain_temp {
        let _ = fs::remove_file(&path);
    }
    result
}

impl EditorError {
    pub fn recovered_text(&self) -> Option<&str> {
        match self {
            Self::Failed { recovered, .. } => recovered.as_deref(),
            _ => None,
        }
    }

    pub fn retained_path(&self) -> Option<&Path> {
        match self {
            Self::Failed { path, .. }
            | Self::InvalidOutput { path, .. }
            | Self::Io { path, .. } => path.as_deref(),
            _ => None,
        }
    }
}

pub fn remove_retained_file(error: &mut EditorError) -> bool {
    let Some(path) = error.retained_path().map(Path::to_owned) else {
        return true;
    };
    match fs::remove_file(&path) {
        Ok(()) => {
            error.clear_retained_path();
            true
        }
        Err(io_error) if io_error.kind() == std::io::ErrorKind::NotFound => {
            error.clear_retained_path();
            true
        }
        Err(_) => false,
    }
}

impl EditorError {
    fn clear_retained_path(&mut self) {
        match self {
            Self::Failed { path, .. }
            | Self::InvalidOutput { path, .. }
            | Self::Io { path, .. } => *path = None,
            Self::Unavailable(_) | Self::Launch(_) => {}
        }
    }
}

fn configured_command(
    presentation: &EffectivePresentation,
    file: &Path,
) -> Result<(PathBuf, Vec<String>), EditorError> {
    let Some(executable) = presentation.editor_executable.as_deref() else {
        return Err(EditorError::Unavailable(
            "No editor executable is configured; set editor.executable first.".to_string(),
        ));
    };
    let file = file.to_string_lossy();
    let mut args = Vec::with_capacity(presentation.editor_args.len() + 1);
    let mut used_file = false;
    for argument in &presentation.editor_args {
        if argument.contains("{file}") {
            used_file = true;
            args.push(argument.replace("{file}", &file));
        } else {
            args.push(argument.clone());
        }
    }
    if !used_file {
        args.push(file.into_owned());
    }
    Ok((PathBuf::from(executable), args))
}

fn unique_temp_path() -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "cap-writer-{}-{:x}-{counter}.md",
        std::process::id(),
        unix_nanos()
    ))
}

fn write_temp(path: &Path, text: &str) -> Result<(), EditorError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| EditorError::Io {
            message: format!("unable to create editor draft: {error}"),
            path: None,
        })?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all())
        .map_err(|error| EditorError::Io {
            message: format!("unable to write editor draft: {error}"),
            path: Some(path.to_owned()),
        })
}

fn read_editor_output(path: &Path) -> Result<String, EditorError> {
    let mut bytes = Vec::new();
    let file = File::open(path).map_err(|error| EditorError::Io {
        message: format!("unable to read editor output: {error}"),
        path: Some(path.to_owned()),
    })?;
    file.take((MAX_BUFFER_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| EditorError::Io {
            message: format!("unable to read editor output: {error}"),
            path: Some(path.to_owned()),
        })?;
    if bytes.len() > MAX_BUFFER_BYTES {
        return Err(EditorError::InvalidOutput {
            message: format!("editor output exceeds the {MAX_BUFFER_BYTES} byte UTF-8 limit"),
            path: None,
        });
    }
    let text = std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
        .map_err(|_| EditorError::InvalidOutput {
            message: "editor output is not valid UTF-8".to_string(),
            path: None,
        })?
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    if text.trim().is_empty() {
        return Err(EditorError::InvalidOutput {
            message: "editor output is blank; the existing draft was kept".to_string(),
            path: None,
        });
    }
    Ok(text)
}

fn attach_path(error: &mut EditorError, path: &Path) {
    match error {
        EditorError::Failed { path: slot, .. }
        | EditorError::InvalidOutput { path: slot, .. }
        | EditorError::Io { path: slot, .. } => *slot = Some(path.to_owned()),
        EditorError::Unavailable(_) | EditorError::Launch(_) => {}
    }
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn presentation(executable: Option<String>, args: Vec<String>) -> EffectivePresentation {
        EffectivePresentation {
            theme: crate::ui::themes::Theme::Aurora,
            output: cap_effects::ResolvedOutputMode::Human {
                color: cap_effects::ColorMode::Plain,
                motion: cap_effects::MotionMode::Off,
            },
            icon_mode: crate::preferences::IconMode::Ascii,
            preview_visibility: crate::preferences::PreviewVisibility::Never,
            writer_display: crate::preferences::WriterDisplay::Compact,
            writer_target_override: None,
            editor_executable: executable,
            editor_args: args,
        }
    }

    #[test]
    fn editor_output_normalizes_bom_newlines_and_rejects_blank_or_invalid() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("editor.md");
        fs::write(&path, b"\xef\xbb\xbfone\r\ntwo\r").unwrap();
        assert_eq!(read_editor_output(&path).unwrap(), "one\ntwo\n");
        fs::write(&path, b" \r\n").unwrap();
        assert!(matches!(
            read_editor_output(&path),
            Err(EditorError::InvalidOutput { .. })
        ));
        fs::write(&path, [0xff]).unwrap();
        assert!(matches!(
            read_editor_output(&path),
            Err(EditorError::InvalidOutput { .. })
        ));
    }

    #[test]
    fn editor_command_uses_argument_array_and_file_placeholder() {
        let path = Path::new(r"C:\folder with spaces\draft.md");
        let p = presentation(
            Some(r"C:\Program Files\Editor\editor.exe".to_string()),
            vec!["--wait".to_string(), "--file={file}".to_string()],
        );
        let (executable, args) = configured_command(&p, path).unwrap();
        assert_eq!(
            executable,
            PathBuf::from(r"C:\Program Files\Editor\editor.exe")
        );
        assert_eq!(args, ["--wait", r"--file=C:\folder with spaces\draft.md"]);
    }

    #[test]
    fn missing_editor_is_reported_without_touching_user_preferences() {
        let p = presentation(None, Vec::new());
        let error = edit(&p, "draft").unwrap_err();
        assert!(matches!(error, EditorError::Unavailable(_)));
    }

    #[test]
    fn successful_document_keeps_temp_until_durable_cleanup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("draft.md");
        fs::write(&path, "draft").unwrap();
        let document = EditedDocument {
            text: "draft".to_owned(),
            path: path.clone(),
        };
        assert_eq!(document.text(), "draft");
        assert_eq!(document.path(), path.as_path());
        assert!(path.exists());
        document.remove_temp();
        assert!(!path.exists());
    }

    #[test]
    fn retained_failure_path_is_cleared_only_after_file_cleanup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("failed.md");
        fs::write(&path, "recovered").unwrap();
        let mut error = EditorError::Failed {
            status: "7".to_owned(),
            recovered: Some("recovered".to_owned()),
            path: Some(path.clone()),
            output_error: None,
        };
        assert_eq!(error.retained_path(), Some(path.as_path()));
        assert!(error.to_string().contains("editor file retained at"));
        assert!(remove_retained_file(&mut error));
        assert!(!path.exists());
        assert!(error.retained_path().is_none());
        assert!(!error.to_string().contains("retained at"));
    }

    #[test]
    fn io_output_errors_include_the_owned_temp_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("output.md");
        fs::write(&path, b"ok").unwrap();
        let mut error = EditorError::Io {
            message: "read failed".to_owned(),
            path: Some(path.clone()),
        };
        assert!(error.to_string().contains(&path.display().to_string()));
        assert!(remove_retained_file(&mut error));
        assert!(error.retained_path().is_none());
    }

    #[test]
    fn persistence_failure_returns_the_document_without_deleting_it() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("persist.md");
        fs::write(&path, "draft").unwrap();
        let document = EditedDocument {
            text: "draft".to_owned(),
            path: path.clone(),
        };
        let (_, retained) = persist_document(document, |_| Err::<(), _>("database unavailable"))
            .expect_err("failed persistence must retain the editor file");
        assert_eq!(retained.path(), path.as_path());
        assert!(path.exists());
        retained.remove_temp();
    }

    #[test]
    fn persistence_success_deletes_the_document_after_the_write() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("persist-ok.md");
        fs::write(&path, "draft").unwrap();
        let document = EditedDocument {
            text: "draft".to_owned(),
            path: path.clone(),
        };
        persist_document(document, |text| {
            assert_eq!(text, "draft");
            Ok::<_, ()>(())
        })
        .unwrap();
        assert!(!path.exists());
    }

    #[test]
    #[cfg(windows)]
    fn real_editor_output_survives_until_persistence_succeeds() {
        let p = presentation(
            Some("cmd.exe".to_owned()),
            vec![
                "/D".to_owned(),
                "/C".to_owned(),
                "echo edited> {file}".to_owned(),
            ],
        );
        let document = edit(&p, "initial").expect("cmd.exe should write the editor file");
        assert_eq!(document.text(), "edited\n");
        assert!(document.path().exists());
        let (_, retained) = persist_document(document, |_| Err::<(), _>("database unavailable"))
            .expect_err("failed persistence must retain real editor output");
        assert_eq!(fs::read_to_string(retained.path()).unwrap(), "edited\r\n");
        retained.remove_temp();
    }
}
