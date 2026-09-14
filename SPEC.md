# cap — a little ceremony for everyday memories

Status: implementation in progress. The command grammar, shared-core extraction,
effects, themes, preferences, completions and journal reads are integrated. Capture and the
remaining R1/R2 features are under review; see [implementation status](docs/implementation-status.md).
Specification version: 0.1.0 · 2026-09-14

## 1. Product promise

Type `cap Had a lovely walk by the water` and the memory becomes a real Capsule
entry, in the same active database, with Capsule's configured location and weather
capture. Capsule can be closed. When it is open, both programs can use the journal.
The terminal gives the moment a small, satisfying celebration.

The personality is a pocket journal crossed with a tiny retro computer: luminous
type, tactile save animations, weather in the margins, and pleasant discoveries
from your own history. The core interaction stays short enough to use ten times
a day. Larger visual experiences are commands you deliberately open.

The first release is a Windows-native `cap.exe`, usable from PowerShell and any
working directory. Rust is the proposed implementation language: it is installed
on the development machine, matches Capsule's backend, and supports a single
binary without requiring Python or Capsule to run. Windows Terminal is the main
visual target; plain output remains fully usable elsewhere.

## 2. Evidence and boundaries

These observations were checked against source on 2026-09-14, not against the
contents of the user's live journal. Database size, installed app behavior, network
latency, and visual quality remain implementation-time verification work.

| Source | Observed behavior | Design consequence |
| --- | --- | --- |
| `C:\_code\capsule_tauri`, commit `5db5502388d5e62e48d75c2e5bbcfab41984118a`, app 0.37.0 | Tauri commands wrap Rust backend functions. | Extract a headless shared library; do not automate the GUI or require a local server. |
| `src-tauri/src/entries.rs`, `create_entry` / `create_entry_inner` | Verified backup; text normalization; `entry_…` identity; tags; optional continuation; FTS refresh; ID resequencing; commit; then location capture. | Reuse the entire save contract, not a bare `INSERT INTO entries`. |
| `entries.rs`, `list_entries_for_database`, `get_entry_for_database`; `search.rs` | Some nominal reads first run `ensure_entry_ids_for_database`, which may write and back up. | Introduce truly read-only query entry points for CLI browsing and diagnostics. |
| `src-tauri/src/db.rs` | Shared path settings, environment overrides, read/write connections without the create flag, 15-second busy timeout, foreign keys, WAL, `synchronous=NORMAL`. | Preserve path authority and benchmark concurrency. Do not create a replacement journal when discovery fails. |
| `src-tauri/src/location.rs` | Flat `location.*` config keys, fixed-place or IP capture, Open-Meteo or MET Norway, geocoding cache, per-request 10-second HTTP timeout. | Share provider/config logic. Add an overall context deadline and explicit outcomes. |
| `location.rs`, `auto_capture_location` | Context is attached after the entry commits; capture errors are logged, and several unavailable paths return `false`/`None`. | Return a committed receipt before optional enrichment can make the result ambiguous. |
| `src-tauri/src/phase6.rs` | Reads existing XP events and supports quest claims; the inspected entry-create path has no automatic XP award. | Do not invent “+25 XP” for a save or write a competing gamification system. |
| `src-tauri/src/stats.rs` | Calendar, streak/activity, weather, and Wrapped models already exist; capture source analytics derive mobile status from location source. | Reuse compatible definitions; do not overwrite location `source` with `cli`. |
| `C:\_code\deepseek_test\color-cli`, commit `c813f12f8578283b68fa124944c0078f15ccdec3`, version 1.0.0 | Python standard-library renderer: eight palettes, five gradient modes, smooth fade, stagger, shimmer, VT setup and cursor restoration. | Port the actual palette data and rendering math into a reusable Rust effects crate with provenance and reference tests. |
| `colorcli.py`, `print_plain`, `build_rows`, `animate` | “Plain” still emits ANSI; redirected output keeps color; wrapping counts Python characters; reveal time grows with text length. | cap needs stronger output-mode, Unicode-width, and bounded-duration behavior. |

Scope of “same as Capsule”: text, Markdown/plain format, timestamps, title,
summary, mood, tags, star/pin defaults, continuation, search visibility, configured
location/weather, backups, and compatible sync data. CLI parity does not mean
rebuilding the desktop's images, cloud AI, sync UI, database restore, or every
management screen. Existing sync can later pick up these entries; saving with cap
does not itself launch cloud sync.

