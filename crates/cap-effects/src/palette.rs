//! The palette and gradient primitives ported from `colorcli.py`.
//!
//! The source uses Python's ``round`` (ties-to-even).  [`lerp_rgb`] keeps that
//! detail so a Rust frame and a reference Python frame differ by at most a
//! channel due to floating-point representation.

use serde::{Deserialize, Serialize};

/// An RGB colour in the same channel order used by the source project.
pub type Rgb = (u8, u8, u8);

/// Uppercase alias retained for callers porting the source's ``RGB`` name.
pub type RGB = Rgb;

/// A point in a named palette.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PaletteStop {
    pub position: f64,
    pub color: Rgb,
}

/// A named, ordered palette definition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaletteDefinition {
    pub name: &'static str,
    pub stops: &'static [PaletteStop],
}

const NEON: &[PaletteStop] = &[
    PaletteStop {
        position: 0.0,
        color: (0, 229, 255),
    },
    PaletteStop {
        position: 0.5,
        color: (255, 0, 200),
    },
    PaletteStop {
        position: 1.0,
        color: (255, 230, 0),
    },
];
const OCEAN: &[PaletteStop] = &[
    PaletteStop {
        position: 0.0,
        color: (0, 90, 255),
    },
    PaletteStop {
        position: 0.5,
        color: (0, 220, 255),
    },
    PaletteStop {
        position: 1.0,
        color: (0, 255, 170),
    },
];
const SUNSET: &[PaletteStop] = &[
    PaletteStop {
        position: 0.0,
        color: (255, 94, 0),
    },
    PaletteStop {
        position: 0.4,
        color: (255, 0, 128),
    },
    PaletteStop {
        position: 0.75,
        color: (140, 0, 255),
    },
    PaletteStop {
        position: 1.0,
        color: (60, 0, 255),
    },
];
const FIRE: &[PaletteStop] = &[
    PaletteStop {
        position: 0.0,
        color: (120, 0, 0),
    },
    PaletteStop {
        position: 0.4,
        color: (255, 60, 0),
    },
    PaletteStop {
        position: 0.75,
        color: (255, 160, 0),
    },
    PaletteStop {
        position: 1.0,
        color: (255, 245, 120),
    },
];
const AURORA: &[PaletteStop] = &[
    PaletteStop {
        position: 0.0,
        color: (0, 255, 140),
    },
    PaletteStop {
        position: 0.35,
        color: (0, 200, 255),
    },
    PaletteStop {
        position: 0.7,
        color: (150, 80, 255),
    },
    PaletteStop {
        position: 1.0,
        color: (255, 80, 200),
    },
];
const ICE: &[PaletteStop] = &[
    PaletteStop {
        position: 0.0,
        color: (60, 120, 255),
    },
    PaletteStop {
        position: 0.5,
        color: (150, 220, 255),
    },
    PaletteStop {
        position: 1.0,
        color: (255, 255, 255),
    },
];
const CANDY: &[PaletteStop] = &[
    PaletteStop {
        position: 0.0,
        color: (255, 105, 180),
    },
    PaletteStop {
        position: 0.5,
        color: (160, 90, 255),
    },
    PaletteStop {
        position: 1.0,
        color: (90, 230, 255),
    },
];
const MONO: &[PaletteStop] = &[
    PaletteStop {
        position: 0.0,
        color: (60, 60, 70),
    },
    PaletteStop {
        position: 1.0,
        color: (255, 255, 255),
    },
];

/// The eight palettes present in the pinned color-cli revision.
///
/// This is a slice rather than a process-global mutable map, so the renderer
/// remains deterministic and does not expose shared state to callers.
pub const PALETTES: &[PaletteDefinition] = &[
    PaletteDefinition {
        name: "neon",
        stops: NEON,
    },
    PaletteDefinition {
        name: "ocean",
        stops: OCEAN,
    },
    PaletteDefinition {
        name: "sunset",
        stops: SUNSET,
    },
    PaletteDefinition {
        name: "fire",
        stops: FIRE,
    },
    PaletteDefinition {
        name: "aurora",
        stops: AURORA,
    },
    PaletteDefinition {
        name: "ice",
        stops: ICE,
    },
    PaletteDefinition {
        name: "candy",
        stops: CANDY,
    },
    PaletteDefinition {
        name: "mono",
        stops: MONO,
    },
];

/// Palette names in source order.
pub const PALETTE_NAMES: [&str; 8] = [
    "neon", "ocean", "sunset", "fire", "aurora", "ice", "candy", "mono",
];

