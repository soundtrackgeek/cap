# Context timeout recovery — 0.2.0-dev.35

Verified on Windows on 2026-09-15, using the pinned Capsule core revision
`c973d8c6c96d35e51237208a8591a0592ed8d0fb`.

## Investigation

Read-only inspection confirmed that the reported entry's full text, Unicode
tags, and mood were already committed. Its optional location row was absent.
The old transport gave a single GET the entire remaining context budget and
reduced network failures to reqwest's outer error message. A failed geocoding
request could therefore exhaust all eight seconds before weather was attempted.

The original provider outage did not recur during investigation. Both curl
and the original reqwest client successfully resolved the configured Unicode
place name. No TLS, certificate, DNS, or IPv6 defect was established, and no
transport security settings or address-family preferences were changed.

## Change

The CLI injects a transport that reserves part of the remaining budget for
one retry of a transient GET failure. When at least two seconds remain, the
first attempt uses at most three seconds or half the remaining time. The retry
uses only the time still available, including response-body reads. HTTP
429/502/503/504 responses can also retry, with at least one second between
request starts and respect for numeric/date `Retry-After` values. Excessive
or malformed delays are not retried. Cancellation is checked during backoff.

Persistent failures retain their underlying cause, elapsed time, and attempt
count, excluding the request URL. A missing-context receipt confirms that the
entry is saved and gives its exact enrichment command. The shared core still
owns provider parsing, the overall deadline, and guarded metadata persistence.
Provider outages can still leave optional context unavailable.

## Verification

- Loopback HTTP regressions: transient status, connection closure, first-request
  stall recovery, repeated timeout budget, error URL removal, permanent errors,
  Retry-After refusal, and cancellation during backoff.
- Offline executable capture: exactly one saved entry, intact Unicode text,
  mood and tag readback, and the matching `cap enrich` command.
- Live executable capture and enrichment: the configured location and weather
  were persisted to disposable synthetic journals, with no warnings or duplicate
  entries. The network check is ignored by default and can be run explicitly:

  ```powershell
  cargo test --locked --test context_capture live_configured_location_capture_and_enrich -- --ignored
  ```

  `CAP_TEST_LOCATION` can select a different place for this disposable check;
  `CAP_ACCEPTANCE_EXECUTABLE` can select an installed or packaged binary.

- `cargo fmt --all --check` and strict workspace/all-target/all-feature Clippy.
- `cargo test --workspace --all-targets --locked`: 231 passed, one live test
  ignored in the default run and passed separately.
- Windows installer lifecycle: all 11 rollback, ownership, collision, checksum,
  PATH, running-binary, and uninstall safety cases passed.
- Capture/recovery process hooks: 16 passed; self-update process checks: three
  passed, with the actual packaged-archive check reserved for the clean ZIP.
- Fresh-shell installed capture/read smoke with installation lifecycle passed.

These checks do not establish the cause of the original external outage or
guarantee provider availability. No Capsule desktop source or installer changed.
