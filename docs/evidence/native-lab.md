# Synthetic native verification lab

`cargo run --example fixture_lab --locked -- 5` creates a new persistent directory
below the OS temporary directory and prints a JSON descriptor. It uses the same
canonical synthetic SQL as automated tests. The generated `lab.json` contains
isolated DB/config/backup/state/media/sync paths and disabled automatic sync/context.
It never accepts an existing database as input. An optional count from 5 to 100000
supports larger performance fixtures.

Native/installed verification must start processes with a cleared environment,
copying only system executable-search variables and the lab's explicit environment.
Also isolate WebView data when launching a test desktop build; use a separate
Tauri application identifier so a running production Capsule is not reused by the
single-instance plugin. Do not click sync or invoke an updater in the lab.

The lab directory is intentionally retained for screenshots and readback. Cleanup
must verify the resolved directory is the exact generated lab under the OS temp
directory before removing it. Evidence should identify which executable/version
was used and which native scenarios were actually exercised.
