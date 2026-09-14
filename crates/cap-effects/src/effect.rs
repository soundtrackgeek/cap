//! Pure, clock-injected frame generation.

use serde::{Deserialize, Serialize};

use crate::layout::TextLayout;
use crate::palette::{lerp_rgb, palette_color_for, python_round_channel, smoothstep, Palette, Rgb};

/// The five gradient placements implemented by the pinned color-cli source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GradientMode {
    /// Progress across all rows as one stream of cells.
    #[default]
    Text,
    /// Restart the gradient on each row.
    Line,
    /// Progress from the first row to the last.
    Vertical,
    /// Blend horizontal and vertical progress.
    Diagonal,
    /// Source-inspired HSV rainbow independent of the selected palette.
    Rainbow,
}

impl GradientMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "text" => Some(Self::Text),
            "line" => Some(Self::Line),
            "vertical" => Some(Self::Vertical),
            "diagonal" => Some(Self::Diagonal),
            "rainbow" => Some(Self::Rainbow),
            _ => None,
        }
    }
}

/// Decorative duration budget. Quick saves never spend more than 650 ms;
/// explicit gallery/demonstration effects may run for at most three seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EffectBudget {
    #[default]
    QuickSave,
    ExplicitDemo,
}

impl EffectBudget {
    pub const fn max_duration_seconds(self) -> f64 {
        match self {
            Self::QuickSave => 0.650,
            Self::ExplicitDemo => 3.0,
        }
    }

    pub const fn max_fps(self) -> u32 {
        match self {
            Self::QuickSave => 30,
            Self::ExplicitDemo => 60,
        }
    }
}

/// Inputs to the pure renderer. Durations are seconds so this type remains
/// easy to construct from CLI/config values and can be serialized in fixtures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectConfig {
    pub palette: Palette,
    pub gradient: GradientMode,
    pub stagger_seconds: f64,
    pub fade_seconds: f64,
    pub fps: u32,
    pub shimmer: bool,
    pub shimmer_width_cells: usize,
    pub shimmer_seconds: f64,
    pub budget: EffectBudget,
}

impl Default for EffectConfig {
    fn default() -> Self {
        Self::quick_save()
    }
}

impl EffectConfig {
    /// Compact defaults for the post-save receipt.
    pub fn quick_save() -> Self {
        Self {
            palette: Palette::Neon,
            gradient: GradientMode::Text,
            stagger_seconds: 0.02,
            fade_seconds: 0.45,
            fps: 30,
            shimmer: true,
            shimmer_width_cells: 14,
            shimmer_seconds: 1.15,
            budget: EffectBudget::QuickSave,
        }
    }

    /// Source-like settings for an explicitly requested visual demo.
    pub fn explicit_demo() -> Self {
        Self {
            fps: 60,
            budget: EffectBudget::ExplicitDemo,
            ..Self::quick_save()
        }
    }

    /// Return a finite, bounded copy suitable for frame generation.
    pub fn bounded(&self) -> Self {
        let mut config = self.clone();
        config.stagger_seconds = finite_nonnegative(config.stagger_seconds);
        config.fade_seconds = finite_nonnegative(config.fade_seconds);
        config.shimmer_seconds = finite_nonnegative(config.shimmer_seconds);
        config.shimmer_width_cells = config.shimmer_width_cells.max(1);
        config.fps = config.fps.clamp(1, config.budget.max_fps());
        if !config.shimmer {
            config.shimmer_seconds = 0.0;
        }
        config
    }
}

fn finite_nonnegative(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

/// The bounded timing information used by an animation loop.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnimationPlan {
    pub total_cells: usize,
    pub reveal_end_seconds: f64,
    pub shimmer_seconds: f64,
    pub end_seconds: f64,
    pub frame_interval_seconds: f64,
    pub max_frames: usize,
    /// Scale applied uniformly to source stagger/fade/shimmer timings to fit
    /// the selected budget. `1.0` means the source timeline is unchanged.
    pub timeline_scale: f64,
    pub budget: EffectBudget,
}

impl AnimationPlan {
    pub fn for_layout(layout: &TextLayout, config: &EffectConfig) -> Self {
        let config = config.bounded();
        let reveal_end = if layout.total_cells == 0 {
            0.0
        } else {
            (layout.total_cells.saturating_sub(1) as f64) * config.stagger_seconds
                + config.fade_seconds
        };
        let raw_end = reveal_end + config.shimmer_seconds;
        let budget = config.budget.max_duration_seconds();
        let timeline_scale = if raw_end > budget && raw_end > 0.0 {
            budget / raw_end
        } else {
            1.0
        };
        let end = raw_end * timeline_scale;
        let frame_interval = 1.0 / f64::from(config.fps);
        let max_frames = (end / frame_interval).ceil() as usize + 1;
        Self {
            total_cells: layout.total_cells,
            reveal_end_seconds: reveal_end * timeline_scale,
            shimmer_seconds: config.shimmer_seconds * timeline_scale,
            end_seconds: end,
            frame_interval_seconds: frame_interval,
            max_frames,
            timeline_scale,
            budget: config.budget,
        }
    }

