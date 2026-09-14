//! Synthetic visual QA only: this example never opens a Capsule database.

use std::io::{self, stdout, Write};

use cap_effects::{
    animate_text, ColorChoice, EffectConfig, GradientMode, MotionChoice, OutputRequest, Palette,
    TerminalCapabilities,
};

fn main() -> io::Result<()> {
    let caps = TerminalCapabilities::detect();
    let mut config = EffectConfig::explicit_demo();
    config.palette = Palette::Aurora;
    config.gradient = GradientMode::Diagonal;
    let request = OutputRequest {
        color: ColorChoice::Always,
        motion: MotionChoice::Full,
        ..OutputRequest::default()
    };
    let mut output = stdout().lock();
    writeln!(output, "[synthetic cap-effects demo — no journal access]")?;
    let result = animate_text(
        &mut output,
        "CAPSULE // saved\nUnicode: e\u{301} 🦀 東京",
        &config,
        request,
        &caps,
    )?;
    writeln!(
        output,
        "frames={} resized={} cancelled={} elapsed={}ms",
        result.frames_rendered,
        result.resized,
        result.cancelled,
        result.elapsed.as_millis()
    )?;
    Ok(())
}
