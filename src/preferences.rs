//! Cap-local presentation and editor preferences.
//!
//! This module intentionally has no knowledge of Capsule's settings files.
//! `ConfigStore` resolves only `%LOCALAPPDATA%\\Capsule\\cap` (or the
//! testable `CAP_CONFIG_HOME` override) and writes a small, deny-listed JSON
//! document using an atomic temporary-file replacement.

use crate::cli::{ColorMode as CliColorMode, GlobalOptions, MotionMode as CliMotionMode};
use crate::ui::themes::Theme;
use cap_effects::{
    resolve_output_mode, ColorChoice, MotionChoice, OutputRequest, ResolvedOutputMode,
    TerminalCapabilities,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const CONFIG_FILE_NAME: &str = "preferences.json";
pub const CONFIG_HOME_ENV: &str = "CAP_CONFIG_HOME";

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Errors are kept separate from the CLI envelope so this module remains
/// reusable by commands, tests and the future writer without printing or
/// exiting on its own.
#[derive(Debug)]
pub enum PreferenceError {
    Io(io::Error),
    Json(serde_json::Error),
    InvalidKey(String),
    InvalidValue(String),
    InvalidConfig(String),
}

impl fmt::Display for PreferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "preference storage failed: {error}"),
            Self::Json(error) => write!(f, "preferences are not valid JSON: {error}"),
            Self::InvalidKey(key) => write!(f, "unsupported cap preference key `{key}`"),
            Self::InvalidValue(message) | Self::InvalidConfig(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PreferenceError {}

impl From<io::Error> for PreferenceError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for PreferenceError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// How icons should be emitted when a terminal supports both Unicode and
/// ASCII.  `Auto` lets the effective presentation select ASCII for uncertain
/// output encodings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IconMode {
    #[default]
    Auto,
    Unicode,
    Ascii,
}

impl IconMode {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Unicode => "unicode",
            Self::Ascii => "ascii",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "unicode" | "utf8" | "utf-8" => Some(Self::Unicode),
            "ascii" => Some(Self::Ascii),
            _ => None,
        }
    }
}

/// Whether a command may show decorative previews in human output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PreviewVisibility {
    #[default]
    Auto,
    Always,
    Never,
}

impl PreviewVisibility {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "always" | "show" | "on" => Some(Self::Always),
            "never" | "hide" | "off" => Some(Self::Never),
            _ => None,
        }
    }
}

/// The amount of chrome shown by the future interactive writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WriterDisplay {
    #[default]
    Compact,
    Full,
}

impl WriterDisplay {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Full => "full",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "compact" | "minimal" => Some(Self::Compact),
            "full" | "expanded" => Some(Self::Full),
            _ => None,
        }
    }
}

/// The complete cap-only document.  Unknown fields are rejected so a Capsule
/// `config.json` cannot accidentally be treated as cap preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Preferences {
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub color: ColorChoice,
    #[serde(default)]
    pub motion: MotionChoice,
    #[serde(default)]
    pub icon_mode: IconMode,
    #[serde(default)]
    pub preview_visibility: PreviewVisibility,
    #[serde(default)]
    pub writer_display: WriterDisplay,
    #[serde(default)]
    pub writer_target_override: Option<u32>,
    #[serde(default)]
    pub editor_executable: Option<String>,
    #[serde(default)]
    pub editor_args: Vec<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: Theme::Aurora,
            color: ColorChoice::Auto,
            motion: MotionChoice::Auto,
            icon_mode: IconMode::Auto,
            preview_visibility: PreviewVisibility::Auto,
            writer_display: WriterDisplay::Compact,
            writer_target_override: None,
            editor_executable: None,
            editor_args: Vec::new(),
        }
    }
}

