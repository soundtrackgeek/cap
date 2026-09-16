//! Text-first seven-day garden rendering.

use std::io::{self, Write};

use cap_effects::{
    animate_frames_with_cancel, final_frame, layout_text, AnimationResult, EffectConfig,
    EffectFrame, OutputRequest, TerminalCapabilities,
};
use capsule_core::stats::{GardenGrowth, MemoryGarden};

pub fn format_garden(garden: &MemoryGarden) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "Seven-day writing garden · {} entries · {} words\n",
        garden.total_entries, garden.total_words
    ));
    for day in &garden.days {
        let label = match day.growth {
            GardenGrowth::Bare => "bare ground",
            GardenGrowth::Seed => "seed",
            GardenGrowth::Sprout => "sprout",
            GardenGrowth::Leaf => "leaf",
            GardenGrowth::Bloom => "bloom",
        };
        output.push_str(&format!(
            "{}  {} {:>12} · {} {} · {} words\n",
            day.date,
            plant_for_growth(day.growth),
            label,
            day.entry_count,
            if day.entry_count == 1 {
                "entry"
            } else {
                "entries"
            },
            day.word_count
        ));
    }
    output.push_str(
        "Legend: bare ground 0 · seed 1–49 · sprout 50–199 · leaf 200–499 · bloom 500+ words",
    );
    output
}

/// Render one seven-day scene at a normalized grow-in progress. Every row is
/// backed by the corresponding day in the metric response; progress only
/// controls how far that day's measured tier has grown.
pub fn garden_scene_at(garden: &MemoryGarden, progress: f64) -> String {
    let progress = if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let mut output = String::new();
    output.push_str(&format!(
        "Seven-day writing garden · {} entries · {} words\n",
        garden.total_entries, garden.total_words
    ));
    for (index, day) in garden.days.iter().enumerate() {
        let row_progress =
            ((progress * garden.days.len().max(1) as f64) - index as f64).clamp(0.0, 1.0);
        let growth = growth_at_progress(day.growth, row_progress);
        let label = match growth {
            GardenGrowth::Bare => "bare ground",
            GardenGrowth::Seed => "seed",
            GardenGrowth::Sprout => "sprout",
            GardenGrowth::Leaf => "leaf",
            GardenGrowth::Bloom => "bloom",
        };
        output.push_str(&format!(
            "{}  {} {:>12} · {} {} · {} words\n",
            day.date,
            plant_for_growth(growth),
            label,
            day.entry_count,
            if day.entry_count == 1 {
                "entry"
            } else {
                "entries"
            },
            day.word_count
        ));
    }
    output.push_str(
        "Legend: bare ground 0 · seed 1–49 · sprout 50–199 · leaf 200–499 · bloom 500+ words",
    );
    output
}

pub fn garden_frame_at(
    garden: &MemoryGarden,
    width: usize,
    config: &EffectConfig,
    elapsed_seconds: f64,
) -> EffectFrame {
    garden_frame_with_icons(garden, width, config, elapsed_seconds, false)
}

pub fn garden_frame_with_icons(
    garden: &MemoryGarden,
    width: usize,
    config: &EffectConfig,
    elapsed_seconds: f64,
    ascii: bool,
) -> EffectFrame {
    let progress = (elapsed_seconds / 0.4).clamp(0.0, 1.0);
    let mut scene = String::new();
    if width >= 42 {
        scene.push_str(&plant_bed_at(garden, progress));
        scene.push('\n');
    }
    if progress >= 1.0 {
        scene.push_str(&garden_scene_at(garden, 1.0));
    } else {
        scene.push_str("Seven-day writing garden");
    }
    if ascii {
        scene = scene
            .replace('·', ".")
            .replace('✧', "*")
            .replace('♧', "v")
            .replace('❧', "Y")
            .replace('✿', "@");
    }
    let layout = layout_text(&scene, width, false);
    let mut frame = final_frame(&layout, config);
    frame.done = progress >= 1.0;
    frame
}

