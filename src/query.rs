//! CLI adapters for Capsule's path-bound, read-only query services.
//!
//! This module owns no SQL.  It resolves the active database once at the
//! command boundary, binds a `capsule_core::JournalReader`, and turns the
//! shared models into stable CLI data/human layouts.

use std::fmt::Write as _;

use cap_effects::{
    layout_text, render_text, sanitize_text, ColorMode, ResolvedOutputMode, TerminalCapabilities,
};
use capsule_core::{
    db::{self, ResolveRequest, ResolvedCapsule},
    location::ContextSettings,
    models::{Entry, MoodUsage, SearchResponse, TagUsage},
    JournalReader, MetadataPage, ReadOptions, ReadPage,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::{cli::GlobalOptions, preferences};

/// Resolve Capsule once from the invocation's explicit `--db` and the
/// snapshotted process environment.  A missing explicit path is never allowed
/// to fall through to another database candidate.
pub fn resolve(global: &GlobalOptions) -> Result<ResolvedCapsule, String> {
    let request = global
        .db
        .clone()
        .map(ResolveRequest::explicit_database)
        .unwrap_or_default();
    capsule_core::resolve_capsule_from_environment(request).map_err(|error| error.to_string())
}

pub fn open_reader(global: &GlobalOptions) -> Result<(JournalReader, ResolvedCapsule), String> {
    let resolved = resolve(global)?;
    let reader =
        JournalReader::open(resolved.database_path.clone()).map_err(|error| error.to_string())?;
    Ok((reader, resolved))
}

pub fn read_options(limit: u32, offset: u64, include_hidden: bool) -> ReadOptions {
    ReadOptions::new(
        i64::from(limit),
        i64::try_from(offset).unwrap_or(i64::MAX),
        include_hidden,
    )
}

pub fn page_data<T: Serialize>(page: &ReadPage<T>) -> Result<Value, String> {
    serde_json::to_value(page).map_err(|error| error.to_string())
}

pub fn metadata_data<T: Serialize>(page: &MetadataPage<T>) -> Result<Value, String> {
    serde_json::to_value(page).map_err(|error| error.to_string())
}

/// Preserve every search diagnostic while presenting the entry collection as
/// the CLI's conventional `items` page field.
pub fn search_data(response: &SearchResponse) -> Result<Value, String> {
    let has_more = response
        .offset
        .saturating_add(response.entries.len() as i64)
        < response.total;
    Ok(json!({
        "items": response.entries,
        "total": response.total,
        "limit": response.limit,
        "offset": response.offset,
        "hasMore": has_more,
        "mode": response.mode,
        "usedFts": response.used_fts,
        "parsedTokens": response.parsed_tokens,
        "warnings": response.warnings,
    }))
}

pub fn context_data(
    settings: &ContextSettings,
    resolved: &ResolvedCapsule,
    global: &GlobalOptions,
) -> Result<(Value, Vec<String>), String> {
    let mut policy = settings.policy.clone();
    let mut warnings = Vec::new();
    let request_mode = if global.no_context {
        policy.auto_capture = false;
        policy.allow_network = false;
        policy.allow_cache = false;
        warnings.push("Context is disabled for this invocation by --no-context.".to_string());
        "no_context"
    } else if global.offline {
        policy.allow_network = false;
        warnings.push(
            "Network context access is disabled for this invocation by --offline.".to_string(),
        );
        "offline"
    } else {
        "normal"
    };

    if !resolved.config_valid {
        warnings.push(resolved.config_error.clone().unwrap_or_else(|| {
            "Resolved context configuration is missing or invalid.".to_string()
        }));
    }
    if !settings.valid && resolved.config_valid {
        warnings
            .push("Context settings could not be loaded; safe defaults are active.".to_string());
    }

    let mut data = serde_json::to_value(policy).map_err(|error| error.to_string())?;
    let object = data
        .as_object_mut()
        .ok_or_else(|| "context policy did not serialize as an object".to_string())?;
    object.insert(
        "configPath".to_string(),
        json!(resolved.config_path.as_deref().map(db::path_to_string)),
    );
    object.insert("configSource".to_string(), json!(resolved.config_source));
    object.insert(
        "configExists".to_string(),
        json!(resolved
            .config_path
            .as_deref()
            .is_some_and(std::path::Path::is_file)),
    );
    object.insert(
        "configValid".to_string(),
        json!(resolved.config_valid && settings.valid),
    );
    object.insert("requestMode".to_string(), json!(request_mode));
    object.insert("warnings".to_string(), json!(warnings));
    Ok((data, warnings))
}

pub fn doctor_data(global: &GlobalOptions) -> Result<(Value, Vec<String>), String> {
    let capabilities = TerminalCapabilities::detect();
    let resolved = resolve(global)?;
    let mut warnings = resolved.capabilities.warnings.clone();
    if !resolved.capabilities.supports_read() {
        warnings.extend(resolved.capabilities.write_support_reasons.clone());
    }
    let report = json!({
        "databasePath": db::path_to_string(&resolved.database_path),
        "databaseSource": resolved.database_source,
        "databaseIdentity": resolved.database_identity,
        "backupDirectory": db::path_to_string(&resolved.backup_directory),
        "backupSource": resolved.backup_source,
        "configPath": resolved.config_path.as_deref().map(db::path_to_string),
        "configSource": resolved.config_source,
        "configValid": resolved.config_valid,
        "settingsPath": resolved.settings_path.as_deref().map(db::path_to_string),
        "settingsSource": resolved.settings_source,
        "settingsValid": resolved.settings_valid,
        "safeSettings": resolved.settings,
        "diagnostics": resolved.diagnostics,
        "capabilities": resolved.capabilities,
        "terminal": capabilities,
        "warnings": warnings,
    });
    Ok((report, warnings))
}

/// Resolve reviewed presentation preferences and render a static, bounded
/// human layout.  JSON/quiet callers still receive a plain string so ANSI can
/// never leak if a caller accidentally displays the field.
pub fn human_text(text: &str, global: &GlobalOptions) -> Result<String, String> {
    let capabilities = TerminalCapabilities::detect();
    // Machine modes never display this field.  Keep the fallback plain and
    // avoid touching cap-local preferences so `--json doctor` remains useful
    // even when a presentation file is malformed.
    if global.json || global.quiet {
        return Ok(render_human_text(text, &capabilities, None));
    }
    let presentation = preferences::resolve_from_store(global, &capabilities)
        .map_err(|error| error.to_string())?;
    Ok(render_human_text(text, &capabilities, Some(&presentation)))
}

fn render_human_text(
    text: &str,
    capabilities: &TerminalCapabilities,
    presentation: Option<&preferences::EffectivePresentation>,
) -> String {
    let sanitized = sanitize_text(text);
    // Reading uses the terminal's available columns, independently of the
    // compact receipt/effect width. Leave one cell to avoid terminal autowrap.
    let width = capabilities.width.saturating_sub(1).max(1);
    if let Some(presentation) = presentation {
        if let ResolvedOutputMode::Human { color, .. } = presentation.output {
            if color != ColorMode::Plain {
                return render_text(
                    &sanitized,
                    width,
                    &presentation.theme.effect_config(false),
                    color,
                );
            }
        }
    }
    layout_text(&sanitized, width, false)
        .plain_lines()
        .join("\n")
}

pub fn format_entries(page: &ReadPage<Entry>) -> String {
    if page.items.is_empty() {
        return "No entries found.".to_string();
    }
    let mut output = String::new();
    for (index, entry) in page.items.iter().enumerate() {
        if index > 0 {
            output.push_str("\n\n");
        }
        let title = entry
            .title
            .as_deref()
            .or(entry.summary.as_deref())
            .map_or_else(
                || "(untitled)".to_string(),
                |value| compact_field(value, 96),
            );
        let mood = entry
            .mood
            .as_deref()
            .map(|value| format!(" · mood:{}", compact_field(value, 48)))
            .unwrap_or_default();
        let _ = write!(
            output,
            "#{id} {date} {title}{mood}\n  {body}",
            id = entry.id,
            date = entry.created_at,
            title = title,
            mood = mood,
            body = entry_body(&entry.text_plain),
        );
    }
    output
}

pub fn format_entry(entry: &Entry) -> String {
    let mut output = String::new();
    let title = entry.title.as_deref().unwrap_or("(untitled)");
    let _ = writeln!(output, "#{id} {uuid}", id = entry.id, uuid = entry.uuid);
    let _ = writeln!(output, "{}", entry.created_at);
    let _ = writeln!(output, "Title: {title}");
    if let Some(summary) = entry.summary.as_deref() {
        let _ = writeln!(output, "Summary: {summary}");
    }
    if let Some(mood) = entry.mood.as_deref() {
        let _ = writeln!(output, "Mood: {mood}");
    }
    if !entry.tags.is_empty() {
        let tags = entry
            .tags
            .iter()
            .map(|tag| tag.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(output, "Tags: {tags}");
    }
    if let Some(location) = entry.location.as_ref() {
        if let Some(place) = location.place_name.as_deref() {
            let _ = writeln!(output, "Place: {place}");
        }
        if let Some(condition) = location.weather_condition.as_deref() {
            let _ = writeln!(output, "Weather: {condition}");
        }
    }
    output.push('\n');
    // Detail views intentionally use Capsule's authored `text`, preserving
    // markdown/newline semantics. Terminal controls are stripped by the final
    // presentation pass, never by the stored projection.
    output.push_str(&entry.text);
    output
}

pub fn format_search(response: &SearchResponse) -> String {
    if response.entries.is_empty() {
        return "No entries found.".to_string();
    }
    let page = ReadPage {
        items: response.entries.clone(),
        total: response.total,
        limit: response.limit,
        offset: response.offset,
        has_more: response
            .offset
            .saturating_add(response.entries.len() as i64)
            < response.total,
    };
    format_entries(&page)
}

pub fn format_tags(page: &MetadataPage<TagUsage>) -> String {
    if page.items.is_empty() {
        return "No tags found.".to_string();
    }
    page.items
        .iter()
        .map(|tag| format!("{} ({})", tag.name, tag.entry_count))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_moods(page: &MetadataPage<MoodUsage>) -> String {
    if page.items.is_empty() {
        return "No moods found.".to_string();
    }
    page.items
        .iter()
        .map(|mood| format!("{} ({})", mood.label, mood.entry_count))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
}

pub fn format_context_report(value: &Value) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "Context settings");
    let _ = writeln!(
        output,
        "  Request mode: {}",
        value["requestMode"].as_str().unwrap_or("normal")
    );
    let _ = writeln!(
        output,
        "  Auto capture: {}",
        yes_no(value["autoCapture"].as_bool().unwrap_or(false))
    );
    let _ = writeln!(
        output,
        "  Default place: {}",
        if value["useDefaultLocation"].as_bool().unwrap_or(false) {
            value["defaultLocationName"].as_str().unwrap_or("(unnamed)")
        } else {
            "off"
        }
    );
    let _ = writeln!(
        output,
        "  Method: {}",
        value["autoCaptureMethod"].as_str().unwrap_or("default")
    );
    let _ = writeln!(
        output,
        "  Weather provider: {}",
        value["weatherProvider"].as_str().unwrap_or("default")
    );
    let _ = writeln!(
        output,
        "  Geocoding cache: {}",
        value["geocodingCacheHours"]
            .as_u64()
            .map_or_else(|| "default".to_string(), |hours| format!("{hours} hours"))
    );
    let _ = writeln!(
        output,
        "  Config: {} ({})",
        value["configPath"].as_str().unwrap_or("not found"),
        if value["configValid"].as_bool().unwrap_or(false) {
            "valid"
        } else {
            "missing/invalid"
        }
    );
    let _ = writeln!(
        output,
        "  Network: {}; cache: {}",
        yes_no(value["allowNetwork"].as_bool().unwrap_or(false)),
        yes_no(value["allowCache"].as_bool().unwrap_or(false))
    );
    if let Some(warnings) = value["warnings"].as_array() {
        for warning in warnings.iter().filter_map(Value::as_str) {
            let _ = writeln!(output, "  Warning: {warning}");
        }
    }
    output.trim_end().to_string()
}

pub fn format_doctor_report(value: &Value) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "Capsule doctor");
    let _ = writeln!(
        output,
        "  Database: {} [{}]",
        value["databasePath"].as_str().unwrap_or("unknown"),
        value["databaseSource"].as_str().unwrap_or("unknown")
    );
    let _ = writeln!(
        output,
        "  Backup: {} [{}]",
        value["backupDirectory"].as_str().unwrap_or("unknown"),
        value["backupSource"].as_str().unwrap_or("unknown")
    );
    let _ = writeln!(
        output,
        "  Config: {} ({})",
        value["configPath"].as_str().unwrap_or("not found"),
        if value["configValid"].as_bool().unwrap_or(false) {
            "valid"
        } else {
            "missing/invalid"
        }
    );
    let schema = &value["capabilities"]["schema"];
    let _ = writeln!(
        output,
        "  Schema: {} tables; read {}",
        schema["tableNames"].as_array().map_or(0, Vec::len),
        yes_no(schema["supportsRead"].as_bool().unwrap_or(false))
    );
    let terminal = &value["terminal"];
    let _ = writeln!(
        output,
        "  Terminal: {} wide, TTY {}, ANSI {}",
        terminal["width"].as_u64().unwrap_or(0),
        yes_no(terminal["is_tty"].as_bool().unwrap_or(false)),
        yes_no(terminal["ansi_support"].as_bool().unwrap_or(false))
    );
    if let Some(warnings) = value["warnings"].as_array() {
        for warning in warnings.iter().filter_map(Value::as_str) {
            let _ = writeln!(output, "  Warning: {warning}");
        }
    }
    output.trim_end().to_string()
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn entry_body(value: &str) -> String {
    let sanitized = sanitize_text(value);
    if sanitized.trim().is_empty() {
        "(empty)".to_string()
    } else {
        sanitized.replace('\n', "\n  ")
    }
}