## 3. The everyday experience

### Quick capture

```powershell
cap Had a lovely walk by the water
cap 'Finally fixed that bug. Time for coffee!'
cap add --mood content --tag life --tag outdoors -- 'A quiet evening outside.'
Get-Content -Raw .\today.md | cap
cap add --file .\today.md
```

Illustrative final receipt, with fictional data:

```text
  ◇ CAPSULE SEALED                              18:42
  Had a lovely walk by the water

  Bergen · Light rain · 12°C          7 words
  entry_k8m2r4a1                      Saved to Capsule
```

Before commit, show a small working indicator only if the operation takes longer
than 150 ms. Immediately after commit, show a legible `Saved` line with the stable
entry UUID. The capsule outline closes and its heading gets color-cli's aurora
fade and one shimmer. Context updates a separate status line while it is resolved.
The save statement itself never disappears into an animation.

The body preview is static, at most two wrapped lines; a long entry never makes
the animation longer. No full-screen clear, startup banner, question, sound, or
random quote on the quick path. The final receipt remains in scrollback.

If weather fails, the ending is `Saved · weather unavailable`. If capture was
disabled in Capsule, it is `Saved · location capture disabled`. Neither becomes
an entry-save failure. Full text is available through `cap show <uuid>`.

### A writing session

`cap` with an interactive input/output terminal opens the compact writer, as does
`cap write`. A thin ambient border uses the chosen theme; the writing surface
stays readable and still. Metadata sits in a small footer.

- Enter inserts a newline. Ctrl+S saves; Ctrl+C exits while keeping a nonempty
  draft. Support multiline paste and Unicode without interpreting pasted keys.
- Display word count and an optional target. Read Capsule's local word-target
  preferences; apply its Gauntlet setting in the interactive writer. Quick capture
  intentionally remains available for short notes and documents that distinction.
- Persist the local draft after 500 ms of inactivity and on normal exit. Show a
  recoverable draft choice on the next writer launch; never quietly publish it.
- `cap write --editor` uses a configured editor executable and argument array,
  then saves the resulting nonempty file only after a successful editor exit.
  A missing editor, blank file, or failed exit leaves the draft recoverable.
- Freeze the destination database for a draft and show it in writer status. If
  Capsule's active path changes, do not silently redirect the draft to another DB.

### Revisiting memories

`cap today` is a compact day card. `cap recall` unseals one random visible entry
with a short reveal. `cap on-this-day` finds past years' entries for today's month
and day. `cap calendar` turns writing activity into a constellation-like heatmap.
These are reads: they never save sample entries, add XP, or repair the journal.

## 4. Command contract

Commands below are proposed release behavior, not commands currently installed.

| Form | Behavior | Release |
| --- | --- | --- |
| `cap <text...>` | Create one entry using Capsule defaults. | R1 |
| `cap add [options] -- <text...>` | Explicit, unambiguous create. | R1 |
| `cap add --file PATH` / `cap add --stdin` | One entry from UTF-8 text; `--file -` also means stdin. | R1 |
| `cap write [--editor]` / interactive `cap` | Draft-backed writing session. | R2 |
| `cap show <uuid-or-number>` | Read exact entry; return both current display number and stable UUID. | R1 |
| `cap today` / `cap recent [--limit N --offset N]` | Visible entries, oldest-first for today and newest-first for recent. | R1 |
| `cap search '<query>' [--limit N --offset N]` | Capsule keyword/structured search, including `tag:`, `mood:`, `before:`, `after:`, `NOT tag:`. | R1 |
| `cap tags` / `cap moods` | Discover existing metadata, with bounded `--limit` and `--offset`. | R1 |
| `cap context` | Report effective capture settings and their source, without network calls. | R1 |
| `cap doctor` | Read-only setup/schema/output-capability report. | R1 |
| `cap status --capture-id ID` | Resolve a capture's durable receipt or pending/unknown status. | R1 |
| `cap recover list` / `show ID` / `retry ID` / `discard ID` | Inspect and explicitly recover or remove a cap-local pending draft. | R1 |
| `cap enrich <uuid>` | Explicitly retry missing context for that entry; never create another entry. | R1 |
| `cap theme list` / `preview NAME` / `set NAME` | Preview or save a cap-local theme. Previews use fictional content. | R1 |
| `cap fx [NAME]` | A short visual playground with synthetic data; works without Capsule installed. | R1 |
| `cap config show` / `set KEY VALUE` | Show/edit only the allowlisted cap presentation/editor settings. | R1 |
| `cap recall [--tag TAG]` | One random visible memory. | R2 |
| `cap on-this-day` | Same calendar date in earlier years; a truthful empty state. | R2 |
| `cap calendar [--month YYYY-MM]` | Month activity heatmap; default current month. | R2 |
| `cap stats [--period week\|month\|year]` | Counts, words and writing streak, default current month. | R2 |
| `cap garden` | A seven-day garden grown from real writing days. | R2 |
| `cap completions powershell` | Emit an opt-in completion script. | R1 |
| `cap --help` / `cap <command> --help` / `cap --version` | Predictable discovery, including shared-core compatibility version. | R1 |

