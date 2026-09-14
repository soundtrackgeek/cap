//! `cap fx`: a bounded, synthetic visual playground.
//!
//! This command intentionally never opens a Capsule database or preferences
//! file.  It uses the renderer defaults, with only invocation-level global
//! flags (including an explicit `--theme`) applied.

use crate::app::{AppError, CommandOutput};
use crate::cli::{ColorMode as CliColorMode, GlobalOptions, MotionMode as CliMotionMode};
use crate::preferences::{resolve_defaults, PreferenceError};
use crate::ui::themes::Theme;
use cap_effects::{
    animate_text_with_cancel, final_frame, layout_text, render_static, ColorChoice, ColorMode,
    EffectConfig, GradientMode, MotionChoice, OutputRequest, Palette, ResolvedOutputMode,
    TerminalCapabilities, PALETTE_NAMES,
};
use serde_json::json;
use std::io::{self, Write};

const GALLERY_TEXT: &str = "cap fx · synthetic palette gallery";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Demo {
    Gallery,
    Palette(Palette),
    Gradient(GradientMode),
}

impl Demo {
    fn parse(value: Option<&str>) -> Result<Self, AppError> {
        let value = value.unwrap_or("all").trim();
        if value.is_empty()
            || value.eq_ignore_ascii_case("all")
            || value.eq_ignore_ascii_case("gallery")
        {
            return Ok(Self::Gallery);
        }
        if let Some(palette) = Palette::parse(value) {
            return Ok(Self::Palette(palette));
        }
        if let Some(gradient) = GradientMode::parse(value) {
            return Ok(Self::Gradient(gradient));
        }
        Err(AppError::new(
            "INVALID_FX",
            format!(
                "Unknown FX demo `{value}`. Choose a palette ({}), a gradient (text, line, vertical, diagonal, rainbow), or all.",
                PALETTE_NAMES.join(", ")
            ),
            2,
        ))
    }

    fn label(self) -> String {
        match self {
            Self::Gallery => "gallery".to_owned(),
            Self::Palette(palette) => format!("palette:{}", palette.name()),
            Self::Gradient(gradient) => format!("gradient:{gradient:?}").to_ascii_lowercase(),
        }
    }
}

pub fn run(name: Option<&str>, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    run_with_capabilities_and_cancel(name, global, &TerminalCapabilities::detect(), || false)
}

pub fn run_with_capabilities(
    name: Option<&str>,
    global: &GlobalOptions,
    capabilities: &TerminalCapabilities,
) -> Result<CommandOutput, AppError> {
    run_with_capabilities_and_cancel(name, global, capabilities, || false)
}

/// Cancellation is injected by the root CLI's process-level Ctrl+C handler.
/// Keeping this callback generic means tests can cancel before the first
/// frame without installing a signal handler.
pub fn run_with_capabilities_and_cancel<F: Fn() -> bool>(
    name: Option<&str>,
    global: &GlobalOptions,
    capabilities: &TerminalCapabilities,
    should_cancel: F,
) -> Result<CommandOutput, AppError> {
    run_internal(
        name,
        global,
        capabilities,
        None::<&mut Vec<u8>>,
        should_cancel,
    )
}

/// Stream a full-motion demo to an already-owned writer.  The root CLI should
/// pass its stdout lock here; `CommandOutput.human` is empty after a streamed
/// run so the caller cannot print the animation a second time.
pub fn run_with_writer<W: Write>(
    name: Option<&str>,
    global: &GlobalOptions,
    capabilities: &TerminalCapabilities,
    writer: &mut W,
) -> Result<CommandOutput, AppError> {
    run_with_writer_and_cancel(name, global, capabilities, writer, || false)
}

pub fn run_with_writer_and_cancel<W: Write, F: Fn() -> bool>(
    name: Option<&str>,
    global: &GlobalOptions,
    capabilities: &TerminalCapabilities,
    writer: &mut W,
    should_cancel: F,
) -> Result<CommandOutput, AppError> {
    run_internal(name, global, capabilities, Some(writer), should_cancel)
}

