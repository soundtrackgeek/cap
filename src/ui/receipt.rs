//! Receipt models and bounded human rendering for a confirmed capture.

use std::{
    fmt::Write as _,
    io::{self, Write},
    time::Duration,
};

use cap_effects::{
    animate_text_with_cancel, grapheme_width, layout_text, render_text, sanitize_text,
    AnimationResult, ColorChoice, ColorMode, MotionChoice, OutputRequest, TerminalCapabilities,
};
use capsule_core::contracts::{CaptureRequest, CommitReceipt, ContextResult};
use serde::{Deserialize, Serialize};

use super::{themes::Theme, weather::weather_accent};
use crate::contracts::SaveState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptModel {
    pub capture_id: String,
    pub save_state: SaveState,
    pub entry_uuid: String,
    pub entry_number: Option<i64>,
    pub created_at: String,
    pub word_count: usize,
    pub location: ReceiptLocation,
    pub weather: ReceiptWeather,
    pub backup: ReceiptBackup,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptLocation {
    pub status: String,
    pub name: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptWeather {
    pub status: String,
    pub condition: Option<String>,
    pub temp_c: Option<f64>,
    pub fetched_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptBackup {
    pub path: Option<String>,
    pub operation: Option<String>,
}

impl ReceiptModel {
    pub fn from_parts(
        request: &CaptureRequest,
        receipt: &CommitReceipt,
        context: Option<&ContextResult>,
    ) -> Self {
        let location = context
            .and_then(|value| value.location.as_ref())
            .map(|value| ReceiptLocation {
                status: context_status(context.map(|value| value.location_status)),
                name: value.place_name.clone().map(|value| sanitize_text(&value)),
                source: value.source.clone().map(|value| sanitize_text(&value)),
            })
            .unwrap_or_else(|| ReceiptLocation {
                status: context_status(context.map(|value| value.location_status)),
                name: None,
                source: None,
            });
        let weather = context
            .and_then(|value| value.weather.as_ref())
            .map(|value| ReceiptWeather {
                status: context_status(context.map(|value| value.weather_status)),
                condition: value.condition.clone().map(|value| sanitize_text(&value)),
                temp_c: value.temp_c,
                fetched_at: value.fetched_at.map(|value| value.to_rfc3339()),
            })
            .unwrap_or_else(|| ReceiptWeather {
                status: context_status(context.map(|value| value.weather_status)),
                condition: None,
                temp_c: None,
                fetched_at: None,
            });
        Self::from_saved_facts(
            &request.capture_id,
            request.created_at,
            request.text.split_whitespace().count(),
            receipt,
            location,
            weather,
        )
    }

    pub fn from_saved_facts(
        capture_id: &str,
        created_at: chrono::DateTime<chrono::FixedOffset>,
        word_count: usize,
        receipt: &CommitReceipt,
        location: ReceiptLocation,
        weather: ReceiptWeather,
    ) -> Self {
        Self {
            capture_id: sanitize_text(capture_id),
            save_state: SaveState::Committed,
            entry_uuid: sanitize_text(&receipt.uuid),
            entry_number: receipt.display_number,
            created_at: created_at.format("%Y-%m-%d %H:%M").to_string(),
            word_count,
            location,
            weather,
            backup: ReceiptBackup {
                path: receipt
                    .backup_path
                    .as_ref()
                    .map(|value| sanitize_text(&value.to_string_lossy())),
                operation: receipt.backup_operation.as_deref().map(sanitize_text),
            },
        }
    }

    pub fn from_record(record: &crate::recovery::RecoveryRecord) -> Option<Self> {
        let receipt = record.receipt.as_ref()?;
        let (location, weather) = context_parts(record.context.as_ref());
        Some(Self::from_saved_facts(
            &record.capture_id,
            record.entry_created_at,
            record.word_count,
            receipt,
            location,
            weather,
        ))
    }
}

fn context_parts(context: Option<&ContextResult>) -> (ReceiptLocation, ReceiptWeather) {
    let location = context
        .and_then(|value| value.location.as_ref())
        .map(|value| ReceiptLocation {
            status: context_status(context.map(|value| value.location_status)),
            name: value.place_name.clone().map(|value| sanitize_text(&value)),
            source: value.source.clone().map(|value| sanitize_text(&value)),
        })
        .unwrap_or_else(|| ReceiptLocation {
            status: context_status(context.map(|value| value.location_status)),
            name: None,
            source: None,
        });
    let weather = context
        .and_then(|value| value.weather.as_ref())
        .map(|value| ReceiptWeather {
            status: context_status(context.map(|value| value.weather_status)),
            condition: value.condition.clone().map(|value| sanitize_text(&value)),
            temp_c: value.temp_c,
            fetched_at: value.fetched_at.map(|value| value.to_rfc3339()),
        })
        .unwrap_or_else(|| ReceiptWeather {
            status: context_status(context.map(|value| value.weather_status)),
            condition: None,
            temp_c: None,
            fetched_at: None,
        });
    (location, weather)
}

/// Render the compact final receipt. Body text is a static two-line preview;
/// callers may use `render_seal` for the optional heading effect.
pub fn render_human(
    request: &CaptureRequest,
    model: &ReceiptModel,
    theme: Theme,
    color: ColorMode,
    width: usize,
) -> String {
    render_human_with_heading(request, model, theme, color, width, true)
}

/// Render the static portion after an animated heading has already been
/// emitted.  Keeping this separate prevents a second heading from being
/// printed while preserving the same body/receipt layout in scrollback.
pub fn render_human_after_animation(
    request: &CaptureRequest,
    model: &ReceiptModel,
    color: ColorMode,
    width: usize,
) -> String {
    render_human_with_heading(request, model, Theme::Aurora, color, width, false)
}

fn render_human_with_heading(
    request: &CaptureRequest,
    model: &ReceiptModel,
    theme: Theme,
    color: ColorMode,
    width: usize,
    include_heading: bool,
) -> String {
    let width = width.clamp(12, 120);
    let plain = color == ColorMode::Plain;
    let mut output = String::new();
    if include_heading {
        output.push_str(&render_heading(model, theme, color, width));
        output.push('\n');
    }
    let preview = layout_text(
        &sanitize_text(&request.text),
        width.saturating_sub(2).max(1),
        false,
    )
    .plain_lines()
    .into_iter()
    .take(2)
    .collect::<Vec<_>>();
    for line in preview {
        let _ = writeln!(output, "{line}");
    }
    output.push('\n');
    output.push_str(&render_metadata(model, plain, width));
    output.trim_end().to_string()
}

/// Render a receipt whose body is intentionally unavailable (for example a
/// status/retry replay).  It uses the same narrow-width-safe metadata layout.
pub fn render_saved(model: &ReceiptModel, theme: Theme, color: ColorMode, width: usize) -> String {
    let width = width.clamp(12, 120);
    let plain = color == ColorMode::Plain;
    format!(
        "{}\n{}",
        render_heading(model, theme, color, width),
        render_metadata(model, plain, width)
    )
}

/// Build the short effect payload used after a confirmed commit.  The two
/// half-shells surround a diamond; the weather accent is derived only from
/// the persisted condition and is rendered by cap-effects during the reveal.
pub fn seal_text(model: &ReceiptModel) -> String {
    let accent = weather_accent(
        model.weather.condition.as_deref(),
        &model.weather.status,
        false,
    );
    format!("◖ ◇ CAPSULE SEALED ◗ {accent}")
}

/// Animate the post-commit seal through the shared cap-effects renderer.  The
/// renderer enforces the quick-save 650 ms budget, redraw bounds and cursor
/// restoration; this wrapper only supplies the real stdout writer and mode.
pub fn animate_seal<W: Write, F: Fn() -> bool>(
    writer: &mut W,
    model: &ReceiptModel,
    theme: Theme,
    color: ColorMode,
    capabilities: &TerminalCapabilities,
    should_cancel: F,
) -> io::Result<AnimationResult> {
    if !capabilities.is_tty || color == ColorMode::Plain {
        return Ok(AnimationResult {
            frames_rendered: 0,
            resized: false,
            cancelled: false,
            elapsed: Duration::ZERO,
        });
    }
    let request = OutputRequest {
        json: false,
        quiet: false,
        plain: false,
        color: match color {
            ColorMode::TrueColor => ColorChoice::TrueColor,
            ColorMode::Ansi256 => ColorChoice::Ansi256,
            ColorMode::Ansi16 => ColorChoice::Ansi16,
            ColorMode::Plain => ColorChoice::Never,
        },
        motion: MotionChoice::Full,
    };
    animate_text_with_cancel(
        writer,
        &seal_text(model),
        &theme.effect_config(false),
        request,
        capabilities,
        should_cancel,
    )
}

fn context_status(status: Option<capsule_core::contracts::ContextStatus>) -> String {
    status
        .map(|value| serde_json::to_string(&value).unwrap_or_else(|_| "unavailable".to_string()))
        .map(|value| value.trim_matches('"').to_string())
        .unwrap_or_else(|| "skipped".to_string())
}

fn render_heading(model: &ReceiptModel, theme: Theme, color: ColorMode, width: usize) -> String {
    if color == ColorMode::Plain {
        "[ CAPSULE SEALED ]".to_string()
    } else {
        render_text(
            &seal_text(model),
            width.saturating_sub(2).max(1),
            &theme.effect_config(false),
            color,
        )
        .trim_end()
        .to_string()
    }
}

fn render_metadata(model: &ReceiptModel, plain: bool, width: usize) -> String {
    let stamp = render_stamp_from_model(model, plain);
    let mut lines = layout_text(&stamp, width, false).plain_lines();
    let word_suffix = format!("{} words", model.word_count);
    if let Some(last) = lines.last_mut() {
        let joined = format!("{last} · {word_suffix}");
        if grapheme_width(&joined) <= width {
            *last = joined;
        } else {
            lines.push(word_suffix);
        }
    } else {
        lines.push(word_suffix);
    }
    let saved = format!("{}  Saved to Capsule", sanitize_text(&model.entry_uuid));
    lines.extend(layout_text(&saved, width, false).plain_lines());
    lines.join("\n")
}

// Keep this conversion in the receipt module so the renderer never needs a
// database or a ContextService handle.
fn render_stamp_from_model(model: &ReceiptModel, plain: bool) -> String {
    let location = model
        .location
        .name
        .as_deref()
        .unwrap_or(match model.location.status.as_str() {
            "disabled" => "Location capture disabled",
            "skipped" => "Location skipped",
            _ => "Location unavailable",
        });
    let weather = match model.weather.condition.as_deref() {
        Some(condition) => model
            .weather
            .temp_c
            .map(|temp| format!("{} · {temp:.1}°C", sanitize_text(condition)))
            .unwrap_or_else(|| sanitize_text(condition)),
        None => match model.weather.status.as_str() {
            "disabled" => "Weather capture disabled",
            "skipped" => "Weather skipped",
            "cached" => "Weather unavailable (cached)",
            _ => "Weather unavailable",
        }
        .to_string(),
    };
    let accent = weather_accent(
        model.weather.condition.as_deref(),
        &model.weather.status,
        plain,
    );
    format!("{accent} {} · {}", sanitize_text(location), weather)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone, Utc};
    use std::path::PathBuf;

    fn request() -> CaptureRequest {
        CaptureRequest::new(
            "A two line\nentry",
            "cap-1",
            "entry-1",
            PathBuf::from("C:\\journal\\capsule.db"),
            FixedOffset::east_opt(3600)
                .unwrap()
                .with_ymd_and_hms(2026, 9, 14, 18, 42, 0)
                .unwrap(),
        )
    }

    #[test]
    fn model_is_machine_safe_and_body_is_not_in_json() {
        let receipt = CommitReceipt::committed("entry-1", "cap-1", Utc::now());
        let model = ReceiptModel::from_parts(&request(), &receipt, None);
        let value = serde_json::to_value(&model).unwrap();
        assert_eq!(value["saveState"], "committed");
        assert!(value.to_string().contains("entry-1"));
        assert!(!value.to_string().contains("A two line"));
    }

    #[test]
    fn human_receipt_has_static_body_preview_and_saved_line() {
        let receipt = CommitReceipt::committed("entry-1", "cap-1", Utc::now());
        let request = request();
        let model = ReceiptModel::from_parts(&request, &receipt, None);
        let text = render_human(&request, &model, Theme::Paper, ColorMode::Plain, 80);
        assert!(text.contains("CAPSULE SEALED"));
        assert!(text.contains("Saved to Capsule"));
        assert!(!text.contains('\x1b'));
    }

    #[test]
    fn narrow_plain_receipt_wraps_every_line_and_keeps_the_seal_shape() {
        let receipt = CommitReceipt::committed("entry-with-a-long-id", "cap-1", Utc::now());
        let mut request = request();
        request.text = "A deliberately long line that must wrap without overflowing".to_string();
        let model = ReceiptModel::from_parts(&request, &receipt, None);
        let text = render_human(&request, &model, Theme::Paper, ColorMode::Plain, 24);
        assert!(text.lines().all(|line| grapheme_width(line) <= 24));
        assert!(seal_text(&model).contains('◖'));
        assert!(seal_text(&model).contains('◗'));
        assert!(seal_text(&model).contains('◇'));
    }
}
