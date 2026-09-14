//! The five small visual skins used by `cap`.
//!
//! Themes are presentation-only.  They select primitives from
//! `cap-effects`, add a readable border treatment, and never contain journal
//! data or a database/configuration handle.

use cap_effects::{
    final_frame, layout_text, render_row, ColorMode, EffectConfig, GradientMode, Palette,
};
use serde::{Deserialize, Serialize};

/// The built-in terminal skins.  The enum is deliberately closed: a saved
/// preference can only select one of these reviewed presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// The default green/cyan/purple aurora treatment.
    #[default]
    Aurora,
    /// High-energy source-neon colours and a rainbow sweep.
    Neon,
    /// C64-inspired block borders with a blue/cyan palette.
    C64,
    /// Warm phosphor-like amber/orange ink.
    Amber,
    /// Quiet monochrome paper and ASCII rules.
    Paper,
}

impl Theme {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Aurora => "aurora",
            Self::Neon => "neon",
            Self::C64 => "c64",
            Self::Amber => "amber",
            Self::Paper => "paper",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "aurora" => Some(Self::Aurora),
            "neon" => Some(Self::Neon),
            "c64" | "c-64" | "commodore" => Some(Self::C64),
            "amber" => Some(Self::Amber),
            "paper" => Some(Self::Paper),
            _ => None,
        }
    }

    pub const fn all() -> &'static [Self; 5] {
        &THEMES
    }

    pub const fn palette(self) -> Palette {
        match self {
            Self::Aurora => Palette::Aurora,
            Self::Neon => Palette::Neon,
            Self::C64 => Palette::Ocean,
            Self::Amber => Palette::Fire,
            Self::Paper => Palette::Mono,
        }
    }

    pub const fn gradient(self) -> GradientMode {
        match self {
            Self::Aurora => GradientMode::Text,
            Self::Neon => GradientMode::Rainbow,
            Self::C64 => GradientMode::Line,
            Self::Amber => GradientMode::Vertical,
            Self::Paper => GradientMode::Text,
        }
    }

    pub const fn border(self) -> Border {
        match self {
            Self::Aurora => Border {
                top_left: '╭',
                top_right: '╮',
                bottom_left: '╰',
                bottom_right: '╯',
                horizontal: '─',
                vertical: '│',
            },
            Self::Neon => Border {
                top_left: '✦',
                top_right: '✦',
                bottom_left: '╰',
                bottom_right: '╯',
                horizontal: '━',
                vertical: '┃',
            },
            Self::C64 => Border {
                top_left: '█',
                top_right: '█',
                bottom_left: '█',
                bottom_right: '█',
                horizontal: '█',
                vertical: '█',
            },
            Self::Amber => Border {
                top_left: '=',
                top_right: '=',
                bottom_left: '=',
                bottom_right: '=',
                horizontal: '=',
                vertical: '!',
            },
            Self::Paper => Border {
                top_left: '+',
                top_right: '+',
                bottom_left: '+',
                bottom_right: '+',
                horizontal: '-',
                vertical: '|',
            },
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Aurora => "green, cyan and violet aurora",
            Self::Neon => "electric neon with a rainbow sweep",
            Self::C64 => "blue C64 block terminal",
            Self::Amber => "warm amber phosphor",
            Self::Paper => "quiet monochrome paper",
        }
    }

    /// Build the shared renderer configuration for this skin.
    pub fn effect_config(self, explicit_demo: bool) -> EffectConfig {
        let mut config = if explicit_demo {
            EffectConfig::explicit_demo()
        } else {
            EffectConfig::quick_save()
        };
        config.palette = self.palette();
        config.gradient = self.gradient();
        // Paper is intentionally still, even when a caller asks for a full
        // demo.  The command's motion resolver may still choose a static
        // frame; this setting only removes the source shimmer from the skin.
        if matches!(self, Self::Paper) {
            config.shimmer = false;
            config.shimmer_seconds = 0.0;
        }
        config
    }
}

const THEMES: [Theme; 5] = [
    Theme::Aurora,
    Theme::Neon,
    Theme::C64,
    Theme::Amber,
    Theme::Paper,
];

/// A cell-safe border glyph set.  The border is kept separate from renderer
/// colour state so plain output remains readable and testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Border {
    pub top_left: char,
    pub top_right: char,
    pub bottom_left: char,
    pub bottom_right: char,
    pub horizontal: char,
    pub vertical: char,
}

