//! Pure rendering and guarded terminal output for the Capsule CLI.
//!
//! The crate deliberately has no Capsule/database dependency. Build a
//! [`layout::TextLayout`], calculate an [`effect::EffectFrame`] at an injected
//! time, and only then choose ANSI/plain output through [`render`] or the
//! guarded [`terminal::animate_text`] entry point.

pub mod effect;
pub mod layout;
pub mod palette;
pub mod render;
pub mod terminal;

pub use effect::{
    final_frame, frame_at, frame_at_seeded, frame_at_with_resize, frame_for, gradient_color,
    hsv_to_rgb, intensity, pick_color, AnimationPlan, EffectBudget, EffectConfig, EffectFrame,
    FrameInput, GradientMode, StyledCell,
};
pub use layout::{grapheme_width, layout_text, sanitize_text, LayoutRow, TextLayout};
pub use palette::{
    lerp_rgb, palette_color, palette_color_for, smoothstep, try_palette_color, Palette,
    PaletteDefinition, PaletteStop, Rgb, PALETTES, PALETTE_NAMES, RGB,
};
pub use render::{
    ansi16_code, ansi256_index, render_frame, render_plain, render_row, render_static, render_text,
    ColorMode,
};
pub use terminal::{
    animate_frames_with_cancel, animate_text, animate_text_with_cancel, prepare_ansi_output,
    resolve_output_mode, AnimationResult, ColorChoice, GuardOptions, MotionChoice, MotionMode,
    OutputRequest, ResolvedOutputMode, TerminalCapabilities, TerminalGuard,
};

pub const COLOR_CLI_SOURCE_REVISION: &str = "c813f12f8578283b68fa124944c0078f15ccdec3";
