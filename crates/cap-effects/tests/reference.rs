use cap_effects::{intensity, palette_color, pick_color, EffectConfig, GradientMode, Palette};

const REFERENCE: &str = include_str!("../../../tests/fixtures/color-cli/reference.json");

#[test]
fn palette_reference_fixture_matches_pinned_python_samples() {
    let fixture: serde_json::Value = serde_json::from_str(REFERENCE).unwrap();
    assert_eq!(
        fixture["sourceRevision"].as_str().unwrap(),
        cap_effects::COLOR_CLI_SOURCE_REVISION
    );
    for palette in cap_effects::PALETTE_NAMES {
        let expected = fixture["paletteSamples"][palette].as_array().unwrap();
        for (index, t) in [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().enumerate() {
            let actual = palette_color(palette, t);
            let expected = expected[index].as_array().unwrap();
            assert_eq!(
                actual,
                (
                    expected[0].as_u64().unwrap() as u8,
                    expected[1].as_u64().unwrap() as u8,
                    expected[2].as_u64().unwrap() as u8,
                ),
                "palette {palette} sample {t}"
            );
        }
    }
}

#[test]
fn gradient_and_intensity_reference_fixture_matches() {
    let fixture: serde_json::Value = serde_json::from_str(REFERENCE).unwrap();
    let samples = [
        (0usize, 0usize, 4usize, 2usize, 8usize, 0usize),
        (0, 3, 4, 2, 8, 0),
        (1, 0, 4, 2, 8, 4),
        (1, 3, 4, 2, 8, 4),
    ];
    for (name, mode) in [
        ("text", GradientMode::Text),
        ("line", GradientMode::Line),
        ("vertical", GradientMode::Vertical),
        ("diagonal", GradientMode::Diagonal),
        ("rainbow", GradientMode::Rainbow),
    ] {
        let config = EffectConfig {
            palette: Palette::Aurora,
            gradient: mode,
            ..EffectConfig::explicit_demo()
        };
        let expected = fixture["gradientSamples"][name].as_array().unwrap();
        for (index, &(row, col, row_len, n_rows, total, offset)) in samples.iter().enumerate() {
            let actual = pick_color(&config, row, col, row_len, n_rows, total, offset);
            let expected = expected[index].as_array().unwrap();
            assert_eq!(
                actual,
                (
                    expected[0].as_u64().unwrap() as u8,
                    expected[1].as_u64().unwrap() as u8,
                    expected[2].as_u64().unwrap() as u8,
                ),
                "gradient {name} sample {index}"
            );
        }
    }

    let config = EffectConfig {
        stagger_seconds: 0.02,
        fade_seconds: 0.45,
        ..EffectConfig::explicit_demo()
    };
    let expected = fixture["intensitySamples"].as_array().unwrap();
    for (index, &(cell, now)) in [(0, 0.0), (0, 0.225), (0, 0.45), (5, 0.2), (5, 0.55)]
        .iter()
        .enumerate()
    {
        let expected = expected[index].as_f64().unwrap();
        assert!((intensity(cell, now, &config) - expected).abs() < 1e-12);
    }
}