impl Preferences {
    pub fn validate(&self) -> Result<(), PreferenceError> {
        if let Some(executable) = self.editor_executable.as_deref() {
            validate_editor_executable(executable)?;
        }
        if self.editor_args.len() > 64 {
            return Err(PreferenceError::InvalidValue(
                "editor.args accepts at most 64 arguments.".to_owned(),
            ));
        }
        if self
            .editor_args
            .iter()
            .any(|argument| argument.contains('\0'))
        {
            return Err(PreferenceError::InvalidValue(
                "editor.args cannot contain NUL characters.".to_owned(),
            ));
        }
        if let Some(target) = self.writer_target_override {
            if !(1..=1_000_000).contains(&target) {
                return Err(PreferenceError::InvalidValue(
                    "writer.target must be between 1 and 1,000,000 words.".to_owned(),
                ));
            }
        }
        Ok(())
    }

    /// Apply one allowlisted `cap config set` key without shell evaluation.
    pub fn set_key(&mut self, key: &str, value: &str) -> Result<(), PreferenceError> {
        let canonical = canonical_key(key);
        match canonical.as_str() {
            "theme" => {
                self.theme = Theme::parse(value).ok_or_else(|| {
                    PreferenceError::InvalidValue(format!(
                        "theme must be one of: {}.",
                        Theme::all()
                            .iter()
                            .map(|theme| theme.name())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                })?;
            }
            "color" => {
                self.color = parse_color(value).ok_or_else(|| {
                    PreferenceError::InvalidValue(
                        "color must be auto, always, never, truecolor, ansi256 or ansi16."
                            .to_owned(),
                    )
                })?;
            }
            "motion" => {
                self.motion = parse_motion(value).ok_or_else(|| {
                    PreferenceError::InvalidValue(
                        "motion must be auto, full, reduced or off.".to_owned(),
                    )
                })?;
            }
            "icon_mode" => {
                self.icon_mode = IconMode::parse(value).ok_or_else(|| {
                    PreferenceError::InvalidValue(
                        "icon_mode must be auto, unicode or ascii.".to_owned(),
                    )
                })?;
            }
            "preview_visibility" => {
                self.preview_visibility = PreviewVisibility::parse(value).ok_or_else(|| {
                    PreferenceError::InvalidValue(
                        "preview_visibility must be auto, always or never.".to_owned(),
                    )
                })?;
            }
            "writer_display" => {
                self.writer_display = WriterDisplay::parse(value).ok_or_else(|| {
                    PreferenceError::InvalidValue(
                        "writer_display must be compact or full.".to_owned(),
                    )
                })?;
            }
            "writer_target_override" | "writer_target" => {
                let trimmed = value.trim();
                self.writer_target_override = if trimmed.eq_ignore_ascii_case("none")
                    || trimmed.eq_ignore_ascii_case("off")
                {
                    None
                } else {
                    Some(trimmed.parse::<u32>().map_err(|_| {
                        PreferenceError::InvalidValue(
                            "writer_target_override must be an integer or none.".to_owned(),
                        )
                    })?)
                };
            }
            "editor_executable" => {
                let executable = value.trim();
                validate_editor_executable(executable)?;
                self.editor_executable = Some(executable.to_owned());
            }
            "editor_args" => {
                let parsed: serde_json::Value = serde_json::from_str(value).map_err(|_| {
                    PreferenceError::InvalidValue(
                        "editor_args must be a JSON array of strings, for example [\"--wait\", \"{file}\"]."
                            .to_owned(),
                    )
                })?;
                let serde_json::Value::Array(values) = parsed else {
                    return Err(PreferenceError::InvalidValue(
                        "editor_args must be a JSON array of strings, for example [\"--wait\", \"{file}\"]."
                            .to_owned(),
                    ));
                };
                let mut args = Vec::with_capacity(values.len());
                for value in values {
                    let Some(argument) = value.as_str() else {
                        return Err(PreferenceError::InvalidValue(
                            "editor_args must contain only strings.".to_owned(),
                        ));
                    };
                    args.push(argument.to_owned());
                }
                self.editor_args = args;
            }
            _ => return Err(PreferenceError::InvalidKey(key.trim().to_owned())),
        }
        self.validate()
    }

    /// Build an executable plus argument vector suitable for
    /// `std::process::Command`.  No shell is involved; `{file}` is expanded
    /// as one argument and a file argument is appended if it was omitted.
    pub fn editor_command(&self, file: &Path) -> Result<(PathBuf, Vec<String>), PreferenceError> {
        self.validate()?;
        let Some(executable) = self.editor_executable.as_deref() else {
            return Err(PreferenceError::InvalidConfig(
                "No editor executable is configured; set editor.executable first.".to_owned(),
            ));
        };
        let file = file.to_string_lossy();
        let mut args = Vec::with_capacity(self.editor_args.len() + 1);
        let mut used_file = false;
        for argument in &self.editor_args {
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

    pub fn to_public_json(&self) -> serde_json::Value {
        serde_json::json!({
            "theme": self.theme.name(),
            "color": color_name(self.color),
            "motion": motion_name(self.motion),
            "iconMode": self.icon_mode.name(),
            "previewVisibility": self.preview_visibility.name(),
            "writerDisplay": self.writer_display.name(),
            "writerTargetOverride": self.writer_target_override,
            "editorExecutable": self.editor_executable,
            "editorArgs": self.editor_args,
        })
    }
}

fn validate_editor_executable(value: &str) -> Result<(), PreferenceError> {
    if value.trim().is_empty() {
        return Err(PreferenceError::InvalidValue(
            "editor.executable cannot be blank.".to_owned(),
        ));
    }
    if value.contains('\0') {
        return Err(PreferenceError::InvalidValue(
            "editor.executable cannot contain NUL characters.".to_owned(),
        ));
    }
    Ok(())
}

fn canonical_key(key: &str) -> String {
    key.trim().to_ascii_lowercase().replace(['-', '.'], "_")
}

fn parse_color(value: &str) -> Option<ColorChoice> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(ColorChoice::Auto),
        "always" => Some(ColorChoice::Always),
        "never" | "plain" => Some(ColorChoice::Never),
        "truecolor" | "true_color" | "24bit" => Some(ColorChoice::TrueColor),
        "ansi256" | "ansi_256" | "256" => Some(ColorChoice::Ansi256),
        "ansi16" | "ansi_16" | "16" => Some(ColorChoice::Ansi16),
        _ => None,
    }
}

fn parse_motion(value: &str) -> Option<MotionChoice> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(MotionChoice::Auto),
        "full" => Some(MotionChoice::Full),
        "reduced" => Some(MotionChoice::Reduced),
        "off" | "none" => Some(MotionChoice::Off),
        _ => None,
    }
}

