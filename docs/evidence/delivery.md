# Standalone CLI delivery

2026-09-14. Built from clean cap source
`ab8e7618d3787c278f8bcca0d2f5f969e77ec7d4`, version `0.2.0-dev.28`, with reviewed
shared core `4888a2a33ab65a1833cdf5e6cbdad38baa3d4030`. Normal release/default
features; synthetic crash hooks are not enabled in the installed executable.

- Archive: `dist/cap-0.2.0-dev.28-windows-x86_64.zip`.
- Archive SHA-256: `157ba921b012e362961479fd689bf9b3b7019c633c8708a8d0511c714e9f179b`.
- Executable SHA-256: `3d632c77e5b07e82b60b2d72af7fb89505b843a44166cefc605a4ef7d9f53f03`.
- Installed: `C:\Users\jtill\AppData\Local\Programs\cap\bin\cap.exe`.
- All seven archive payload hashes verified against its manifest; dirty=false.

The extracted artifact passed a temporary per-user installation lifecycle in a
path containing spaces, with a fresh-shell synthetic capture/read/doctor smoke.
The reviewed installer then installed the real per-user CLI, recorded its receipt
and added only its bin directory to user PATH. Completion profile activation was
off. Eleven focused lifecycle cases passed, including checksum/collision refusal,
late rollback, locked receipt, profile failure, locked uninstall and PATH/data
preservation; both PowerShell 5.1 and 7 are covered by worker evidence, with 5.1
repeated on the integrated root scripts.

Independent final check: a new PowerShell process was launched from
`cap lab 58280-1789418787018476700/temp`, with its environment cleared and PATH
rebuilt from persisted machine/user values. `Get-Command cap` resolved the installed
path, `--json --version` reported dev.28/core4888a2a, and an isolated save/readback of
`Installed cap synthetic smoke` plus doctor succeeded. The installed bin occurs
exactly once in persisted user PATH. An already-open terminal may need restarting
or a PATH refresh; the check does not claim it modified existing shell environments.

Final real-setup inspection used `cap --json doctor` and `cap --json context`: the existing saved
Capsule database/settings resolved successfully, all required entry columns were
present, configuration was valid, and diagnostics contained no warnings. This was
read-only schema/path/context-policy inspection, without entry creation, provider
calls or repair. Existing default-location and weather-provider configuration was
recognized; private configuration values are not copied into this evidence file.

Only cap.exe was delivered. The installed Capsule app and production journal,
configuration, backups, media and sync state were not test destinations. Capsule's
source checkout is clean original master `5db5502`; shared-core/optional desktop
changes remain preserved at `codex/cap-core-integration` (`aae4200`) and feature
branches. Nothing was merged into Capsule master, installed as a desktop update,
tagged as a release or published as a public artifact.

Actual Windows Terminal visual review and native desktop coexistence remain
unverified because the desktop was locked. Color-cli's inspected source has no
license declaration; the development archive preserves that provenance without
inventing a license grant.