Input rules:

1. The exact first non-global-option token chooses a reserved subcommand. All
   other positional input is an entry. Reserved names appear in help. To journal
   text starting with one, use `cap -- today was wonderful` or `cap add -- 'today
   was wonderful'`. Invalid arguments to a recognized command are errors, never
   fallback journal entries. `cap doctor nonsense` must not write anything.
2. `--` ends all option parsing and treats the remainder as text. Otherwise flags
   must precede free text. Unknown leading options fail with a useful hint.
   `write-the-entry-here` remains literal text; hyphens do not become spaces.
3. The shell processes its own syntax before cap runs. Unquoted words are joined
   with one space. Quote punctuation, PowerShell `#`, `|`, `&`, `$` expressions,
   and content requiring exact spacing. File/stdin input preserves spacing and
   line breaks apart from Capsule's CRLF/CR-to-LF normalization. UTF-8 BOM is
   accepted. Invalid UTF-8 is reported, not silently replaced.
4. Exactly one content source is allowed. Text plus file/stdin is an error. No
   arguments with piped stdin captures one entry, not one per line. With no input
   and no usable interactive terminal, show usage and exit 2. `--help` never waits
   for stdin or opens a DB.
5. Reject whitespace-only input. Default maximum input is 1 MiB of UTF-8 bytes;
   stop reading at the limit and explain it. Larger import tooling is deferred.
6. Metadata flags: `--mood`, repeatable `--tag`, `--title`, `--summary`, `--star`,
   `--pin`, `--format markdown|plain`, and `--continue <uuid-or-number>`. Default
   format is Capsule's Markdown. Tags use its normalization. Hashtags or emoji in
   the text are not silently removed or converted to metadata.
7. No backdated creation flag in R1. `created_at` is captured at the save request,
   before waiting for the DB/weather; retries preserve that original value.
   Numbers are display aliases and may change after resequencing. Durable links,
   recovery, continuation storage and receipts always use UUIDs.

Global controls before the subcommand/text: `--db PATH`, `--json`, `--quiet`,
`--plain`, `--color auto|always|never`, `--motion auto|full|reduced|off`,
`--theme NAME`, `--offline`, `--no-context`, and `--dry-run` for create commands.
`--offline` forbids network; `--no-context` skips all location/weather work for
that invocation. These overrides do not edit Capsule settings.

`--dry-run` validates content, resolves paths, and inspects schema read-only. It
returns the proposed payload and effective context policy, but creates no backup,
draft, entry, cache row or network request. It cannot guarantee a future write.

## 5. Fun features with explicit limits

