//! Text-first recall presentation.  The caller may decorate this string with
//! the normal cap theme; the data and empty state remain legible in plain mode.

use std::io::{self, Write};

use cap_effects::{
    animate_frames_with_cancel, final_frame, layout_text, AnimationResult, EffectConfig,
    EffectFrame, OutputRequest, TerminalCapabilities,
};
use capsule_core::stats::MemoryEntry;

pub fn format_recall(entry: Option<&MemoryEntry>) -> String {
    let Some(entry) = entry else {
        return "No visible memories to recall.".to_string();
    };
    format!("{}{}", recall_opening(entry), recall_body(entry))
}

fn recall_opening(entry: &MemoryEntry) -> String {
    let mut output = String::from("◇ TIME MACHINE UNSEAL\n");
    output.push_str(&entry.date);
    if let Some(mood) = entry
        .mood
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        output.push_str(" · mood:");
        output.push_str(mood.trim());
    }
    output
}

fn recall_body(entry: &MemoryEntry) -> String {
    format!(
        "\n\n{}\n\n{}  Saved to Capsule",
        entry.text,
        entry.uuid.as_deref().unwrap_or("(legacy row without UUID)")
    )
}

pub fn format_on_this_day(date: &str, entries: &[MemoryEntry]) -> String {
    if entries.is_empty() {
        return format!("On this day · {date}\nNo entries from earlier years.");
    }
    let mut output = format!("On this day · {date}\n");
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        output.push_str(&format!(
            "{} · {}\n{}",
            entry.date,
            entry.uuid.as_deref().unwrap_or("(legacy row without UUID)"),
            entry.text
        ));
    }
    output
}

/// Pure time-injected unseal frame.  The date is deliberately part of the
/// prefix, before the authored body, so the temporal cue lands first even
/// when the effect is reduced to a short reveal.
pub fn recall_frame_at(
    entry: Option<&MemoryEntry>,
    width: usize,
    config: &EffectConfig,
    elapsed_seconds: f64,
) -> EffectFrame {
    recall_frame_with_icons(entry, width, config, elapsed_seconds, false)
}

pub fn recall_frame_with_icons(
    entry: Option<&MemoryEntry>,
    width: usize,
    config: &EffectConfig,
    elapsed_seconds: f64,
    ascii: bool,
) -> EffectFrame {
    let Some(entry) = entry else {
        let layout = layout_text("No visible memories to recall.", width, false);
        return final_frame(&layout, config);
    };
    let opened = (elapsed_seconds / 0.35).clamp(0.0, 1.0);
    let mut scene = super::ceremony::capsule_outline(1.0 - opened, width, ascii);
    scene.push_str("\nTIME MACHINE UNSEAL");
    if elapsed_seconds >= 0.35 {
        scene.push('\n');
        scene.push_str(&entry.date);
    }
    let mut frame = final_frame(&layout_text(&scene, width, false), config);
    frame.done = elapsed_seconds >= 0.65;
    frame.elapsed_seconds = elapsed_seconds;
    // The authored body is laid out/rendered only after the small temporal
    // cue has completed. This keeps long entries out of the animated sampler
    // while still producing one complete static body at the end.
    if frame.done {
        let mood = entry
            .mood
            .as_deref()
            .map(|mood| format!("\nmood: {mood}"))
            .unwrap_or_default();
        let body_layout = layout_text(&format!("{mood}{}", recall_body(entry)), width, false);
        frame.rows.extend(final_frame(&body_layout, config).rows);
    }
    frame
}

/// Stream a recall unseal directly to a guarded TTY.  The command result
/// remains JSON/static-safe; callers opt into this helper only for human
/// output and pass the resolved theme/output request explicitly.
pub fn animate_recall<W: Write, F: Fn() -> bool>(
    writer: &mut W,
    entry: Option<&MemoryEntry>,
    config: &EffectConfig,
    request: OutputRequest,
    capabilities: &TerminalCapabilities,
    should_cancel: F,
) -> io::Result<AnimationResult> {
    let text_entry = entry.cloned();
    animate_frames_with_cancel(
        writer,
        config,
        request,
        capabilities,
        move |elapsed, width| recall_frame_at(text_entry.as_ref(), width, config, elapsed),
        should_cancel,
        std::time::Duration::from_millis(700),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> MemoryEntry {
        MemoryEntry {
            id: 7,
            uuid: Some("memory-7".to_string()),
            created_at: "2026-09-14 10:00:00".to_string(),
            date: "2026-09-14".to_string(),
            text: std::iter::repeat_n("long-body", 400)
                .collect::<Vec<_>>()
                .join(" "),
            text_plain: String::new(),
            mood: Some("focused".to_string()),
            hidden: false,
        }
    }

    #[test]
    fn opening_frame_does_not_sample_long_authored_body() {
        let entry = entry();
        let config = EffectConfig::quick_save();
        let frame = recall_frame_at(Some(&entry), 79, &config, 0.0);
        let visible = frame.plain_lines().join("\n");
        assert!(!visible.contains("long-body"));

        let completed = recall_frame_at(Some(&entry), 79, &config, 1.0);
        assert!(completed.done);
        let completed_text = completed.plain_lines().join("\n");
        assert!(completed_text.contains("2026-09-14"));
        assert!(completed_text.contains("long-body"));
        assert_eq!(
            format_recall(Some(&entry)).matches("long-body").count(),
            400
        );
    }

    #[test]
    fn guarded_recall_restores_terminal_state() {
        let entry = entry();
        let caps = TerminalCapabilities::synthetic(true, 80, true, true, false, false, false);
        let request = OutputRequest {
            color: cap_effects::ColorChoice::Always,
            motion: cap_effects::MotionChoice::Full,
            ..OutputRequest::default()
        };
        let mut output = Vec::new();
        let result = animate_recall(
            &mut output,
            Some(&entry),
            &EffectConfig::quick_save(),
            request,
            &caps,
            || false,
        )
        .expect("guarded animation");
        assert!(!result.cancelled);
        let output = String::from_utf8_lossy(&output);
        assert!(output.contains("\x1b[?25l"));
        assert!(output.contains("\x1b[?25h"));
    }
}