fn run_internal<W: Write, F: Fn() -> bool>(
    name: Option<&str>,
    global: &GlobalOptions,
    capabilities: &TerminalCapabilities,
    writer: Option<&mut W>,
    should_cancel: F,
) -> Result<CommandOutput, AppError> {
    let demo = Demo::parse(name)?;
    // `resolve_defaults` is deliberately used instead of `resolve_from_store`:
    // a synthetic playground must remain useful without Capsule installed and
    // must not create/read cap preferences merely to draw a demo.
    let presentation = resolve_defaults(global, capabilities).map_err(preference_error)?;
    let config = demo_config(demo, presentation.theme);
    let plain_preview = render_demo(
        demo,
        &config,
        ColorMode::Plain,
        capabilities.compact_width(),
        presentation.theme,
    );
    let mut fallback_static = false;
    let rendered = match presentation.output {
        ResolvedOutputMode::Json | ResolvedOutputMode::Quiet => String::new(),
        ResolvedOutputMode::Human {
            color,
            motion: cap_effects::MotionMode::Full,
        } => {
            if let Some(writer) = writer {
                animate_demo(
                    writer,
                    demo,
                    &config,
                    presentation.theme,
                    output_request(global),
                    capabilities,
                    should_cancel,
                )?;
                String::new()
            } else {
                // DTO-only callers cannot provide live output.  Keep their
                // path immediate and honest rather than buffering a timed
                // animation that would be dumped after the command returns.
                if should_cancel() {
                    return Err(AppError::new("CANCELLED", "FX demo cancelled.", 130));
                }
                fallback_static = true;
                render_demo(
                    demo,
                    &config,
                    color,
                    capabilities.compact_width(),
                    presentation.theme,
                )
            }
        }
        ResolvedOutputMode::Human { color, .. } => render_demo(
            demo,
            &config,
            color,
            capabilities.compact_width(),
            presentation.theme,
        ),
    };
    let mut output = CommandOutput::new(
        json!({
            "synthetic": true,
            "demo": demo.label(),
            "theme": presentation.theme.name(),
            "palettes": PALETTE_NAMES,
            "gradients": ["text", "line", "vertical", "diagonal", "rainbow"],
            // Machine output is always static and ANSI-free.
            "preview": plain_preview,
        }),
        rendered,
    );
    output.quiet = Some(String::new());
    if fallback_static {
        output.warnings.push(
            "Full-motion FX needs a streaming writer; rendered a static preview instead."
                .to_owned(),
        );
    }
    Ok(output)
}

fn demo_config(demo: Demo, theme: Theme) -> EffectConfig {
    let mut config = theme.effect_config(true);
    match demo {
        Demo::Gallery => {
            config.palette = Palette::Neon;
            config.gradient = GradientMode::Text;
        }
        Demo::Palette(palette) => {
            config.palette = palette;
            config.gradient = GradientMode::Text;
        }
        Demo::Gradient(gradient) => {
            config.gradient = gradient;
        }
    }
    config
}

fn render_demo(
    demo: Demo,
    config: &EffectConfig,
    color: ColorMode,
    width: usize,
    theme: Theme,
) -> String {
    let width = width.max(16);
    match demo {
        Demo::Gallery => {
            let mut output = format!(
                "[synthetic] cap fx · theme:{} · palette gallery\n",
                theme.name()
            );
            for palette_name in PALETTE_NAMES {
                let mut palette_config = config.clone();
                palette_config.palette = Palette::parse(palette_name).unwrap_or(Palette::Neon);
                let text = format!("{palette_name:<7}  luminous sample");
                let frame = final_frame(&layout_text(&text, width, false), &palette_config);
                output.push_str(&render_static(&frame, color));
            }
            output
        }
        Demo::Palette(palette) => {
            let text = format!(
                "[synthetic] theme:{} · {} palette · luminous sample",
                theme.name(),
                palette.name()
            );
            let frame = final_frame(&layout_text(&text, width, false), config);
            render_static(&frame, color)
        }
        Demo::Gradient(gradient) => {
            let text = format!(
                "[synthetic] theme:{} · {gradient:?} gradient · luminous sample",
                theme.name()
            )
            .to_ascii_lowercase();
            let frame = final_frame(&layout_text(&text, width, false), config);
            render_static(&frame, color)
        }
    }
}

fn animate_demo<W: Write, F: Fn() -> bool>(
    writer: &mut W,
    demo: Demo,
    config: &EffectConfig,
    theme: Theme,
    request: OutputRequest,
    capabilities: &TerminalCapabilities,
    should_cancel: F,
) -> Result<(), AppError> {
    // The gallery has enough text to show every source palette but is still
    // one bounded explicit-demo animation (at most three seconds).
    let text = match demo {
        Demo::Gallery => GALLERY_TEXT,
        Demo::Palette(palette) => match palette {
            Palette::Neon => "cap fx · neon",
            Palette::Ocean => "cap fx · ocean",
            Palette::Sunset => "cap fx · sunset",
            Palette::Fire => "cap fx · fire",
            Palette::Aurora => "cap fx · aurora",
            Palette::Ice => "cap fx · ice",
            Palette::Candy => "cap fx · candy",
            Palette::Mono => "cap fx · mono",
        },
        Demo::Gradient(gradient) => match gradient {
            GradientMode::Text => "cap fx · text gradient",
            GradientMode::Line => "cap fx · line gradient",
            GradientMode::Vertical => "cap fx · vertical gradient",
            GradientMode::Diagonal => "cap fx · diagonal gradient",
            GradientMode::Rainbow => "cap fx · rainbow gradient",
        },
    };
    let text = format!("{text} · theme:{}", theme.name());
    let result =
        animate_text_with_cancel(writer, &text, config, request, capabilities, should_cancel)
            .map_err(effect_error)?;
    if result.cancelled {
        return Err(AppError::new("CANCELLED", "FX demo cancelled.", 130));
    }
    Ok(())
}