    pub const fn is_empty(self) -> bool {
        self.total_cells == 0
    }
}

/// One grapheme's state at a point in the reveal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyledCell {
    pub text: String,
    pub cell_width: usize,
    pub base_color: Rgb,
    /// Final colour after reveal intensity and shimmer blending.
    pub color: Rgb,
    pub intensity: f64,
    /// White-blend amount from the shimmer sweep, in `[0, .85]`.
    pub shimmer_boost: f64,
    /// False means the source effect has not revealed this grapheme yet.
    pub visible: bool,
}

/// Pure frame output. No ANSI sequences or terminal I/O are stored here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectFrame {
    pub rows: Vec<Vec<StyledCell>>,
    pub elapsed_seconds: f64,
    pub done: bool,
    /// True when a terminal resize forced an immediate static frame.
    pub resized: bool,
}

/// Explicit inputs for deterministic frame calculation. The pinned source
/// has no random branch, so `seed` is intentionally not consumed today; it is
/// carried to keep receipt/theme callers from reaching for process-global RNG
/// when a future effect adds a seeded variation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct FrameInput {
    pub elapsed_seconds: f64,
    pub seed: u64,
}

impl FrameInput {
    pub const fn from_seconds(elapsed_seconds: f64, seed: u64) -> Self {
        Self {
            elapsed_seconds,
            seed,
        }
    }

    pub const fn elapsed_f64(self) -> f64 {
        self.elapsed_seconds
    }
}

impl EffectFrame {
    pub fn plain_lines(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| {
                row.iter()
                    .filter(|cell| cell.visible)
                    .map(|cell| cell.text.as_str())
                    .collect()
            })
            .collect()
    }

    pub fn total_cells(&self) -> usize {
        self.rows
            .iter()
            .flat_map(|row| row.iter())
            .map(|cell| cell.cell_width)
            .sum()
    }
}

/// Build a frame at an injected monotonic elapsed time.
pub fn frame_at(layout: &TextLayout, config: &EffectConfig, elapsed_seconds: f64) -> EffectFrame {
    frame_at_internal(layout, config, elapsed_seconds, false, false)
}

/// Seeded/time-injected spelling for callers that keep a deterministic effect
/// seed alongside a monotonic timestamp. The current source-backed effect is
/// seed-independent; retaining the argument makes that fact explicit.
pub fn frame_at_seeded(
    layout: &TextLayout,
    config: &EffectConfig,
    elapsed_seconds: f64,
    seed: u64,
) -> EffectFrame {
    let _ = seed;
    frame_at(layout, config, elapsed_seconds)
}

/// Build a frame from an owned [`FrameInput`].
pub fn frame_for(layout: &TextLayout, config: &EffectConfig, input: FrameInput) -> EffectFrame {
    frame_at_seeded(layout, config, input.elapsed_f64(), input.seed)
}

/// Build a frame while tracking terminal width. A width change returns a
/// completed static frame so a caller never redraws against stale geometry.
pub fn frame_at_with_resize(
    layout: &TextLayout,
    config: &EffectConfig,
    elapsed_seconds: f64,
    initial_width: usize,
    current_width: usize,
) -> EffectFrame {
    let resized = initial_width != current_width;
    frame_at_internal(layout, config, elapsed_seconds, resized, resized)
}

/// Return the final, fully revealed static frame.
pub fn final_frame(layout: &TextLayout, config: &EffectConfig) -> EffectFrame {
    let plan = AnimationPlan::for_layout(layout, config);
    frame_at_internal(layout, config, plan.end_seconds, true, false)
}