/// Truncate one-line CLI fields by terminal cells while preserving grapheme
/// clusters. `layout_text` owns the Unicode segmentation and width policy used
/// by the renderer, so emoji ZWJ sequences and combining marks are never cut
/// between code points.
fn compact_field(value: &str, max_cells: usize) -> String {
    let sanitized = sanitize_text(value);
    let line = sanitized.lines().next().unwrap_or_default().trim();
    if line.is_empty() {
        return "(empty)".to_string();
    }

    let max_cells = max_cells.max(4);
    let content_cells = max_cells.saturating_sub(3).max(1);
    let layout = layout_text(line, content_cells, false);
    let mut compact = layout
        .rows
        .first()
        .map(|row| row.text())
        .unwrap_or_default();

    // Probe at least two cells so a single wide grapheme can be recognized as
    // exceeding the content budget even when the renderer substitutes it for
    // a one-cell replacement at the smaller width.
    let probe = layout_text(line, content_cells.max(2), false);
    let truncated = probe.rows.len() > 1
        || probe
            .rows
            .first()
            .is_some_and(|row| row.cell_width > content_cells);
    if truncated {
        compact.push_str("...");
    }
    compact
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_effects::grapheme_width;

    #[test]
    fn read_layout_uses_available_width_in_plain_color_and_machine_modes() {
        let text = format!(
            "{}\n\nSecond paragraph: e\u{301} 👨‍👩‍👧‍👦 界",
            "readable text ".repeat(40)
        );
        for columns in [1, 2, 40, 80, 120, 240] {
            let capabilities =
                TerminalCapabilities::synthetic(true, columns, true, true, false, false, false);
            for plain in [false, true] {
                let global = GlobalOptions {
                    plain,
                    ..GlobalOptions::default()
                };
                let presentation = preferences::resolve_defaults(&global, &capabilities).unwrap();
                for presentation in [None, Some(&presentation)] {
                    let output = render_human_text(&text, &capabilities, presentation);
                    let clean = sanitize_text(&output);
                    let available = columns.saturating_sub(1).max(1);
                    assert_eq!(grapheme_width(clean.lines().next().unwrap()), available);
                    assert!(clean.lines().all(|line| grapheme_width(line) <= available));
                    if available >= 2 {
                        assert_eq!(clean.replace('\n', ""), text.replace('\n', ""));
                        assert!(clean.contains("e\u{301}"));
                        assert!(clean.contains("👨‍👩‍👧‍👦"));
                        assert!(clean.contains('界'));
                    }
                }
            }
        }
    }

    #[test]
    fn search_data_preserves_diagnostics_and_items_shape() {
        let response = SearchResponse {
            entries: Vec::new(),
            total: 0,
            limit: 20,
            offset: 0,
            mode: capsule_core::models::SearchMode::Keyword,
            used_fts: false,
            parsed_tokens: Vec::new(),
            warnings: vec!["FTS fallback".to_string()],
        };
        let data = search_data(&response).expect("data");
        assert!(data.get("items").is_some());
        assert_eq!(data["warnings"][0], "FTS fallback");
        assert_eq!(data["usedFts"], false);
    }

    #[test]
    fn entry_bodies_sanitize_terminal_controls_and_preserve_paragraphs() {
        let value = "hello\x1b[31m world\x1b[0m";
        assert_eq!(entry_body(value), "hello world");
        assert_eq!(entry_body("first\n\nlast"), "first\n  \n  last");
        assert_eq!(entry_body(" \n\t"), "(empty)");
    }

    #[test]
    fn metadata_keeps_unicode_graphemes_and_bounds_long_fields() {
        let family = "👨‍👩‍👧‍👦";
        let value = format!("{}tail", family.repeat(100));
        let snippet = compact_field(&value, 96);
        assert!(snippet.ends_with("..."));
        assert!(grapheme_width(&snippet) <= 96);
        assert!(snippet.trim_end_matches("...").ends_with(family));

        let metadata = "界".repeat(1_000);
        let title = compact_field(&metadata, 96);
        let mood = compact_field(&metadata, 48);
        assert!(title.ends_with("..."));
        assert!(mood.ends_with("..."));
        assert!(grapheme_width(&title) <= 96);
        assert!(grapheme_width(&mood) <= 48);
    }
}
