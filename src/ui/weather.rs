//! Static, accessibility-first weather receipt treatment.

use cap_effects::sanitize_text;
use capsule_core::contracts::{ContextResult, ContextStatus, WeatherObservation};

/// A short status line for a persisted context result.  It deliberately never
/// invents a weather condition when the provider did not return one.
pub fn summary(result: Option<&ContextResult>) -> String {
    let Some(result) = result else {
        return "Weather unavailable".to_string();
    };
    match result.weather_status {
        ContextStatus::Captured => result
            .weather
            .as_ref()
            .map(format_observation)
            .unwrap_or_else(|| "Weather unavailable".to_string()),
        ContextStatus::Cached => result
            .weather
            .as_ref()
            .map(|value| format!("{} (cached)", format_observation(value)))
            .unwrap_or_else(|| "Weather unavailable".to_string()),
        ContextStatus::Disabled => "Weather capture disabled".to_string(),
        ContextStatus::Skipped => "Weather skipped".to_string(),
        ContextStatus::Unavailable => "Weather unavailable".to_string(),
    }
}

pub fn location_summary(result: Option<&ContextResult>) -> String {
    let Some(result) = result else {
        return "Location unavailable".to_string();
    };
    match result.location_status {
        ContextStatus::Captured | ContextStatus::Cached => result
            .location
            .as_ref()
            .and_then(|location| location.place_name.as_deref())
            .map(sanitize_text)
            .unwrap_or_else(|| "Location captured".to_string()),
        ContextStatus::Disabled => "Location capture disabled".to_string(),
        ContextStatus::Skipped => "Location skipped".to_string(),
        ContextStatus::Unavailable => "Location unavailable".to_string(),
    }
}

pub fn render_stamp(result: Option<&ContextResult>, plain: bool) -> String {
    let weather = summary(result);
    let location = location_summary(result);
    let accent = result
        .and_then(|value| value.weather.as_ref())
        .and_then(|value| value.condition.as_deref())
        .map(|condition| weather_accent(Some(condition), "captured", plain))
        .unwrap_or_else(|| weather_accent(None, "unavailable", plain));
    format!("{accent} {location} · {weather}")
}

/// Choose a small, deterministic weather accent from the persisted
/// condition.  This is presentation-only: an absent/unknown condition never
/// becomes a sunny placeholder, and the plain branch stays ASCII-safe.
pub fn weather_accent(condition: Option<&str>, status: &str, plain: bool) -> &'static str {
    if plain {
        return match status {
            "captured" | "cached" => "*",
            _ => "-",
        };
    }
    let value = condition
        .map(sanitize_text)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if value.contains("snow") || value.contains("sleet") || value.contains("ice") {
        "⁙"
    } else if value.contains("rain")
        || value.contains("drizzle")
        || value.contains("shower")
        || value.contains("storm")
        || value.contains("thunder")
    {
        "╱╱"
    } else if value.contains("clear") || value.contains("sun") {
        "✦"
    } else if value.contains("fog") || value.contains("mist") || value.contains("haze") {
        "≋"
    } else if value.contains("cloud") || value.contains("overcast") {
        "☁"
    } else {
        "·"
    }
}

fn format_observation(observation: &WeatherObservation) -> String {
    let condition = observation
        .condition
        .as_deref()
        .map(sanitize_text)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Weather".to_string());
    let temperature = observation
        .temp_c
        .map(|value| format!("{value:.1}°C"))
        .or_else(|| observation.temp_f.map(|value| format!("{value:.1}°F")));
    match temperature {
        Some(value) => format!("{condition} · {value}"),
        None => condition,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn unavailable_weather_never_falls_back_to_sunny() {
        let result = ContextResult {
            weather_status: ContextStatus::Unavailable,
            ..ContextResult::default()
        };
        assert_eq!(summary(Some(&result)), "Weather unavailable");
    }

    #[test]
    fn provider_strings_are_sanitized_in_the_stamp() {
        let result = ContextResult {
            location_status: ContextStatus::Captured,
            weather_status: ContextStatus::Captured,
            location: Some(capsule_core::contracts::ContextLocation {
                latitude: 1.0,
                longitude: 2.0,
                place_name: Some("Bergen\x1b[31m".to_string()),
                place_details: None,
                source: Some("default".to_string()),
            }),
            weather: Some(WeatherObservation {
                provider: Some("open_meteo".to_string()),
                condition: Some("Light rain\x1b[0m".to_string()),
                icon: None,
                temp_c: Some(12.0),
                temp_f: None,
                humidity: None,
                wind_kph: None,
                fetched_at: Some(Utc::now()),
            }),
            ..ContextResult::default()
        };
        let stamp = render_stamp(Some(&result), true);
        assert!(!stamp.contains('\x1b'));
        assert!(stamp.contains("Bergen"));
        assert!(stamp.contains("Light rain"));
    }

    #[test]
    fn weather_accent_follows_condition_without_a_sunny_fallback() {
        assert_eq!(weather_accent(Some("Light rain"), "captured", false), "╱╱");
        assert_eq!(weather_accent(Some("Snow"), "cached", false), "⁙");
        assert_eq!(weather_accent(Some("Clear sky"), "captured", false), "✦");
        assert_eq!(weather_accent(None, "unavailable", false), "·");
        assert_eq!(weather_accent(Some("Clear"), "captured", true), "*");
    }
}