fn frame_at_internal(
    layout: &TextLayout,
    config: &EffectConfig,
    elapsed_seconds: f64,
    force_complete: bool,
    resized: bool,
) -> EffectFrame {
    let mut config = config.bounded();
    let plan = AnimationPlan::for_layout(layout, &config);
    // Keep the color-cli equations intact while uniformly compressing their
    // clock when a quick-save/explicit-demo budget would otherwise be missed.
    config.stagger_seconds *= plan.timeline_scale;
    config.fade_seconds *= plan.timeline_scale;
    config.shimmer_seconds = plan.shimmer_seconds;
    let elapsed = if elapsed_seconds.is_finite() {
        elapsed_seconds.max(0.0)
    } else {
        0.0
    };
    let complete = force_complete || elapsed >= plan.end_seconds;
    let elapsed_for_math = elapsed.min(plan.end_seconds);
    let n_rows = layout.rows.len();
    let mut rows = Vec::with_capacity(n_rows);
    let mut offset = 0usize;

    for (row_index, row) in layout.rows.iter().enumerate() {
        let row_cells = row.cell_width;
        let mut cell_offset = 0usize;
        let mut styled = Vec::with_capacity(row.graphemes.len());
        for text in &row.graphemes {
            let width = crate::layout::grapheme_width(text);
            let base_color = pick_color(
                &config,
                row_index,
                cell_offset,
                row_cells,
                n_rows,
                layout.total_cells,
                offset,
            );
            let index = offset + cell_offset;
            let intensity = if complete {
                1.0
            } else {
                intensity(index, elapsed_for_math, &config)
            };
            let visible = complete || intensity > 0.02;
            let mut color = scale_rgb(base_color, intensity);
            let mut shimmer_boost = 0.0;
            if !complete
                && config.shimmer_seconds > 0.0
                && elapsed_for_math >= plan.reveal_end_seconds
            {
                let progress = ((elapsed_for_math - plan.reveal_end_seconds)
                    / config.shimmer_seconds)
                    .clamp(0.0, 1.0);
                let position = -(config.shimmer_width_cells as f64)
                    + (layout.total_cells as f64 + 2.0 * config.shimmer_width_cells as f64)
                        * progress;
                let distance =
                    (index as f64 - position).abs() / config.shimmer_width_cells.max(1) as f64;
                if distance < 1.0 {
                    shimmer_boost = (1.0 - distance).powi(2) * 0.85;
                    color = lerp_rgb(color, (255, 255, 255), shimmer_boost);
                }
            }
            styled.push(StyledCell {
                text: text.clone(),
                cell_width: width,
                base_color,
                color,
                intensity,
                shimmer_boost,
                visible,
            });
            cell_offset += width;
        }
        offset += row_cells;
        rows.push(styled);
    }

    EffectFrame {
        rows,
        elapsed_seconds: elapsed,
        done: complete || plan.is_empty(),
        resized,
    }
}

/// Return the source-compatible smoothstep intensity for one display cell.
/// The function is pure and useful for golden/reference tests.
pub fn intensity(index: usize, now: f64, config: &EffectConfig) -> f64 {
    let now = if now.is_finite() { now.max(0.0) } else { 0.0 };
    let lead = index as f64 * config.stagger_seconds;
    if now <= lead {
        0.0
    } else if config.fade_seconds <= 0.0 {
        1.0
    } else {
        smoothstep(((now - lead) / config.fade_seconds).min(1.0))
    }
}

fn scale_rgb(rgb: Rgb, intensity: f64) -> Rgb {
    (
        python_round_channel(f64::from(rgb.0) * intensity),
        python_round_channel(f64::from(rgb.1) * intensity),
        python_round_channel(f64::from(rgb.2) * intensity),
    )
}

/// Select a gradient colour using one of the five source placement formulas.
pub fn pick_color(
    config: &EffectConfig,
    row: usize,
    col: usize,
    row_len: usize,
    n_rows: usize,
    total: usize,
    offset: usize,
) -> Rgb {
    match config.gradient {
        GradientMode::Rainbow => {
            let hue = ((offset + col) as f64 * 0.045).rem_euclid(1.0);
            hsv_to_rgb(hue, 1.0, 1.0)
        }
        GradientMode::Line => {
            let t = col as f64 / row_len.saturating_sub(1).max(1) as f64;
            palette_color_for(config.palette, t)
        }
        GradientMode::Vertical => {
            let t = row as f64 / n_rows.saturating_sub(1).max(1) as f64;
            palette_color_for(config.palette, t)
        }
        GradientMode::Diagonal => {
            let t = 0.5 * (offset + col) as f64 / total.max(1) as f64
                + 0.5 * row as f64 / n_rows.max(1) as f64;
            palette_color_for(config.palette, t)
        }
        GradientMode::Text => {
            let t = (offset + col) as f64 / total.saturating_sub(1).max(1) as f64;
            palette_color_for(config.palette, t)
        }
    }
}

/// Descriptive alias for [`pick_color`] used by theme/receipt callers.
pub fn gradient_color(
    config: &EffectConfig,
    row: usize,
    col: usize,
    row_len: usize,
    n_rows: usize,
    total: usize,
    offset: usize,
) -> Rgb {
    pick_color(config, row, col, row_len, n_rows, total, offset)
}

