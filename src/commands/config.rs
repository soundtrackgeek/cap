//! `cap config` handlers for cap-local presentation/editor settings.

use crate::app::{AppError, CommandOutput};
use crate::cli::{ConfigAction, GlobalOptions};
use crate::preferences::{ConfigStore, PreferenceError, Preferences};
use serde_json::json;

pub fn run(action: &ConfigAction, _global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    let store = ConfigStore::from_env().map_err(preference_error)?;
    run_with_store(action, &store)
}

pub fn run_with_store(
    action: &ConfigAction,
    store: &ConfigStore,
) -> Result<CommandOutput, AppError> {
    match action {
        ConfigAction::Show => show(store),
        ConfigAction::Set { key, value } => set(key, value, store),
    }
}

pub fn show(store: &ConfigStore) -> Result<CommandOutput, AppError> {
    let preferences = store.load().map_err(preference_error)?;
    let public = preferences.to_public_json();
    let human = format_preferences(&preferences);
    let mut output = CommandOutput::new(
        json!({
            "path": store.path().to_string_lossy(),
            "exists": store.path().is_file(),
            "preferences": public,
        }),
        human,
    );
    output.quiet = Some(String::new());
    Ok(output)
}

fn set(key: &str, value: &str, store: &ConfigStore) -> Result<CommandOutput, AppError> {
    let mut preferences = store.load().map_err(preference_error)?;
    preferences.set_key(key, value).map_err(preference_error)?;
    store.save(&preferences).map_err(preference_error)?;
    let canonical = key.trim().to_ascii_lowercase().replace(['-', '.'], "_");
    let public = preferences.to_public_json();
    let selected_value = match canonical.as_str() {
        "theme" => json!(preferences.theme.name()),
        "color" => public["color"].clone(),
        "motion" => public["motion"].clone(),
        "icon_mode" => public["iconMode"].clone(),
        "preview_visibility" => public["previewVisibility"].clone(),
        "writer_display" => public["writerDisplay"].clone(),
        "writer_target" | "writer_target_override" => public["writerTargetOverride"].clone(),
        "editor_executable" => public["editorExecutable"].clone(),
        "editor_args" => public["editorArgs"].clone(),
        _ => serde_json::Value::Null,
    };
    let mut output = CommandOutput::new(
        json!({
            "key": canonical,
            "value": selected_value,
            "preferences": public,
            "saved": true,
        }),
        format!("Saved cap preference `{key}`."),
    );
    output.quiet = Some(String::new());
    Ok(output)
}

fn format_preferences(preferences: &Preferences) -> String {
    let value = preferences.to_public_json();
    [
        format!("theme = {}", value["theme"].as_str().unwrap_or("aurora")),
        format!("color = {}", value["color"].as_str().unwrap_or("auto")),
        format!("motion = {}", value["motion"].as_str().unwrap_or("auto")),
        format!(
            "icon_mode = {}",
            value["iconMode"].as_str().unwrap_or("auto")
        ),
        format!(
            "preview_visibility = {}",
            value["previewVisibility"].as_str().unwrap_or("auto")
        ),
        format!(
            "writer_display = {}",
            value["writerDisplay"].as_str().unwrap_or("compact")
        ),
        format!(
            "writer_target_override = {}",
            value["writerTargetOverride"]
                .as_u64()
                .map(|target| target.to_string())
                .unwrap_or_else(|| "none".to_owned())
        ),
        format!(
            "editor_executable = {}",
            value["editorExecutable"].as_str().unwrap_or("none")
        ),
        format!(
            "editor_args = {}",
            serde_json::to_string(&value["editorArgs"]).unwrap_or_else(|_| "[]".to_owned())
        ),
    ]
    .join("\n")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::ConfigAction;
    use tempfile::tempdir;

    #[test]
    fn show_is_read_only_and_set_persists_allowlisted_values() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::at_dir(directory.path());
        let shown = run_with_store(&ConfigAction::Show, &store).unwrap();
        assert!(!shown.data["exists"].as_bool().unwrap());
        assert!(!directory.path().join("preferences.json").exists());
        run_with_store(
            &ConfigAction::Set {
                key: "writer.target".to_owned(),
                value: "500".to_owned(),
            },
            &store,
        )
        .unwrap();
        let preferences = store.load().unwrap();
        assert_eq!(preferences.writer_target_override, Some(500));
    }

    #[test]
    fn capsule_location_keys_are_rejected_without_writing() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::at_dir(directory.path());
        let error = run_with_store(
            &ConfigAction::Set {
                key: "location.auto_capture".to_owned(),
                value: "true".to_owned(),
            },
            &store,
        )
        .unwrap_err();
        assert_eq!(error.exit_code, 2);
        assert!(!directory.path().join("preferences.json").exists());
    }

    #[test]
    fn editor_arguments_are_json_strings_and_never_shell_fragments() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::at_dir(directory.path());
        run_with_store(
            &ConfigAction::Set {
                key: "editor.executable".to_owned(),
                value: r#"C:\Program Files\Editor.exe"#.to_owned(),
            },
            &store,
        )
        .unwrap();
        run_with_store(
            &ConfigAction::Set {
                key: "editor.args".to_owned(),
                value: r#"["--wait", "{file}"]"#.to_owned(),
            },
            &store,
        )
        .unwrap();
        let preferences = store.load().unwrap();
        let (exe, args) = preferences
            .editor_command(std::path::Path::new(r#"C:\draft file.md"#))
            .unwrap();
        assert_eq!(exe.to_string_lossy(), r#"C:\Program Files\Editor.exe"#);
        assert_eq!(args, ["--wait", r#"C:\draft file.md"#]);
    }
}