| Feature | Experience | Data and fallback | Budget |
| --- | --- | --- | --- |
| Capsule seal | Two half-shells close around a small diamond; heading fades through aurora and shimmers. | Runs only after a confirmed commit; static `[saved]` in plain mode. | 450–650 ms total; R1. |
| Living weather stamp | A few rain strokes, drifting snow dots, a sun glint, or a moving cloud beside the receipt. | Only persisted weather from this capture; unknown stays unknown. No fake “sunny” fallback. | One 250 ms accent inside the seal's budget; R1. |
| Color-cli palette gallery | Actual neon, aurora, ocean, sunset, fire, ice, candy and mono previews. All five source gradient modes are demonstrable. | Synthetic text; usable without a journal. | Up to 3 seconds per explicit demo; R1. |
| Retro skins | `aurora` default; `neon`, `c64`, `amber`, `paper`. C64 block borders and blue/pale type, amber phosphor glow, paper minimal ink. | Theme presets map onto shared renderer primitives. Preserve contrast and textual status labels. | No startup delay; R1. |
| Milestone glint | A single small star when a real daily/weekly writing milestone is crossed. | Derived from visible entries from either app. Once-per-milestone state is cap-local; no XP writes or made-up “level ups”. | Replaces, rather than adds to, the seal effect; R2. |
| Streak garden | Seven little plants, one per local day, with growth based on that day's words. A missed day is bare ground, never a dead plant or scolding message. | Data-derived view; seed 1–49 words, sprout 50–199, leaf 200–499, bloom 500+. Text legend always present. | 400 ms grow-in only when explicitly opened; R2. |
| Time-machine unseal | `recall` opens a capsule outline, then reveals the date before the entry. | Visible entries only; do not repeat the last recall when another candidate exists. Static full text follows. | At most 700 ms; R2. |
| Calendar constellation | Active days become stars of different brightness; a selected day's count/words appear below. R2 starts as a static month grid. | Same local-date and visibility rules as stats; list fallback under narrow widths. | At most 300 ms reveal; R2. |
| Ambient writer | A quiet gradient rail, optional slow phosphor pulse and a word-target line. | Does not animate the text/caret. Pauses motion while typing and on unfocused/unsupported terminals. | 15 fps ambient cap, reduced/off disables it; R2. |

Weather labels, word counts and success must remain understandable without color,
animation or emoji. Themes do not change stored content. The default is fun, not
a slot machine: no random rewards, streak punishment, daily nags or surprise sounds.

Later candidates, outside R1/R2: opt-in sound packs, a short weekly Wrapped reel,
prompt roulette backed by Capsule's prompt library, terminal image previews, and
additional seasonal effects. They need their own scoped plan before implementation.

### color-cli reuse

Create `crates/cap-effects` in this repository. Port `PALETTES`, `lerp_rgb`,
`palette_color`, gradient placement, smoothstep intensity and shimmer falloff from
the inspected Python revision. Preserve attribution in
`docs/provenance/color-cli.md`. No license file was listed in the inspected source;
record that state and resolve distribution licensing during release packaging,
rather than inventing a license notice.

Capture small golden color/frame fixtures from the original functions. Match
palette endpoints and interpolation within one RGB channel value, documenting
Python/Rust rounding differences. The original Python project stays independently
usable. cap does not shell out to `python colorcli.py` or depend on its checkout.

Improve layout using grapheme boundaries and terminal cell width, sanitize terminal
control sequences in user/provider text at rendering time, and restore cursor,
color and raw mode through normal exit, error and interruption. Never store ANSI
styling in the journal. Full-screen alternate-buffer use is limited to the writer
and explicit effects demos, with guaranteed restoration.

## 6. Shared data and architecture

```text
 PowerShell / Windows Terminal
             |
          cap.exe ---------------- cap-effects (pure rendering)
             |
        capsule-core <------------ Capsule Tauri command adapters
             |
   shared path/settings resolution
             |
   active Capsule SQLite DB + backups
             |
   existing location/weather providers
```

Extract `crates/capsule-core` inside `C:\_code\capsule_tauri`. Begin with the
dependency-closed set of `db`, `backup`, `entries`, `location`, and relevant models.
Keep desktop shell actions such as opening Explorer in Tauri adapters. Preserve
existing internal module names through re-exports where that keeps the extraction
small. Add read-only search and stats surfaces as their work packages require.

The shared library has no Tauri, webview, tray, window, single-instance or global
shortcut dependency. Both clients call the same mutation implementation. cap uses
a Git dependency pinned to a reviewed capsule-core revision, with `Cargo.lock`
committed. Worktrees may use local, uncommitted Cargo patch configuration while
developing; release builds must work from a clean checkout without sibling folders.

Do not maintain two copies of entry SQL, import Rust source through absolute paths,
or link the whole Tauri application into the CLI. Do not create an HTTP daemon for
local capture. If extraction reveals unexpected coupling, the orchestrator revises
the boundary before parallel feature work; schema safety is not traded for speed.

### Database and settings authority