/// Seven distinct small plants share a soil line; a quiet day stays bare.
fn plant_bed_at(garden: &MemoryGarden, progress: f64) -> String {
    let mut rows = vec![String::new(); 5];
    for (index, day) in garden.days.iter().enumerate() {
        let growth = growth_at_progress(
            day.growth,
            (progress * garden.days.len().max(1) as f64 - index as f64).clamp(0.0, 1.0),
        );
        let sprite = match growth {
            GardenGrowth::Bare => ["     ", "     ", "     ", "_____"],
            GardenGrowth::Seed => ["     ", "     ", "  .  ", "_____"],
            GardenGrowth::Sprout => ["     ", " \\ / ", "  |  ", "__|__"],
            GardenGrowth::Leaf => ["  |  ", " \\|/ ", "  |  ", "__|__"],
            GardenGrowth::Bloom => [" (@) ", " \\|/ ", "  |  ", "__|__"],
        };
        for (row, text) in rows.iter_mut().take(4).zip(sprite) {
            row.push_str(text);
            row.push(' ');
        }
        rows[4].push_str(&format!(" {:^3}  ", day.date.get(8..10).unwrap_or("?")));
    }
    rows.iter()
        .map(|row| row.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Stream the garden's pure timed frames through cap-effects' terminal guard.
/// The 400ms budget is independent of the journal query and never runs for
/// JSON, quiet, redirected, or reduced-motion output.
pub fn animate_garden<W: Write, F: Fn() -> bool>(
    writer: &mut W,
    garden: &MemoryGarden,
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
        |elapsed, width| garden_frame_at(garden, width, config, elapsed),
        should_cancel,
        std::time::Duration::from_millis(400),
    )
}

fn plant_for_growth(growth: GardenGrowth) -> &'static str {
    match growth {
        GardenGrowth::Bare => "·",
        GardenGrowth::Seed => "✧",
        GardenGrowth::Sprout => "♧",
        GardenGrowth::Leaf => "❧",
        GardenGrowth::Bloom => "✿",
    }
}

fn growth_at_progress(target: GardenGrowth, progress: f64) -> GardenGrowth {
    let target_level = growth_level(target);
    let level = (target_level as f64 * progress).round() as usize;
    growth_from_level(level.min(target_level))
}

fn growth_level(growth: GardenGrowth) -> usize {
    match growth {
        GardenGrowth::Bare => 0,
        GardenGrowth::Seed => 1,
        GardenGrowth::Sprout => 2,
        GardenGrowth::Leaf => 3,
        GardenGrowth::Bloom => 4,
    }
}

fn growth_from_level(level: usize) -> GardenGrowth {
    match level {
        0 => GardenGrowth::Bare,
        1 => GardenGrowth::Seed,
        2 => GardenGrowth::Sprout,
        3 => GardenGrowth::Leaf,
        _ => GardenGrowth::Bloom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capsule_core::stats::GardenDay;

    fn garden() -> MemoryGarden {
        let today = "2026-09-14";
        MemoryGarden {
            from: "2026-09-08".to_string(),
            to: today.to_string(),
            total_entries: 7,
            total_words: 700,
            days: (0..7)
                .map(|index| GardenDay {
                    date: format!("2026-09-{}", 8 + index),
                    entry_count: 1,
                    word_count: 100,
                    growth: GardenGrowth::Sprout,
                })
                .collect(),
        }
    }

    #[test]
    fn end_frame_contains_all_seven_measured_plants() {
        let garden = garden();
        let frame = garden_frame_at(&garden, 79, &EffectConfig::quick_save(), 0.4);
        assert!(frame.done);
        let visible = frame.plain_lines().join("\n");
        assert_eq!(visible.matches('♧').count(), 7);
        assert!(visible.contains("Legend:"));
    }

    #[test]
    fn grow_in_frames_are_data_derived() {
        let garden = garden();
        let early = garden_scene_at(&garden, 0.2);
        let late = garden_scene_at(&garden, 1.0);
        assert!(early.contains("✧") || early.contains("♧"));
        assert_eq!(late.matches('♧').count(), 7);
    }
}
