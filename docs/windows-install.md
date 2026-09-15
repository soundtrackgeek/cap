# Windows delivery

`cap` is delivered as a native, per-user Windows executable. A packaged
archive contains `bin\cap.exe`, a SHA-256 manifest, the install and uninstall
scripts, `NOTICE.txt`, and the source provenance documents.

## Build a local archive

From a clean checkout, run:

```powershell
.\scripts\package.ps1 -OutputDirectory .\dist -ExpectedCoreRevision <reviewed-core-sha>
```

The packager refuses staged or untracked source changes by default so the
manifest's source revision describes the actual payload. `-AllowDirty` is
available only for a clearly local development archive.

The script runs `cargo build --release --locked` in an isolated delivery target,
requires the `capsule-core` Git revision in `Cargo.toml` to remain pinned, and
does not build tests/examples or use a sibling Capsule, color-cli, or Python
checkout. The archive name includes the package version and Windows platform.
`<archive>.sha256` verifies the archive itself; `checksums.sha256` inside the
archive verifies every payload file except the manifest and checksum file.
Each successful normal package also records a local provenance stamp binding the
delivery binary hash to the source/core revisions, release profile, default
feature set, and checkout state; `-SkipBuild` refuses a binary without a matching
stamp.

## Install and update

Once cap is installed, run `cap update` to install a newer version from the
repository's `master` branch, or `cap update --check` to check without installing.
Windows x64 uses the `v<version>` GitHub release ZIP and verifies the archive and
executable hashes before replacement. Development prereleases are supported.
The source-build fallback requires Cargo, Git, and native build tools; its cache
is kept in `update-build` under cap's configuration directory.

The updater replaces the executable you invoked, maintains the managed install
receipt/checksums/provenance, and preserves PATH and completion ownership. It
stages and verifies the candidate first and restores the old executable and
metadata if replacement fails. No journal is opened. Existing installations
older than 0.2.0-dev.32 need the archive installer once to gain `cap update`.

Run `install.ps1` from the extracted archive (or pass `-SourcePath` to a
release `cap.exe`):

```powershell
.\install.ps1
# Optional explicit source and destination:
.\install.ps1 -SourcePath 'D:\Builds\cap.exe' -InstallRoot "$env:LOCALAPPDATA\Programs\cap"
```

The default destination is `%LOCALAPPDATA%\Programs\cap\bin\cap.exe`; no
administrator prompt is required. The installer verifies the package
checksum when `checksums.sha256` is present, stages the executable, checks for
an in-use/running prior binary, and publishes an install receipt at
`.cap-install.json`. Re-running the script updates only a prior cap install.
An unrelated existing `cap.exe` is a collision and requires an explicit
`-Force` after review.
These scripts deliver only `cap.exe` and installer metadata; they never install
or update the Capsule desktop app or any Capsule journal, recovery, settings,
media, sync, or backup data.

Only the bin directory is added to the user PATH. Existing PATH text and entry
ordering are preserved, and a matching entry is never duplicated. The current
shell does not inherit a newly written user PATH; open a fresh PowerShell
session (or start a new Windows Terminal tab) after installation.

Completion activation is deliberately opt-in:

```powershell
.\install.ps1 -ActivateCompletions
```

Without that switch, no shell profile is written. With it, the installer adds a
managed marker and completion invocation to the selected PowerShell profile
(`-CompletionProfilePath` can point to an explicit profile). Uninstall removes
that exact managed line only when it is still intact.

## Uninstall

```powershell
.\uninstall.ps1
```

Uninstall removes only files recorded by the receipt, verifies their hashes
unless `-Force` is requested, removes one installer-owned PATH entry, and
leaves modified or unrelated files in place. It never searches for or deletes
Capsule journal, recovery, settings, media, sync, or backup data. A running
`cap.exe` is reported and left untouched until it is closed.

For installation tests, pass `-InstallRoot` below an owned temporary directory
and `-PathStatePath` to a text file containing a synthetic PATH. This keeps
real user PATH and shell profiles unchanged:

```powershell
.\install.ps1 -InstallRoot 'C:\Temp\cap test\Programs\cap' `
  -PathStatePath 'C:\Temp\cap test\user-path.txt'
.\uninstall.ps1 -InstallRoot 'C:\Temp\cap test\Programs\cap' `
  -PathStatePath 'C:\Temp\cap test\user-path.txt'
```

## Isolated smoke test

After building the release CLI and `fixture_lab` example, run the smoke script
from any directory:

```powershell
.\scripts\smoke.ps1 `
  -CapPath .\target\release\cap.exe `
  -FixtureGeneratorPath .\target\release\examples\fixture_lab.exe
```

The script creates a new synthetic fixture lab, clears inherited Capsule/cap
environment variables in every child, launches from an unrelated working
directory and uses a fresh `PowerShell -NoProfile` process for `cap --help`,
`cap --json doctor`, and the capture/read smoke. Child environment variables
are reduced to the synthetic test values plus the minimum Windows launcher
variables, and each child has a bounded timeout. It does not use the live
journal. The native Windows Terminal UI and physical-device behavior remain
separate release gates.

## Provenance and licensing boundary

The archive preserves `docs/provenance/color-cli.md` and its attribution. The
inspected upstream color-cli snapshot had no license file or explicit license
declaration; `NOTICE.txt` records that status and does not grant public-release
permission. Resolve that permission before publishing a public binary.