/// A strongly typed palette selector for application settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Palette {
    #[default]
    Neon,
    Ocean,
    Sunset,
    Fire,
    Aurora,
    Ice,
    Candy,
    Mono,
}

impl Palette {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Neon => "neon",
            Self::Ocean => "ocean",
            Self::Sunset => "sunset",
            Self::Fire => "fire",
            Self::Aurora => "aurora",
            Self::Ice => "ice",
            Self::Candy => "candy",
            Self::Mono => "mono",
        }
    }

    pub const fn stops(self) -> &'static [PaletteStop] {
        match self {
            Self::Neon => NEON,
            Self::Ocean => OCEAN,
            Self::Sunset => SUNSET,
            Self::Fire => FIRE,
            Self::Aurora => AURORA,
            Self::Ice => ICE,
            Self::Candy => CANDY,
            Self::Mono => MONO,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "neon" => Some(Self::Neon),
            "ocean" => Some(Self::Ocean),
            "sunset" => Some(Self::Sunset),
            "fire" => Some(Self::Fire),
            "aurora" => Some(Self::Aurora),
            "ice" => Some(Self::Ice),
            "candy" => Some(Self::Candy),
            "mono" => Some(Self::Mono),
            _ => None,
        }
    }
}

/// Interpolate two RGB colours with Python-compatible ties-to-even rounding.
pub fn lerp_rgb(a: Rgb, b: Rgb, t: f64) -> Rgb {
    (
        python_round_channel(f64::from(a.0) + (f64::from(b.0) - f64::from(a.0)) * t),
        python_round_channel(f64::from(a.1) + (f64::from(b.1) - f64::from(a.1)) * t),
        python_round_channel(f64::from(a.2) + (f64::from(b.2) - f64::from(a.2)) * t),
    )
}

/// Return a colour from a named source palette, clamping `t` to `[0, 1]`.
///
/// Invalid names are reported by [`try_palette_color`].  This convenience
/// function keeps the direct shape of the source's `palette_color` helper and
/// falls back to `neon` for an invalid name; application-facing code should
/// parse a [`Palette`] first or use the checked helper.
pub fn palette_color(name: &str, t: f64) -> Rgb {
    try_palette_color(name, t).unwrap_or_else(|| palette_color_for(Palette::Neon, t))
}

/// Checked variant of [`palette_color`].
pub fn try_palette_color(name: &str, t: f64) -> Option<Rgb> {
    Palette::parse(name).map(|palette| palette_color_for(palette, t))
}

/// Return a colour from a typed palette, clamping `t` to `[0, 1]`.
pub fn palette_color_for(palette: Palette, t: f64) -> Rgb {
    let stops = palette.stops();
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else if t.is_sign_negative() {
        0.0
    } else {
        1.0
    };

    for pair in stops.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        if t <= right.position {
            let span = right.position - left.position;
            let local = if span <= 0.0 {
                0.0
            } else {
                (t - left.position) / span
            };
            return lerp_rgb(left.color, right.color, local);
        }
    }
    stops.last().map(|stop| stop.color).unwrap_or((0, 0, 0))
}

/// Smoothstep reveal used by color-cli (`x²(3−2x)`).
pub fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Python's `round` for the non-negative channel values used by this crate.
pub(crate) fn python_round_channel(value: f64) -> u8 {
    if !value.is_finite() {
        return 0;
    }
    let floor = value.floor();
    let fraction = value - floor;
    let rounded = if fraction > 0.5 || (fraction == 0.5 && (floor as i64) % 2 != 0) {
        floor + 1.0
    } else {
        floor
    };
    rounded.clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_palettes_and_endpoints_are_present() {
        assert_eq!(PALETTES.len(), 8);
        assert_eq!(
            PALETTE_NAMES,
            ["neon", "ocean", "sunset", "fire", "aurora", "ice", "candy", "mono"]
        );
        assert_eq!(palette_color("neon", 0.0), (0, 229, 255));
        assert_eq!(palette_color("neon", 0.5), (255, 0, 200));
        assert_eq!(palette_color("neon", 1.0), (255, 230, 0));
    }

    #[test]
    fn interpolation_matches_python_tie_rounding() {
        assert_eq!(lerp_rgb((0, 0, 0), (1, 1, 1), 0.5), (0, 0, 0));
        assert_eq!(lerp_rgb((0, 0, 0), (3, 3, 3), 0.5), (2, 2, 2));
        assert_eq!(
            palette_color("not-a-palette", 0.0),
            palette_color("neon", 0.0)
        );
        assert!(try_palette_color("not-a-palette", 0.5).is_none());
    }
}