fn color_name(value: ColorChoice) -> &'static str {
    match value {
        ColorChoice::Auto => "auto",
        ColorChoice::Always => "always",
        ColorChoice::TrueColor => "truecolor",
        ColorChoice::Ansi256 => "ansi256",
        ColorChoice::Ansi16 => "ansi16",
        ColorChoice::Never => "never",
    }
}

fn motion_name(value: MotionChoice) -> &'static str {
    match value {
        MotionChoice::Auto => "auto",
        MotionChoice::Full => "full",
        MotionChoice::Reduced => "reduced",
        MotionChoice::Off => "off",
    }
}

/// Resolve the platform-local cap directory.  Tests should use
/// `ConfigStore::at_dir` or `ConfigStore::at_path` to avoid process-global
/// environment changes.
pub fn config_home_from_env() -> Result<PathBuf, PreferenceError> {
    if let Some(value) = std::env::var_os(CONFIG_HOME_ENV) {
        let value = value.to_string_lossy().trim().to_owned();
        if value.is_empty() {
            return Err(PreferenceError::InvalidConfig(
                "CAP_CONFIG_HOME is set but empty.".to_owned(),
            ));
        }
        return Ok(PathBuf::from(value));
    }

    #[cfg(windows)]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            return Ok(PathBuf::from(local_app_data).join("Capsule").join("cap"));
        }
        if let Some(user_profile) = std::env::var_os("USERPROFILE") {
            return Ok(PathBuf::from(user_profile)
                .join("AppData")
                .join("Local")
                .join("Capsule")
                .join("cap"));
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(local_data) = std::env::var_os("XDG_STATE_HOME") {
            return Ok(PathBuf::from(local_data).join("Capsule").join("cap"));
        }
        if let Some(local_data) = std::env::var_os("XDG_CONFIG_HOME") {
            return Ok(PathBuf::from(local_data).join("Capsule").join("cap"));
        }
        if let Some(home) = std::env::var_os("HOME") {
            return Ok(PathBuf::from(home)
                .join(".local")
                .join("state")
                .join("Capsule")
                .join("cap"));
        }
    }
    Err(PreferenceError::InvalidConfig(
        "Unable to determine the local cap configuration directory; set CAP_CONFIG_HOME."
            .to_owned(),
    ))
}