fn output_request(global: &GlobalOptions) -> OutputRequest {
    OutputRequest {
        json: global.json,
        quiet: global.quiet,
        plain: global.plain,
        color: global.color.map(cli_color).unwrap_or(ColorChoice::Auto),
        motion: global.motion.map(cli_motion).unwrap_or(MotionChoice::Auto),
    }
}

fn cli_color(value: CliColorMode) -> ColorChoice {
    match value {
        CliColorMode::Auto => ColorChoice::Auto,
        CliColorMode::Always => ColorChoice::Always,
        CliColorMode::Never => ColorChoice::Never,
    }
}

fn cli_motion(value: CliMotionMode) -> MotionChoice {
    match value {
        CliMotionMode::Auto => MotionChoice::Auto,
        CliMotionMode::Full => MotionChoice::Full,
        CliMotionMode::Reduced => MotionChoice::Reduced,
        CliMotionMode::Off => MotionChoice::Off,
    }
}

fn effect_error(error: io::Error) -> AppError {
    AppError::new("EFFECT_IO", format!("FX renderer failed: {error}"), 1)
}

fn preference_error(error: PreferenceError) -> AppError {
    AppError::new("INVALID_CONFIG", error.to_string(), 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_is_marked_synthetic_and_has_all_source_options() {
        let caps = TerminalCapabilities::synthetic(false, 80, false, false, false, false, false);
        let output = run_with_capabilities(None, &GlobalOptions::default(), &caps).unwrap();
        assert!(output.data["synthetic"].as_bool().unwrap());
        assert_eq!(output.data["palettes"].as_array().unwrap().len(), 8);
        assert_eq!(output.data["gradients"].as_array().unwrap().len(), 5);
        assert!(output.human.contains("[synthetic]"));
    }

    #[test]
    fn every_palette_and_gradient_parses_without_io() {
        let caps = TerminalCapabilities::synthetic(false, 40, false, false, false, false, false);
        for name in PALETTE_NAMES {
            let output =
                run_with_capabilities(Some(name), &GlobalOptions::default(), &caps).unwrap();
            assert!(output.data["demo"].as_str().unwrap().contains(name));
        }
        for name in ["text", "line", "vertical", "diagonal", "rainbow"] {
            let output =
                run_with_capabilities(Some(name), &GlobalOptions::default(), &caps).unwrap();
            assert!(output.data["demo"].as_str().unwrap().contains(name));
        }
    }

    #[test]
    fn machine_and_plain_modes_never_emit_ansi_or_animation() {
        let caps = TerminalCapabilities::synthetic(true, 40, true, true, false, false, false);
        let machine_global = GlobalOptions {
            json: true,
            motion: Some(CliMotionMode::Full),
            color: Some(CliColorMode::Always),
            ..GlobalOptions::default()
        };
        let output = run_with_capabilities(Some("neon"), &machine_global, &caps).unwrap();
        assert!(output.human.is_empty());
        assert!(!output.data["preview"].as_str().unwrap().contains('\x1b'));

        let plain_global = GlobalOptions {
            plain: true,
            motion: Some(CliMotionMode::Full),
            color: Some(CliColorMode::Always),
            ..GlobalOptions::default()
        };
        let output = run_with_capabilities(Some("neon"), &plain_global, &caps).unwrap();
        assert!(!output.human.contains('\x1b'));
    }

    #[test]
    fn cancellation_is_reported_before_any_commit_surface() {
        let caps = TerminalCapabilities::synthetic(true, 40, true, true, false, false, false);
        let global = GlobalOptions {
            color: Some(CliColorMode::Always),
            motion: Some(CliMotionMode::Full),
            ..GlobalOptions::default()
        };
        let error =
            run_with_capabilities_and_cancel(Some("neon"), &global, &caps, || true).unwrap_err();
        assert_eq!(error.exit_code, 130);
    }

    #[test]
    fn full_motion_streams_before_return_and_does_not_duplicate_human_output() {
        use std::sync::mpsc;
        use std::thread;
        use std::time::Duration;

        struct SignalWriter {
            sender: mpsc::Sender<()>,
        }

        impl Write for SignalWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if !bytes.is_empty() {
                    let _ = self.sender.send(());
                }
                Ok(bytes.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                let _ = self.sender.send(());
                Ok(())
            }
        }

        let width = TerminalCapabilities::detect().width;
        let caps = TerminalCapabilities::synthetic(false, width, true, true, false, false, false);
        let global = GlobalOptions {
            color: Some(CliColorMode::Always),
            motion: Some(CliMotionMode::Full),
            ..GlobalOptions::default()
        };
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut writer = SignalWriter { sender };
            run_with_writer(Some("neon"), &global, &caps, &mut writer)
        });
        receiver
            .recv_timeout(Duration::from_millis(500))
            .expect("stream receives setup/frame before timed demo completes");
        let output = worker.join().unwrap().unwrap();
        assert!(output.human.is_empty());
    }
}
