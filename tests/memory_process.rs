//! Root review of all memory routes through real processes and isolated state.
mod support;
use serde_json::Value;
use support::{Fixture, FixtureProfile};

#[test]
fn every_memory_route_is_read_only_and_obeys_machine_output_modes() {
    let fixture = Fixture::from_profile(FixtureProfile::full());
    let before = fixture.snapshot().unwrap();
    for command in ["recall", "on-this-day", "calendar", "stats", "garden"] {
        for mode in ["--json", "--plain", "--quiet"] {
            let output = fixture.command().args([mode, command]).output().unwrap();
            assert!(
                output.status.success(),
                "{command} {mode}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!output.stdout.contains(&0x1b));
            if mode == "--json" {
                let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(envelope["command"], command);
                assert_eq!(envelope["ok"], true);
            } else if mode == "--quiet" {
                assert!(output.stdout.is_empty());
            } else {
                assert!(!output.stdout.is_empty());
            }
        }
    }
    assert_eq!(before, fixture.snapshot().unwrap());
}
