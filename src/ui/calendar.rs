//! Compact calendar rendering with a narrow-width list fallback.

use std::io::{self, Write};

use cap_effects::{
    animate_frames_with_cancel, final_frame, layout_text, AnimationResult, EffectConfig,
    EffectFrame, OutputRequest, Rgb, TerminalCapabilities,
};
use capsule_core::stats::{self, MemoryCalendar, NaiveDate};

pub fn format_calendar(calendar: &MemoryCalendar, width: usize) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "Calendar {} · {} active days · {} entries · {} words\n",
        calendar.month, calendar.active_days, calendar.total_entries, calendar.total_words
    ));
    if width < 40 {
        output.push_str("(narrow view: active days)\n");
        for day in calendar.days.iter().filter(|day| day.entry_count > 0) {
            output.push_str(&format!(
                "{}  {} {} · {} words\n",
                day.date,
                day.entry_count,
                plural(day.entry_count, "entry"),
                day.word_count
            ));
        }
        if calendar.active_days == 0 {
            output.push_str("No writing days in this month.\n");
        }
        return output.trim_end().to_string();
    }

    output.push_str("Mo  Tu  We  Th  Fr  Sa  Su\n");
    let first_weekday = NaiveDate::parse_from_str(&format!("{}-01", calendar.month), "%Y-%m-%d")
        .map(|date| stats::weekday_index(date) as usize)
        .unwrap_or(0);
    for _ in 0..first_weekday {
        output.push_str("    ");
    }
    for (index, day) in calendar.days.iter().enumerate() {
        let marker = match day.entry_count {
            0 => "·".to_string(),
            1 => "*".to_string(),
            _ => "#".to_string(),
        };
        let day_number = day.date.get(8..10).unwrap_or("??");
        output.push_str(&format!("{day_number}{marker} "));
        let column = first_weekday + index + 1;
        if column.is_multiple_of(7) {
            output.push('\n');
        }
    }
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str("* one entry   # two or more   · quiet day");
    output.trim_end().to_string()
}

/// Calendar is intentionally a static grid: it may be revealed by the same
/// guarded effect machinery but never spends more than the 300ms ceremony
/// budget. The grid itself remains pure and deterministic for 40-column
/// fallback tests.
pub fn calendar_frame_at(
    calendar: &MemoryCalendar,
    width: usize,
    config: &EffectConfig,
    _elapsed_seconds: f64,
) -> EffectFrame {
    calendar_frame_with_icons(calendar, width, config, false)
}

pub fn calendar_frame_with_icons(
    calendar: &MemoryCalendar,
    width: usize,
    config: &EffectConfig,
    ascii: bool,
) -> EffectFrame {
    let mut text = format_calendar(calendar, width);
    if ascii {
        text = text.replace('·', ".");
    }
    let layout = layout_text(&text, width, false);
    let mut frame = final_frame(&layout, config);
    let mut day_index = 0usize;
    let mut in_grid = false;
    for row in &mut frame.rows {
        let row_text = row
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>();
        if row_text.contains("Mo") {
            in_grid = true;
            continue;
        }
        if !in_grid {
            continue;
        }
        for cell in row {
            if day_index >= calendar.days.len() {
                break;
            }
            let Some(day) = calendar.days.get(day_index) else {
                break;
            };
            if matches!(cell.text.as_str(), "·" | "." | "*" | "#") {
                let brightness = match day.entry_count {
                    0 => 0.35,
                    1 => 0.7,
                    _ => 1.0,
                };
                cell.intensity = brightness;
                cell.color = brighten(cell.base_color, brightness);
                day_index += 1;
            }
        }
    }
    frame.done = true;
    frame
}

pub fn animate_calendar<W: Write, F: Fn() -> bool>(
    writer: &mut W,
    calendar: &MemoryCalendar,
    config: &EffectConfig,
    request: OutputRequest,
    capabilities: &TerminalCapabilities,
    should_cancel: F,
) -> io::Result<AnimationResult> {
    animate_frames_with_cancel(
        writer,
        config,
        request,
        capabilities,
        |elapsed, width| calendar_frame_at(calendar, width, config, elapsed),
        should_cancel,
        std::time::Duration::from_millis(300),
    )
}

fn plural(value: i64, singular: &str) -> &'static str {
    if value == 1 {
        // The returned string is static to keep this tiny formatter allocation-free.
        match singular {
            "entry" => "entry",
            _ => "item",
        }
    } else {
        match singular {
            "entry" => "entries",
            _ => "items",
        }
    }
}

fn brighten(base: Rgb, brightness: f64) -> Rgb {
    let channel = |value: u8| (f64::from(value) * brightness).round().clamp(0.0, 255.0) as u8;
    (channel(base.0), channel(base.1), channel(base.2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use capsule_core::stats::ActivityDay;

    fn calendar() -> MemoryCalendar {
        MemoryCalendar {
            month: "2026-09".to_string(),
            year: 2026,
            days_in_month: 30,
            active_days: 2,
            total_entries: 3,
            total_words: 60,
            max_entry_count: 2,
            days: (0..30)
                .map(|index| ActivityDay {
                    date: format!("2026-09-{:02}", index + 1),
                    entry_count: if index == 0 {
                        1
                    } else if index == 1 {
                        2
                    } else {
                        0
                    },
                    word_count: if index == 0 {
                        10
                    } else if index == 1 {
                        50
                    } else {
                        0
                    },
                })
                .collect(),
        }
    }

    #[test]
    fn width_40_calendar_uses_aligned_grid_and_count_brightness() {
        let calendar = calendar();
        let text = format_calendar(&calendar, 40);
        let header = text.lines().nth(1).expect("header");
        assert_eq!(header, "Mo  Tu  We  Th  Fr  Sa  Su");
        let frame = calendar_frame_at(&calendar, 40, &EffectConfig::quick_save(), 0.0);
        let markers = frame
            .rows
            .iter()
            .flat_map(|row| row.iter())
            .filter(|cell| matches!(cell.text.as_str(), "*" | "#"))
            .take(2)
            .map(|cell| cell.intensity)
            .collect::<Vec<_>>();
        assert_eq!(markers, vec![0.7, 1.0]);
    }
}
