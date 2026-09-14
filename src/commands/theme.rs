//! `cap theme` handlers.  Theme previews are fictional and never need a
//! Capsule database, network provider, or desktop configuration file.

use crate::app::{AppError, CommandOutput};
use crate::cli::{GlobalOptions, ThemeAction};
use crate::preferences::{ConfigStore, EffectivePresentation, IconMode, PreferenceError};
use crate::ui::themes::{render_plain_preview, render_preview_with_mode, summaries, Theme};
use cap_effects::{ColorMode, ResolvedOutputMode, TerminalCapabilities};
use serde_json::json;

const PREVIEW_TEXT: &str = "A small bright thought, kept close.";

pub fn run(action: &ThemeAction, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    run_with_capabilities(action, global, &TerminalCapabilities::detect())
}

/// Deterministic variant for tests and embedders that already captured
/// terminal capabilities for the command.
pub fn run_with_capabilities(
    action: &ThemeAction,
    global: &GlobalOptions,
    capabilities: &TerminalCapabilities,
) -> Result<CommandOutput, AppError> {
    if let ThemeAction::Preview { name } = action {
        return preview(name, global, capabilities);
    }
    let store = ConfigStore::from_env().map_err(preference_error)?;
    run_with_store(action, global, capabilities, &store)
}

/// Path-injected variant for tests and embedders. Preview is self-contained
/// and does not consult this store.
pub fn run_with_store(
    action: &ThemeAction,
    global: &GlobalOptions,
    capabilities: &TerminalCapabilities,
    store: &ConfigStore,
) -> Result<CommandOutput, AppError> {
    match action {
        ThemeAction::List => list(global, store),
        ThemeAction::Preview { name } => preview(name, global, capabilities),
        ThemeAction::Set { name } => set(name, store),
    }
}

fn list(global: &GlobalOptions, store: &ConfigStore) -> Result<CommandOutput, AppError> {
    let preferences = store.load().map_err(preference_error)?;
    let selected = preferences.theme;
    let themes = summaries();
    let human = themes
        .iter()
        .map(|summary| {
            let marker = if summary.name == selected.name() {
                "*"
            } else {
                " "
            };
            format!("{marker} {:<7} — {}", summary.name, summary.description)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = CommandOutput::new(
        json!({"themes": themes, "selected": selected.name()}),
        human,
    );
    output.quiet = Some(String::new());
    if global.json || global.quiet {
        // Keep this branch explicit: the main process chooses the envelope
        // for JSON and the empty quiet string for quiet mode.
        output.human.clear();
    }
    Ok(output)
}

fn preview(
    name: &str,
    global: &GlobalOptions,
    capabilities: &TerminalCapabilities,
) -> Result<CommandOutput, AppError> {
    let theme = Theme::parse(name).ok_or_else(|| unknown_theme(name))?;
    // An explicit preview name is self-contained.  In particular, a broken or
    // absent cap preferences file cannot prevent `cap theme preview neon`.
    let mut presentation =
        crate::preferences::resolve_defaults(global, capabilities).map_err(preference_error)?;
    presentation.theme = theme;
    let plain = render_plain_preview(theme, PREVIEW_TEXT, capabilities.compact_width());
    let shown = match presentation.output {
        ResolvedOutputMode::Json | ResolvedOutputMode::Quiet => String::new(),
        ResolvedOutputMode::Human { color, .. } => {
            let mut text = format!("{} — {}\n", theme.name(), theme.description());
            text.push_str(&render_preview_with_mode(
                theme,
                PREVIEW_TEXT,
                capabilities.compact_width(),
                color,
                presentation.icon_mode == IconMode::Ascii,
            ));
            text
        }
    };
    let mut output = CommandOutput::new(
        json!({
            "theme": theme.name(),
            "synthetic": true,
            "description": theme.description(),
            // JSON is always ANSI-free and therefore useful to scripts too.
            "preview": plain,
        }),
        shown,
    );
    output.quiet = Some(String::new());
    Ok(output)
}

fn set(name: &str, store: &ConfigStore) -> Result<CommandOutput, AppError> {
    let theme = Theme::parse(name).ok_or_else(|| unknown_theme(name))?;
    let mut preferences = store.load().map_err(preference_error)?;
    preferences.theme = theme;
    store.save(&preferences).map_err(preference_error)?;
    let mut output = CommandOutput::new(
        json!({"theme": theme.name(), "saved": true}),
        format!("Theme set to {}.", theme.name()),
    );
    output.quiet = Some(String::new());
    Ok(output)
}

fn unknown_theme(name: &str) -> AppError {
    AppError::new(
        "INVALID_THEME",
        format!(
            "Unknown theme `{name}`. Choose one of: {}.",
            Theme::all()
                .iter()
                .map(|theme| theme.name())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        2,
    )
}

fn preference_error(error: PreferenceError) -> AppError {
    let message = error.to_string();
    let (code, exit) = match error {
        PreferenceError::InvalidKey(_) | PreferenceError::InvalidValue(_) => ("INVALID_CONFIG", 2),
        PreferenceError::Json(_) | PreferenceError::InvalidConfig(_) => ("CONFIG_INVALID", 3),
        PreferenceError::Io(_) => ("CONFIG_WRITE_FAILED", 5),
    };
    AppError::new(code, message, exit)
}

#[allow(dead_code)]
fn output_color(presentation: &EffectivePresentation) -> ColorMode {
    presentation.output.color()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::ThemeAction;
    use cap_effects::TerminalCapabilities;
    use tempfile::tempdir;

    #[test]
    fn preview_is_synthetic_and_ansi_free_in_plain_mode() {
        let caps = TerminalCapabilities::synthetic(false, 40, false, false, false, false, false);
        let global = GlobalOptions {
            plain: true,
            ..GlobalOptions::default()
        };
        let output = run_with_capabilities(
            &ThemeAction::Preview {
                name: "c64".to_owned(),
            },
            &global,
            &caps,
        )
        .unwrap();
        assert!(output.data["synthetic"].as_bool().unwrap());
        assert!(!output.human.contains('\x1b'));
        assert!(output.human.contains('+'));
    }

    #[test]
    fn set_round_trips_only_cap_local_preferences() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::at_dir(directory.path());
        let result = run_with_store(
            &ThemeAction::Set {
                name: "paper".into(),
            },
            &GlobalOptions::default(),
            &TerminalCapabilities::default(),
            &store,
        );
        result.unwrap();
        let stored = ConfigStore::at_dir(directory.path()).load().unwrap();
        assert_eq!(stored.theme, Theme::Paper);
        assert!(!directory.path().join("config.json").exists());
    }
}