/// Convert HSV to the source rainbow's rounded RGB channels.
pub fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> Rgb {
    let h = (hue.rem_euclid(1.0) * 6.0).floor() as i32;
    let f = hue.rem_euclid(1.0) * 6.0 - h as f64;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * f);
    let t = value * (1.0 - saturation * (1.0 - f));
    let (r, g, b) = match h % 6 {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    };
    (
        python_round_channel(r * 255.0),
        python_round_channel(g * 255.0),
        python_round_channel(b * 255.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout_text;

    #[test]
    fn source_smoothstep_and_reveal_boundaries_match() {
        let layout = layout_text("abc", 40, false);
        let config = EffectConfig {
            stagger_seconds: 0.02,
            fade_seconds: 0.45,
            shimmer: false,
            shimmer_seconds: 0.0,
            ..EffectConfig::explicit_demo()
        };
        let at_zero = frame_at(&layout, &config, 0.0);
        assert!(at_zero.rows[0].iter().all(|cell| !cell.visible));
        let halfway = frame_at(&layout, &config, 0.225);
        assert!((halfway.rows[0][0].intensity - 0.5).abs() < 1e-12);
        let done = final_frame(&layout, &config);
        assert!(done.done);
        assert!(done.rows[0].iter().all(|cell| cell.visible));
    }

    #[test]
    fn shimmer_is_white_blended_only_during_sweep() {
        let layout = layout_text("abcd", 40, false);
        let mut config = EffectConfig::explicit_demo();
        config.stagger_seconds = 0.0;
        config.fade_seconds = 0.0;
        config.shimmer_seconds = 1.0;
        let plan = AnimationPlan::for_layout(&layout, &config);
        let frame = frame_at(&layout, &config, plan.reveal_end_seconds + 0.5);
        assert!(frame
            .rows
            .iter()
            .flatten()
            .any(|cell| cell.shimmer_boost > 0.0));
        let final_frame = frame_at(&layout, &config, plan.end_seconds);
        assert!(final_frame
            .rows
            .iter()
            .flatten()
            .all(|cell| cell.shimmer_boost == 0.0));
    }

    #[test]
    fn resize_forces_static_completion() {
        let layout = layout_text("resize me", 20, false);
        let frame = frame_at_with_resize(&layout, &EffectConfig::default(), 0.01, 20, 19);
        assert!(frame.done);
        assert!(frame.resized);
        assert!(frame.rows.iter().flatten().all(|cell| cell.visible));
    }

    #[test]
    fn budgets_and_fps_are_bounded() {
        let layout = layout_text(&"x".repeat(500), 40, false);
        let config = EffectConfig {
            fps: 999,
            stagger_seconds: 99.0,
            ..EffectConfig::default()
        };
        let plan = AnimationPlan::for_layout(&layout, &config);
        assert!(plan.end_seconds <= 0.650);
        assert_eq!(config.bounded().fps, 30);
        assert!(plan.max_frames <= 21);
    }

    #[test]
    fn quick_save_scales_reveal_and_shimmer_instead_of_cutting_them_off() {
        let layout = layout_text(&"x".repeat(30), 40, false);
        let config = EffectConfig {
            stagger_seconds: 0.02,
            fade_seconds: 0.45,
            shimmer_seconds: 1.15,
            ..EffectConfig::default()
        };
        let plan = AnimationPlan::for_layout(&layout, &config);
        assert!(plan.timeline_scale < 1.0);
        assert!(plan.end_seconds <= 0.650);
        assert!(plan.reveal_end_seconds < plan.end_seconds);

        let before_sweep = frame_at(&layout, &config, plan.reveal_end_seconds * 0.5);
        assert!(before_sweep.rows.iter().flatten().any(|cell| !cell.visible));
        let sweep_a = frame_at(
            &layout,
            &config,
            plan.reveal_end_seconds + plan.shimmer_seconds * 0.25,
        );
        let sweep_b = frame_at(
            &layout,
            &config,
            plan.reveal_end_seconds + plan.shimmer_seconds * 0.75,
        );
        let position_a = sweep_a
            .rows
            .iter()
            .flatten()
            .position(|cell| cell.shimmer_boost > 0.01);
        let position_b = sweep_b
            .rows
            .iter()
            .flatten()
            .position(|cell| cell.shimmer_boost > 0.01);
        assert!(position_a.is_some() && position_b.is_some());
        assert_ne!(position_a, position_b);
        let done = final_frame(&layout, &config);
        assert!(done.rows.iter().flatten().all(|cell| cell.visible));
        assert!(done
            .rows
            .iter()
            .flatten()
            .all(|cell| cell.shimmer_boost == 0.0));
    }

    #[test]
    fn seeded_input_is_reproducible_for_source_deterministic_math() {
        let layout = layout_text("seed", 20, false);
        let config = EffectConfig::explicit_demo();
        let first = frame_for(&layout, &config, FrameInput::from_seconds(0.35, 7));
        let second = frame_for(&layout, &config, FrameInput::from_seconds(0.35, 7));
        let different_seed = frame_for(&layout, &config, FrameInput::from_seconds(0.35, 8));
        assert_eq!(first, second);
        assert_eq!(first, different_seed);
    }
}