/// A path-bound preference store.  Binding the path explicitly makes tests
/// deterministic and prevents a DB override from changing preference state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn from_env() -> Result<Self, PreferenceError> {
        Ok(Self::at_dir(&config_home_from_env()?))
    }

    pub fn at_dir(directory: &Path) -> Self {
        Self {
            path: directory.join(CONFIG_FILE_NAME),
        }
    }

    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Preferences, PreferenceError> {
        if !self.path.exists() {
            return Ok(Preferences::default());
        }
        let bytes = read_consistent(&self.path)?;
        let preferences: Preferences = serde_json::from_slice(&bytes)?;
        preferences.validate()?;
        Ok(preferences)
    }

    pub fn save(&self, preferences: &Preferences) -> Result<(), PreferenceError> {
        preferences.validate()?;
        let parent = self.path.parent().ok_or_else(|| {
            PreferenceError::InvalidConfig("Preference path has no parent directory.".to_owned())
        })?;
        fs::create_dir_all(parent)?;

        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{}.{}.{}.tmp",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(CONFIG_FILE_NAME),
            std::process::id(),
            counter
        ));
        let result = (|| -> Result<(), PreferenceError> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            let mut bytes = serde_json::to_vec_pretty(preferences)?;
            bytes.push(b'\n');
            file.write_all(&bytes)?;
            file.flush()?;
            file.sync_all()?;
            drop(file);
            replace_file(&temporary, &self.path)?;
            // A directory sync is not available on every Windows filesystem;
            // the file sync above is the durability boundary we can promise.
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn read_consistent(path: &Path) -> io::Result<Vec<u8>> {
    // ReplaceFileW cannot swap a file while a Windows reader has it open
    // without FILE_SHARE_DELETE.  Keep the read side bounded and retry the
    // short sharing window; each successful read is still one complete file
    // because writers replace by rename rather than truncating in place.
    for attempt in 0..200 {
        match fs::read(path) {
            Ok(bytes) => return Ok(bytes),
            Err(error)
                if cfg!(windows)
                    && matches!(error.raw_os_error(), Some(32 | 33))
                    && attempt < 199 =>
            {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("bounded preference read loop always returns")
}

fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
        const REPLACEFILE_WRITE_THROUGH: u32 = 0x0000_0001;
        let wide = |path: &Path| {
            path.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        };
        let temporary_wide = wide(temporary);
        let destination_wide = wide(destination);

        // ReplaceFileW performs the destination swap without a delete/rename
        // gap.  If the destination has not been created yet, MoveFileExW is
        // the equivalent atomic first-install operation.  Both calls happen
        // after the temporary handle was flushed and closed above.
        #[allow(non_snake_case)]
        unsafe extern "system" {
            fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
            fn ReplaceFileW(
                replaced: *const u16,
                replacement: *const u16,
                backup: *const u16,
                flags: u32,
                exclude: *mut std::ffi::c_void,
                reserved: *mut std::ffi::c_void,
            ) -> i32;
        }

        // Try the replace path first.  A missing destination returns an error
        // and is handled by MoveFileExW below; a reader may briefly hold the
        // old file, so sharing violations receive a bounded retry rather than
        // exposing a spurious config-write failure.
        let mut last_error = None;
        for attempt in 0..200 {
            let replaced = unsafe {
                ReplaceFileW(
                    destination_wide.as_ptr(),
                    temporary_wide.as_ptr(),
                    std::ptr::null(),
                    REPLACEFILE_WRITE_THROUGH,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if replaced != 0 {
                return Ok(());
            }
            let replace_error = io::Error::last_os_error();
            let moved = unsafe {
                MoveFileExW(
                    temporary_wide.as_ptr(),
                    destination_wide.as_ptr(),
                    MOVEFILE_WRITE_THROUGH,
                )
            };
            if moved != 0 {
                return Ok(());
            }
            let move_error = io::Error::last_os_error();
            last_error = Some(move_error);
            let sharing = matches!(replace_error.raw_os_error(), Some(32 | 33))
                || matches!(
                    last_error.as_ref().and_then(io::Error::raw_os_error),
                    Some(32 | 33)
                );
            if !sharing || attempt == 199 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Err(last_error.unwrap_or_else(io::Error::last_os_error))
    }
}

/// A fully resolved, renderer-ready presentation snapshot.  Callers can
/// retain it for one command; no renderer needs to reread environment/config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectivePresentation {
    pub theme: Theme,
    pub output: ResolvedOutputMode,
    pub icon_mode: IconMode,
    pub preview_visibility: PreviewVisibility,
    pub writer_display: WriterDisplay,
    pub writer_target_override: Option<u32>,
    pub editor_executable: Option<String>,
    pub editor_args: Vec<String>,
}

impl Preferences {
    pub fn resolve(
        &self,
        global: &GlobalOptions,
        capabilities: &TerminalCapabilities,
    ) -> Result<EffectivePresentation, PreferenceError> {
        self.validate()?;
        let theme = if let Some(name) = global.theme.as_deref() {
            Theme::parse(name).ok_or_else(|| {
                PreferenceError::InvalidValue(format!(
                    "Unknown theme `{name}`. Choose one of: {}.",
                    Theme::all()
                        .iter()
                        .map(|theme| theme.name())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?
        } else {
            self.theme
        };
        let request = OutputRequest {
            json: global.json,
            quiet: global.quiet,
            plain: global.plain,
            color: global.color.map(cli_color).unwrap_or(self.color),
            motion: global.motion.map(cli_motion).unwrap_or(self.motion),
        };
        let output = resolve_output_mode(request, capabilities);
        let icon_mode = if matches!(self.icon_mode, IconMode::Auto)
            && (global.plain || output.color() == cap_effects::ColorMode::Plain)
        {
            IconMode::Ascii
        } else {
            self.icon_mode
        };
        Ok(EffectivePresentation {
            theme,
            output,
            icon_mode,
            preview_visibility: self.preview_visibility,
            writer_display: self.writer_display,
            writer_target_override: self.writer_target_override,
            editor_executable: self.editor_executable.clone(),
            editor_args: self.editor_args.clone(),
        })
    }
}

/// Resolve presentation from cap-local settings.  This function is the
/// normal command path; callers that deliberately avoid config reads (FX
/// synthetic defaults and an explicit theme preview) can use
/// `resolve_defaults` instead.
pub fn resolve_from_store(
    global: &GlobalOptions,
    capabilities: &TerminalCapabilities,
) -> Result<EffectivePresentation, PreferenceError> {
    ConfigStore::from_env()?
        .load()?
        .resolve(global, capabilities)
}

pub fn resolve_defaults(
    global: &GlobalOptions,
    capabilities: &TerminalCapabilities,
) -> Result<EffectivePresentation, PreferenceError> {
    Preferences::default().resolve(global, capabilities)
}

fn cli_color(value: CliColorMode) -> ColorChoice {
    match value {
        CliColorMode::Auto => ColorChoice::Auto,
        CliColorMode::Always => ColorChoice::Always,
        CliColorMode::Never => ColorChoice::Never,
    }
}

fn cli_motion(value: CliMotionMode) -> MotionChoice {
    match value {
        CliMotionMode::Auto => MotionChoice::Auto,
        CliMotionMode::Full => MotionChoice::Full,
        CliMotionMode::Reduced => MotionChoice::Reduced,
        CliMotionMode::Off => MotionChoice::Off,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn defaults_are_cap_local_and_round_trip_atomically() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::at_dir(directory.path());
        let mut preferences = store.load().unwrap();
        assert_eq!(preferences.theme, Theme::Aurora);
        preferences.set_key("theme", "paper").unwrap();
        preferences
            .set_key("editor.args", r#"["--wait", "{file}"]"#)
            .unwrap();
        store.save(&preferences).unwrap();
        assert_eq!(store.load().unwrap(), preferences);
        assert!(directory.path().join(CONFIG_FILE_NAME).is_file());
        assert!(!directory.path().join("config.json").exists());
    }

    #[test]
    fn unrelated_capsule_keys_and_malformed_documents_fail_closed() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::at_dir(directory.path());
        let mut preferences = Preferences::default();
        assert!(matches!(
            preferences.set_key("location.auto_capture", "true"),
            Err(PreferenceError::InvalidKey(_))
        ));
        fs::write(store.path(), br#"{"location.auto_capture":true}"#).unwrap();
        assert!(store.load().is_err());
    }

    #[test]
    fn editor_uses_argument_array_and_expands_only_file_placeholder() {
        let mut preferences = Preferences::default();
        preferences
            .set_key("editor.executable", r#"C:\Tools\Editor With Space.exe"#)
            .unwrap();
        preferences
            .set_key("editor.args", r#"["--wait", "--name={file}"]"#)
            .unwrap();
        let (executable, args) = preferences
            .editor_command(Path::new(r#"C:\draft file.md"#))
            .unwrap();
        assert_eq!(
            executable,
            PathBuf::from(r#"C:\Tools\Editor With Space.exe"#)
        );
        assert_eq!(args, ["--wait", r#"--name=C:\draft file.md"#]);
    }

    #[test]
    fn precedence_is_json_quiet_plain_then_cli_then_preferences() {
        let preferences = Preferences {
            color: ColorChoice::Always,
            motion: MotionChoice::Full,
            ..Preferences::default()
        };
        let caps = TerminalCapabilities::synthetic(true, 80, true, true, false, false, false);
        let mut global = GlobalOptions::default();
        assert!(matches!(
            preferences.resolve(&global, &caps).unwrap().output,
            ResolvedOutputMode::Human {
                color: cap_effects::ColorMode::TrueColor,
                motion: cap_effects::MotionMode::Full
            }
        ));
        global.plain = true;
        assert!(matches!(
            preferences.resolve(&global, &caps).unwrap().output,
            ResolvedOutputMode::Human {
                color: cap_effects::ColorMode::Plain,
                motion: cap_effects::MotionMode::Off
            }
        ));
        global.plain = false;
        global.quiet = true;
        assert_eq!(
            preferences.resolve(&global, &caps).unwrap().output,
            ResolvedOutputMode::Quiet
        );
        global.json = true;
        assert_eq!(
            preferences.resolve(&global, &caps).unwrap().output,
            ResolvedOutputMode::Json
        );
    }

    #[test]
    fn public_json_contains_no_unrelated_settings() {
        let value = Preferences::default().to_public_json();
        assert!(value.get("theme").is_some());
        assert!(value.get("location").is_none());
    }

    #[test]
    fn concurrent_readers_observe_only_complete_json_documents() {
        use std::sync::Arc;
        use std::thread;

        let directory = tempdir().unwrap();
        let store = Arc::new(ConfigStore::at_dir(directory.path()));
        let reader_store = Arc::clone(&store);
        let reader = thread::spawn(move || {
            for _ in 0..200 {
                if reader_store.path().exists() {
                    let loaded = reader_store.load().expect("atomic JSON replacement");
                    loaded.validate().expect("validated preference snapshot");
                }
                thread::yield_now();
            }
        });
        for index in 0..50 {
            let preferences = Preferences {
                theme: if index % 2 == 0 {
                    Theme::Aurora
                } else {
                    Theme::Paper
                },
                ..Preferences::default()
            };
            store.save(&preferences).unwrap();
        }
        reader.join().unwrap();
    }
}
