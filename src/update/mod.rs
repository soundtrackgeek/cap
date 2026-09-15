//! Repository updates are independent of Capsule's journal and desktop app.
mod install;
mod release;
mod source;
#[cfg(feature = "test-hooks")]
mod test_fixture;

use crate::{
    app::{AppError, CommandOutput},
    cli::{Cli, Command, GlobalOptions},
    preferences,
};
use anyhow::{Context, Result};
use cap_effects::{ColorMode, TerminalCapabilities};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, IsTerminal, Write},
    path::Path,
    process::{Command as Process, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

pub const REPOSITORY: &str = "https://github.com/soundtrackgeek/cap.git";
pub const BRANCH: &str = "master";
const CHECK_INTERVAL: u64 = 24 * 60 * 60;
const FAILURE_INTERVAL: u64 = 60 * 60;
const CACHE_NAME: &str = "update-check.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Available {
    version: String,
    revision: String,
    core_revision: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    checked_at: u64,
    failed: bool,
    latest: Option<Available>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn newer(candidate: &str, current: &str) -> bool {
    match (Version::parse(candidate), Version::parse(current)) {
        (Ok(candidate), Ok(current)) => candidate.cmp_precedence(&current).is_gt(),
        _ => false,
    }
}

fn read_cache(home: &Path) -> Cache {
    fs::read(home.join(CACHE_NAME))
        .ok()
        .filter(|bytes| bytes.len() <= 16 * 1024)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn due(cache: &Cache, now: u64) -> bool {
    cache.checked_at == 0
        || now < cache.checked_at
        || now - cache.checked_at
            >= if cache.failed {
                FAILURE_INTERVAL
            } else {
                CHECK_INTERVAL
            }
}

fn record_check(home: &Path, result: &Result<Available>) {
    // Failed network checks are quiet and rate-limited; never announce stale results.
    let cache = Cache {
        checked_at: now(),
        failed: result.is_err(),
        latest: result.as_ref().ok().cloned(),
    };
    if let Ok(bytes) = serde_json::to_vec(&cache) {
        let _ = atomic_write(&home.join(CACHE_NAME), &bytes);
    }
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("File has no parent directory")?;
    fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(bytes)?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn lock(path: &Path) -> Result<File> {
    fs::create_dir_all(path.parent().context("Lock has no parent")?)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    file.try_lock()
        .context("Another cap update is already running. Try again when it finishes.")?;
    Ok(file)
}

pub fn refresh_background() {
    let Ok(home) = preferences::config_home_from_env() else {
        return;
    };
    let Ok(_lock) = lock(&home.join("update-check.lock")) else {
        return;
    };
    if due(&read_cache(&home), now()) {
        record_check(&home, &source::latest());
    }
}

fn eligible(cli: &Cli, tty: bool, ci: bool) -> bool {
    !cli.global.json
        && !cli.global.quiet
        && !cli.global.offline
        && !cli.global.dry_run
        && tty
        && !ci
        && matches!(
            cli.command,
            Some(
                Command::Add(_)
                    | Command::Write(_)
                    | Command::Show(_)
                    | Command::Today(_)
                    | Command::Recent(_)
                    | Command::Search(_)
                    | Command::Tags(_)
                    | Command::Moods(_)
                    | Command::Status { .. }
                    | Command::Recover { .. }
                    | Command::Enrich { .. }
                    | Command::Recall { .. }
                    | Command::OnThisDay(_)
                    | Command::Calendar { .. }
                    | Command::Stats { .. }
                    | Command::Garden { .. }
            )
        )
}

fn notice(version: &str, color: ColorMode) -> String {
    let text =
        format!("A little upgrade is ready: cap {version}. Run `cap update` when you're ready.");
    if color == ColorMode::Plain {
        text
    } else {
        format!("\x1b[95m{text}\x1b[0m")
    }
}

/// Called only after the normal successful output. No network waits on this path.
pub fn after_success(cli: &Cli) {
    let caps = TerminalCapabilities::detect();
    if !eligible(
        cli,
        io::stdout().is_terminal() && io::stderr().is_terminal(),
        caps.ci,
    ) {
        return;
    }
    let Ok(home) = preferences::config_home_from_env() else {
        return;
    };
    let cache = read_cache(&home);
    if !cache.failed && !due(&cache, now()) {
        if let Some(latest) = &cache.latest {
            if newer(&latest.version, env!("CARGO_PKG_VERSION")) {
                let color = preferences::resolve_from_store(&cli.global, &caps)
                    .map(|presentation| presentation.output.color())
                    .unwrap_or(ColorMode::Plain);
                let _ = writeln!(io::stderr().lock(), "\n{}", notice(&latest.version, color));
            }
        }
    }
    if due(&cache, now()) {
        if let Ok(exe) = std::env::current_exe() {
            let mut child = Process::new(exe);
            child
                .arg("__check-update")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                child.creation_flags(0x08000000); // CREATE_NO_WINDOW
            }
            // The child owns the network timeout and cache lock, and can outlive a quick capture.
            if let Ok(mut child) = child.spawn() {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
        }
    }
}

pub fn run(check_only: bool, global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    if global.offline {
        return Err(AppError::new("OFFLINE", "Updating needs an internet connection. Run `cap update` without --offline when you're online.", 2));
    }
    run_update(check_only, global).map_err(|error| {
        AppError::new(
            "UPDATE_FAILED",
            format!("Could not update cap: {error:#}"),
            1,
        )
    })
}

fn progress(global: &GlobalOptions, text: &str) {
    if !global.json && !global.quiet {
        let _ = writeln!(io::stderr().lock(), "{text}");
    }
}

fn run_update(check_only: bool, global: &GlobalOptions) -> Result<CommandOutput> {
    progress(global, "Checking for a new cap version...");
    let home = preferences::config_home_from_env()?;
    let latest = source::latest();
    record_check(&home, &latest);
    let mut latest = latest?;
    let current = current_version();
    let available = newer(&latest.version, &current);
    let mut data = json!({
        "currentVersion": current, "latestVersion": latest.version,
        "updateAvailable": available, "updated": false, "repository": REPOSITORY,
        "branch": BRANCH, "sourceRevision": latest.revision,
    });
    if !available {
        return Ok(CommandOutput::new(
            data,
            format!("You're up to date! cap {} is installed.", current),
        ));
    }
    if check_only {
        return Ok(CommandOutput::new(
            data,
            notice(&latest.version, ColorMode::Plain),
        ));
    }
    let executable = std::env::current_exe()?.canonicalize()?;
    let _lock = lock(
        &executable
            .parent()
            .context("Executable has no directory")?
            .join(".cap-update.lock"),
    )?;
    let plan = install::Installation::inspect(&executable)?;
    progress(global, &format!("Getting cap {}...", latest.version));
    let staged = match release::download(&mut latest)? {
        Some(staged) => staged,
        None => {
            progress(global, "No packaged download is available. Building from source; the first build can take a few minutes...");
            source::build(&latest, &home, global)?
        }
    };
    let candidate = staged
        .path()
        .join("bin")
        .join(format!("cap{}", std::env::consts::EXE_SUFFIX));
    source::verify_binary(&candidate, &latest)?;
    progress(global, "Installing the new version...");
    plan.replace(&candidate, &latest, |path| self_replace::self_replace(path))?;
    data["updated"] = json!(true);
    data["sourceRevision"] = json!(latest.revision);
    data["installedVersion"] = json!(latest.version);
    data["executable"] = json!(executable);
    let mut output = CommandOutput::new(
        data,
        format!(
            "All set! cap {} -> {}. Enjoy the update!",
            current, latest.version
        ),
    );
    output.committed = true;
    Ok(output)
}

fn current_version() -> String {
    #[cfg(feature = "test-hooks")]
    if let Some(fixture) = test_fixture::load() {
        return fixture.current_version;
    }
    env!("CARGO_PKG_VERSION").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn semantic_versions_handle_development_numbers_stable_and_no_downgrades() {
        assert!(newer("0.2.0-dev.32", "0.2.0-dev.9"));
        assert!(newer("0.2.0", "0.2.0-dev.99"));
        for version in [
            "0.2.0-dev.9",
            "0.2.0-dev.32",
            "0.2.0-dev.32+build",
            "garbage\x1b[0m",
        ] {
            assert!(!newer(version, "0.2.0-dev.32"));
        }
    }

    #[test]
    fn notices_are_only_for_successful_interactive_journal_commands() {
        for args in [vec!["cap", "add", "hello"], vec!["cap", "recent"]] {
            let cli = Cli::parse_from(args);
            assert!(eligible(&cli, true, false));
            assert!(!eligible(&cli, false, false));
            assert!(!eligible(&cli, true, true));
        }
        for args in [
            vec!["cap"],
            vec!["cap", "--json", "recent"],
            vec!["cap", "--quiet", "recent"],
            vec!["cap", "--offline", "recent"],
            vec!["cap", "--dry-run", "add", "hello"],
            vec!["cap", "completions", "powershell"],
            vec!["cap", "doctor"],
            vec!["cap", "context"],
            vec!["cap", "fx"],
            vec!["cap", "update"],
            vec!["cap", "__check-update"],
        ] {
            assert!(!eligible(&Cli::parse_from(args), true, false));
        }
        let plain = notice("1.0.0", ColorMode::Plain);
        assert!(!plain.contains('\x1b'));
        assert!(notice("1.0.0", ColorMode::TrueColor).contains("\x1b[95m"));
    }

    #[test]
    fn cache_recovers_from_corruption_and_rate_limits_successes_and_failures() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(CACHE_NAME), "broken").unwrap();
        assert!(due(&read_cache(dir.path()), 100));
        record_check(dir.path(), &Err(anyhow::anyhow!("offline")));
        let cache = read_cache(dir.path());
        assert!(!due(&cache, cache.checked_at + FAILURE_INTERVAL - 1));
        assert!(due(&cache, cache.checked_at + FAILURE_INTERVAL));
        assert!(cache.latest.is_none());
        let cache = Cache {
            checked_at: 100,
            failed: false,
            latest: None,
        };
        assert!(!due(&cache, 100 + CHECK_INTERVAL - 1));
        assert!(due(&cache, 99));
    }
}
