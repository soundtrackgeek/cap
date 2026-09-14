//! Resolve presentation once, then stream only the bounded decorative scene.
use std::{io, time::Duration};

use cap_effects::{
    animate_frames_with_cancel, render_frame, EffectConfig, EffectFrame, MotionMode, OutputRequest,
    TerminalCapabilities,
};

use crate::{
    app::AppError,
    cancellation,
    cli::GlobalOptions,
    preferences::{self, EffectivePresentation, IconMode},
};

pub struct MemoryPresentation {
    caps: TerminalCapabilities,
    pub style: EffectivePresentation,
}

impl MemoryPresentation {
    pub fn resolve(global: &GlobalOptions) -> Result<Self, AppError> {
        let caps = TerminalCapabilities::detect();
        let style = preferences::resolve_from_store(global, &caps)
            .map_err(|error| AppError::new("INVALID_CONFIG", error.to_string(), 2))?;
        Ok(Self { caps, style })
    }

    pub fn width(&self) -> usize {
        self.caps.width.saturating_sub(1).clamp(1, 120)
    }
    pub fn ascii(&self) -> bool {
        self.style.icon_mode == IconMode::Ascii
    }

    pub fn present<F>(&self, mut frames: F, budget: Duration) -> Result<String, AppError>
    where
        F: FnMut(f64, usize, &EffectConfig) -> EffectFrame,
    {
        let config = self.style.theme.effect_config(false);
        if self.style.output.is_machine()
            || !self.caps.is_tty
            || self.style.output.motion() != MotionMode::Full
            || self.style.icon_mode == IconMode::Ascii
            || !self.style.output.color().emits_ansi()
        {
            return Ok(render_frame(
                &frames(budget.as_secs_f64(), self.width(), &config),
                self.style.output.color(),
            )
            .trim_end()
            .to_string());
        }
        cancellation::install()
            .map_err(|error| AppError::new("EFFECT_IO", error.to_string(), 1))?;
        let request = OutputRequest {
            color: match self.style.output.color() {
                cap_effects::ColorMode::TrueColor => cap_effects::ColorChoice::TrueColor,
                cap_effects::ColorMode::Ansi256 => cap_effects::ColorChoice::Ansi256,
                cap_effects::ColorMode::Ansi16 => cap_effects::ColorChoice::Ansi16,
                cap_effects::ColorMode::Plain => cap_effects::ColorChoice::Never,
            },
            motion: cap_effects::MotionChoice::Full,
            ..OutputRequest::default()
        };
        let result = animate_frames_with_cancel(
            &mut io::stdout().lock(),
            &config,
            request,
            &self.caps,
            |seconds, width| frames(seconds, width, &config),
            cancellation::requested,
            budget,
        )
        .map_err(|error| AppError::new("EFFECT_IO", error.to_string(), 1))?;
        if result.cancelled {
            return Err(AppError::new("CANCELLED", "Memory view cancelled.", 130));
        }
        Ok(String::new())
    }
}
