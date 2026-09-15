mod support;

use serde_json::{json, Value};
use support::Fixture;

fn configure(fixture: &Fixture, place: &str) {
    std::fs::write(
        &fixture.paths().config,
        serde_json::to_vec(&json!({
            "location.auto_capture": true,
            "location.use_default_location": true,
            "location.default_location_name": place,
            "location.weather_provider": "open_meteo"
        }))
        .unwrap(),
    )
    .unwrap();
}

fn run(fixture: &Fixture, args: &[&str]) -> Value {
    let mut command = if let Some(executable) = std::env::var_os("CAP_ACCEPTANCE_EXECUTABLE") {
        let mut command = std::process::Command::new(executable);
        command
            .env_clear()
            .envs(fixture.isolated_environment())
            .current_dir(fixture.root.path());
        command
    } else {
        fixture.command()
    };
    let output = command.arg("--json").args(args).output().unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn missing_context_preserves_unicode_entry_and_gives_the_exact_repair_command() {
    let fixture = Fixture::new();
    configure(&fixture, "Synthetic Harbor");
    let before = fixture.row_count("entries").unwrap();
    let value = run(
        &fixture,
        &[
            "--offline",
            "add",
            "--mood",
            "good",
            "--tags",
            "codex,håvard,work",
            "Synthetic context check with Håvard.",
        ],
    );
    let uuid = value["data"]["entryUuid"].as_str().unwrap();
    assert_eq!(value["data"]["saveState"], "committed");
    assert_eq!(fixture.row_count("entries").unwrap(), before + 1);
    assert!(value["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning.as_str().is_some_and(|warning| warning
            == format!("Entry is saved. Retry missing context with: cap enrich {uuid}"))));
    let readback = run(&fixture, &["show", uuid]);
    assert_eq!(
        readback["data"]["text"],
        "Synthetic context check with Håvard."
    );
    assert_eq!(readback["data"]["mood"], "good");
    assert!(readback["data"]["tags"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tag| tag["name"] == "håvard"));
}

#[test]
#[ignore = "live Nominatim/Open-Meteo check; only writes disposable synthetic journals"]
fn live_configured_location_capture_and_enrich() {
    let fixture = Fixture::new();
    let place = std::env::var("CAP_TEST_LOCATION").unwrap_or_else(|_| "Bergen, Norway".to_string());
    configure(&fixture, &place);
    let before = fixture.row_count("entries").unwrap();
    let value = run(
        &fixture,
        &[
            "add",
            "--mood",
            "good",
            "--tags",
            "codex,håvard,work",
            "Synthetic context check with Håvard.",
        ],
    );
    assert_eq!(value["data"]["location"]["status"], "captured", "{value}");
    assert_eq!(value["data"]["weather"]["status"], "captured", "{value}");
    assert_eq!(value["warnings"], json!([]), "{value}");
    let uuid = value["data"]["entryUuid"].as_str().unwrap();
    let readback = run(&fixture, &["show", uuid]);
    assert_eq!(readback["data"]["location"]["placeName"], place);
    assert_eq!(
        readback["data"]["text"],
        "Synthetic context check with Håvard."
    );
    let missing = run(
        &fixture,
        &["--no-context", "add", "Synthetic enrichment check."],
    );
    let missing_uuid = missing["data"]["entryUuid"].as_str().unwrap();
    let enriched = run(&fixture, &["enrich", missing_uuid]);
    assert_eq!(enriched["data"]["locationStatus"], "captured", "{enriched}");
    assert!(
        matches!(
            enriched["data"]["weatherStatus"].as_str(),
            Some("captured" | "cached")
        ),
        "{enriched}"
    );
    assert_eq!(enriched["warnings"], json!([]), "{enriched}");
    assert_eq!(fixture.row_count("entries").unwrap(), before + 2);
}