impl Border {
    pub const fn ascii() -> Self {
        Self {
            top_left: '+',
            top_right: '+',
            bottom_left: '+',
            bottom_right: '+',
            horizontal: '-',
            vertical: '|',
        }
    }
}

/// Render a static, fictional theme preview.  The returned string always
/// ends in a newline and contains no database/configuration side effects.
pub fn render_preview(theme: Theme, text: &str, width: usize, color: ColorMode) -> String {
    render_preview_with_mode(theme, text, width, color, color == ColorMode::Plain)
}

/// Render a preview while explicitly selecting the ASCII fallback.  The
/// fallback is used by `--plain`/uncertain encodings unless a caller has
/// deliberately selected Unicode icon mode.
pub fn render_preview_with_mode(
    theme: Theme,
    text: &str,
    width: usize,
    color: ColorMode,
    ascii_only: bool,
) -> String {
    let width = width.max(12);
    let inner_width = width.saturating_sub(4).max(4);
    let border = if ascii_only {
        Border::ascii()
    } else {
        theme.border()
    };
    let config = theme.effect_config(false);
    let layout = layout_text(text, inner_width, false);
    let frame = final_frame(&layout, &config);

    let mut out = String::new();
    out.push(border.top_left);
    out.extend(std::iter::repeat_n(
        border.horizontal,
        width.saturating_sub(2),
    ));
    out.push(border.top_right);
    out.push('\n');

    for row in &frame.rows {
        let rendered = render_row(row, color);
        let rendered = rendered.strip_suffix('\n').unwrap_or(&rendered);
        let row_width = row.iter().map(|cell| cell.cell_width).sum::<usize>();
        let padding = inner_width.saturating_sub(row_width);
        out.push(border.vertical);
        out.push(' ');
        out.push_str(rendered);
        out.extend(std::iter::repeat_n(' ', padding));
        out.push(' ');
        out.push(border.vertical);
        out.push('\n');
    }

    out.push(border.bottom_left);
    out.extend(std::iter::repeat_n(
        border.horizontal,
        width.saturating_sub(2),
    ));
    out.push(border.bottom_right);
    out.push('\n');
    out
}

/// Render a plain preview independently of terminal capability negotiation.
pub fn render_plain_preview(theme: Theme, text: &str, width: usize) -> String {
    render_preview_with_mode(theme, text, width, ColorMode::Plain, true)
}

/// Metadata used by list/config output.  Keeping it owned makes the JSON
/// contract stable if the visual implementation changes later.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSummary {
    pub name: String,
    pub description: String,
    pub palette: String,
    pub gradient: String,
    pub default_theme: bool,
}

pub fn summaries() -> Vec<ThemeSummary> {
    Theme::all()
        .iter()
        .copied()
        .map(|theme| ThemeSummary {
            name: theme.name().to_owned(),
            description: theme.description().to_owned(),
            palette: theme.palette().name().to_owned(),
            gradient: format!("{:?}", theme.gradient()).to_ascii_lowercase(),
            default_theme: theme == Theme::Aurora,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_five_presets_are_distinct_and_parseable() {
        let summaries = summaries();
        assert_eq!(summaries.len(), 5);
        assert_eq!(summaries[0].name, "aurora");
        assert!(summaries
            .iter()
            .all(|summary| Theme::parse(&summary.name).is_some()));
        let previews = Theme::all()
            .iter()
            .map(|theme| render_plain_preview(*theme, "synthetic", 24))
            .collect::<Vec<_>>();
        assert!(previews.iter().all(|preview| preview.starts_with('+')));
        let colored_previews = Theme::all()
            .iter()
            .map(|theme| render_preview(*theme, "synthetic", 24, ColorMode::TrueColor))
            .collect::<Vec<_>>();
        assert_eq!(
            colored_previews
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            5
        );
    }

    #[test]
    fn plain_preview_contains_no_ansi_and_stays_bounded() {
        let preview = render_plain_preview(Theme::C64, "fictional\nentry", 20);
        assert!(!preview.contains('\x1b'));
        assert!(preview.lines().all(|line| line.chars().count() <= 20));
    }

    #[test]
    fn colored_preview_uses_shared_renderer() {
        let preview = render_preview(Theme::Neon, "synthetic", 24, ColorMode::TrueColor);
        assert!(preview.contains("\x1b[38;2;"));
        assert!(preview.contains('✦'));
    }
}