The invocation-level `--db` override comes first. Otherwise use the shared Capsule
resolver: `CAPSULE_DB_PATH`, then saved `databasePath`, then the existing platform
defaults/fallbacks. At the inspected revision Windows checks the legacy
`C:\Users\jtill\.capsule\capsule.db` default before the current profile's
`.capsule\capsule.db`, then `CAPSULE_HOME\capsule.db`. Keep that behavior in the
shared resolver until Capsule changes it deliberately. Do not copy that hardcoded
path into cap itself or silently select a different candidate when an explicit
path is missing.

Read local settings via `CAPSULE_PATH_SETTINGS_PATH` or
`%APPDATA%\Capsule\path_settings.json` on Windows, using the shared fallback rules.
Read location config from the shared resolver's `CAPSULE_CONFIG_PATH` candidate
then `config.json` beside the active database. Existing code accepts the first
valid object; diagnostics must expose unreadable/malformed candidates rather than
quietly claiming saved preferences were honored. For cap, a malformed explicit
settings/config override fails preflight; if an inherited config is unusable,
capture text with context skipped and an explicit warning, never guess permission
to send location requests.

Use Capsule's backup path and retention resolution, including `CAPSULE_BACKUP_DIR`.
Do not dump full path-settings/config objects: they can contain unrelated tokens.
Diagnostics expose a safe allowlist and redact secrets.

cap-only state lives in `%LOCALAPPDATA%\Capsule\cap\` (override:
`CAP_CONFIG_HOME`). It contains presentation/editor config, pending captures,
writer drafts, recovery receipts, and narrowly scoped cache/milestone state.
There is no second authoritative journal. A DB override is never persisted by a
capture command. Cache/recovery keys include the canonical DB identity.

### Save and recovery state machine

1. Parse and validate input, snapshot the requested save time, freeze the resolved
   database/config paths, inspect schema, and reject unsupported required columns
   before any journal mutation. Do not assume an unverified `user_version` is a
   complete schema contract. Record supported schema capabilities in fixtures.
2. Reserve a Capsule-compatible entry UUID and a separate cap capture ID. Write
   an atomic pending record containing the request and intended DB identity before
   mutation. Failure to persist it stops the capture. Dry runs skip this step.
3. Create and verify the existing SQLite backup and manifest. Backups use SQLite's
   backup API, never a raw copy of a live DB/WAL pair. A failed backup blocks the
   entry mutation. Make backup filename reservation and retention safe under
   concurrent clients; do not allow one process to prune another's active backup.
4. Acquire a bounded write transaction before allocating numeric IDs or checking
   identity collisions. Insert the entry, tags, continuation and FTS updates, and
   retain Capsule's resequencing/reference rules atomically. No network or effects
   inside a write transaction. Shared known schema repairs remain backup guarded;
   unsupported schemas fail before repair/write.
5. Commit. Record the result using the reserved UUID, not a fallible post-commit
   full-detail query. Mark the pending record committed and render the immediate
   human save acknowledgement. If local receipt writing fails now, the entry is
   still saved; retain/reconcile the original pending record and report a warning.
6. Resolve and attach optional context outside the entry transaction under the
   deadline below. Record separate location/weather outcomes and return the final
   receipt. Release locks before rendering or computing optional statistics.

Reserve stable identity by extending the shared core request, not by assuming
entry text is unique. Retry checks the same UUID in the same DB and compares the
stored normalized request fields. A matching committed entry returns the existing
receipt; conflicting content is an explicit error. Two deliberately repeated
normal `cap same words` invocations still create two entries. Expose
`cap add --capture-id ID` for caller-controlled retries; validate its format and
bind it to one database/request. Pending records use per-record locking and atomic
replacement so parallel invocations cannot overwrite each other.

`cap recover retry ID` reconciles identity before writing and refuses a changed or
missing destination DB. `recover discard ID` deletes only local recovery data and
requires an interactive confirmation or explicit `--yes`; it never deletes a
journal entry. Confirmed receipts are retained for 30 days without entry bodies;
unresolved drafts are kept until saved/discarded. After receipt expiry, a reused
caller capture ID must be rejected as unverifiable, not silently create a duplicate;
keep a compact ID-to-UUID binding for explicitly supplied IDs. Access to these
files follows the current Windows user's permissions; they are not an encryption
feature and never belong in Git or diagnostic uploads.

Cancellation before commit retains the draft and exits 130. Cancellation after
commit stops optional context/effects and reports saved, exit 0. If interruption
or an I/O error makes the commit outcome unknown, return exit 6 with capture ID and
`cap status --capture-id ...`; recovery must query before retrying. A display or
weather failure must never trigger a second insert. A broken output pipe may make
the receipt undeliverable, but must not alter or replay a completed save.

SQLite WAL supports concurrent readers but still serializes writers. Use
`BEGIN IMMEDIATE` for the read-then-write creation transaction, bounded retry on
known contention, and a 15-second total DB-lock wait budget rather than multiplying
the current timeout across retries. These choices follow SQLite's documented
[isolation](https://www.sqlite.org/isolation.html) and
[transaction behavior](https://www.sqlite.org/lang_transaction.html).
Backups, restore/replacement races and an older running desktop binary require
separate integration tests; WAL alone is not the completion criterion. Known
active restore/replacement must be reported as busy, and the core must revalidate
the target file identity before mutation to avoid writing an obsolete handle.

Keep the inspected `synchronous=NORMAL` policy initially, and document its actual
power-loss guarantee; do not claim that a successful commit under that policy is
immune to sudden power failure. Process-crash recovery and power-loss durability
are different tests. See [SQLite WAL](https://www.sqlite.org/wal.html).

### Location and weather

Honor `location.auto_capture`, `location.use_default_location`,
`location.default_location_name`, `location.auto_capture_method`,
`location.weather_provider`, and `location.geocoding_cache_hours`.
Fixed location wins when enabled; failure to geocode it must not fall through to
an unrelated IP location. Preserve `source=default|ip` and the provider's persisted
weather fields, units and fetched time. Do not replace them with `source=cli`.

Use the existing providers and response interpretation. The new context interface
adds injected HTTP/clock/cache dependencies and structured outcomes. It must avoid
the existing possibility of a second weather attempt inside `attach_location`
when the first call returned no data. All provider work shares an 8-second overall
deadline with cancellation, including IP, geocode, fallback and weather requests.
Tests must show that individual 10-second client timeouts cannot exceed that budget.
Ordinary entry capture ends after this deadline; it does not spawn an invisible
background worker that dies when cap exits.

Location and weather statuses are independently `captured`, `cached`, `disabled`,
`unavailable`, or `skipped`. A configured place label can be shown as
`Configured place: Bergen` when offline geocoding is unavailable, but is not
reported as a persisted location unless coordinates were actually attached.

The shared geocode cache can supply coordinates/place data when applicable. An
optional cap-local weather cache may reuse an observation for at most 15 minutes,
keyed by exact resolved place coordinates and provider, with its original fetched
timestamp and a visible `cached` label. Do not use yesterday's weather or another
location's observation to make a receipt look complete. `--offline` uses only
valid local context; an empty cache means save without weather. `--no-context`
performs no capture/cache work.

`cap enrich <uuid>` fills only missing fields, after a fresh backup, never
overwrites manual/existing context, and uses entry-time weather where the provider
supports it. MET Norway currently follows the source's current-forecast path;
therefore old entries must remain weather-unavailable rather than be stamped with
today's forecast. Enrichment retry uses a 3-hour current-weather cutoff consistent
with the existing Open-Meteo selection, then requests historical data or declines.
Journal text, tags and titles are never sent to a weather/geocoding provider.

### Desktop coexistence

R1 requires proof that a CLI entry appears in Capsule's entry list, detail and
search after its supported refresh action, with location/weather and tags intact.
Restarting Capsule must also show it. This works independently of a new automatic
refresh feature, and the installed desktop version must be recorded in evidence.

R2 adds a lightweight external-change signal on focus and while a relevant view
is visible. Prefer `PRAGMA data_version` on a retained read-only connection; values
must be compared on the same connection. Reopen it when the configured DB changes
and track DB/WAL replacement. Debounce refresh, preserve selection/scroll/filters,
and never reset a composer draft. Current source inspection did not establish
automatic external-write refresh; treat that as planned work, not existing proof.

## 7. Output, accessibility and performance

`--json` emits one versioned JSON object to stdout, including failures. No ANSI,
banner, progress, editor, question or animation is allowed on either stream in
JSON mode. Optional textual diagnostics go to stderr without secrets. Unknown
metadata is `null`, not invented. Read commands return their requested content;
capture receipts omit the full entry text by default.

Illustrative committed receipt:

```json
{
  "schemaVersion": 1,
  "ok": true,
  "command": "add",
  "data": {
    "captureId": "cap_01_example",
    "saveState": "committed",
    "entryUuid": "entry_k8m2r4a1",
    "entryNumber": 1248,
    "createdAt": "2026-09-14 18:42",
    "wordCount": 7,
    "location": {"status": "captured", "name": "Bergen", "source": "default"},
    "weather": {"status": "captured", "condition": "Light rain", "tempC": 12.0, "fetchedAt": "2026-09-14 18:42"}
  },
  "warnings": [],
  "error": null
}
```

Error shape uses the same envelope with `ok:false`, `data:null` (or the known
capture ID/save state when recovery is needed), `warnings:[]`, and
`error:{"code":"DB_BUSY","message":"...","retryable":true}`. Stable codes,
schema and success/error fixtures for every command family are frozen before
parallel implementation. Reads paginate at 20 by default, maximum 200, with
`total`, `limit`, `offset` and `hasMore`; hidden entries are excluded unless
`--include-hidden` is explicitly supplied to a supported read command. `show`
also requires that flag for a hidden entry, even when its UUID is known.

| Exit | Meaning |
| --- | --- |
| 0 | Successful read/action or confirmed entry save, including missing weather. |
| 1 | Unexpected failure known to occur before commit. |
| 2 | Usage, input or validation error. |
| 3 | Missing DB/config, unsupported schema, unreadable DB or unusable setup. |
| 4 | DB contention/replacement timeout with no commit. |
| 5 | Backup/recovery-storage failure before commit. |
| 6 | Commit result uncertain; status/reconciliation required before retry. |
| 130 | User cancelled before commit. |

Precedence: `--json` > `--quiet` > `--plain` > explicit color/motion flags > cap
settings > automatic capabilities. `--quiet` prints only the UUID on successful
create; errors stay visible. `--plain` means no ANSI and no motion, unlike the
source color-cli flag. Non-TTY, `TERM=dumb`, and CI default to plain static output.
Nonempty [`NO_COLOR`](https://no-color.org/) disables automatic color; explicit
`--color always` may override it for human output, but never JSON/quiet/plain.
`--motion reduced` is static colored output; `off` also disables all temporal
effects. Screen-reader guidance uses plain mode. No rapid flashing or BEL output.

The renderer degrades truecolor → 256 colors → 16 colors → plain and uses ASCII
icons when width/encoding is uncertain. It handles a 40-column terminal without
horizontal overflow and never redraws beyond its own allocated receipt lines.
Resize mid-effect finishes in a static layout. Animation uses a monotonic clock,
skips late frames, and is capped at 30 fps except explicit `fx` demos (60 fps max).
Cap quick-save decorative time at 650 ms independent of entry length.

Targets, not measured promises:

- Cold `--help` under 100 ms and small offline capture under 500 ms excluding the
  existing verified backup cost and optional decoration, on the target Windows PC.
- Show working feedback by 150 ms and a saved UUID immediately after commit.
- Bound context to 8 seconds and DB lock waiting to 15 seconds. Backup duration
  is measured separately and remains visible; never silently skip backup to meet
  a headline latency target.
- Benchmark p50/p95 for cold/warm capture, backup, insert/resequence, enrichment and
  rendering separately on synthetic 1k/10k/100k-entry databases. Report hardware,
  DB size and filesystem. A real-journal benchmark requires a reviewed backup or
  explicit live-write authorization at execution time.

## 8. Release acceptance

R1 is the useful, delightful vertical slice: discovery, safe capture/recovery,
shared context, small read commands, real color-cli effects, themes, JSON/plain,
diagnostics, installation, and verified desktop refresh interoperability.
R2 completes the richer experience: draft writer, time machine, garden, calendar,
stats/milestones and automatic desktop external-change refresh.

Both releases must satisfy their mapped gates in PLAN.md. No release is complete
based on unit tests alone: installed invocation from another directory and real
Windows terminal interaction are required. Capsule integration uses disposable
fixture/copy databases; production journal entries are not test fixtures.

Deferred management surface: editing/deleting journal entries, attachment writes,
AI, cloud sync execution, backup restore, raw SQL writes, and schema migration
commands. `doctor --json` provides schema/capability diagnostics as the low-level
read escape hatch. Arbitrary SQL and provider APIs are not required for capture.

Dependency packaging follows Cargo's documented
[Git dependency and revision pinning](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html).
Exact crate versions and minimum Rust version are selected and locked during the
foundation work, after checking the shared dependency graph.
