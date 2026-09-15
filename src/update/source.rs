use super::{Available, BRANCH, REPOSITORY};
use crate::cli::GlobalOptions;
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

const MAX_RESPONSE: u64 = 64 * 1024;

fn get(client: &reqwest::blocking::Client, url: &str) -> Result<String> {
    let mut body = String::new();
    client
        .get(url)
        .send()?
        .error_for_status()?
        .take(MAX_RESPONSE + 1)
        .read_to_string(&mut body)?;
    ensure!(
        body.len() as u64 <= MAX_RESPONSE,
        "Repository response was unexpectedly large"
    );
    Ok(body)
}

pub(super) fn latest() -> Result<Available> {
    #[cfg(feature = "test-hooks")]
    if let Some(fixture) = super::test_fixture::load() {
        return Ok(fixture.latest);
    }
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("cap/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let reference = get(
        &client,
        &format!("https://api.github.com/repos/soundtrackgeek/cap/git/ref/heads/{BRANCH}"),
    )
    .context("Couldn't reach the cap Git repository. Check your connection and try again.")?;
    let reference: serde_json::Value = serde_json::from_str(&reference)?;
    let revision = reference["object"]["sha"]
        .as_str()
        .context("Repository response has no commit")?;
    ensure!(
        valid_revision(revision),
        "Repository returned an invalid commit"
    );
    let manifest = get(
        &client,
        &format!("https://raw.githubusercontent.com/soundtrackgeek/cap/{revision}/Cargo.toml"),
    )?;
    parse_manifest(&manifest, revision)
}

fn valid_revision(revision: &str) -> bool {
    revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_manifest(text: &str, revision: &str) -> Result<Available> {
    #[derive(Deserialize)]
    struct Manifest {
        package: Package,
        dependencies: Dependencies,
    }
    #[derive(Deserialize)]
    struct Package {
        name: String,
        version: String,
    }
    #[derive(Deserialize)]
    struct Dependencies {
        #[serde(rename = "capsule-core")]
        core: Core,
    }
    #[derive(Deserialize)]
    struct Core {
        rev: String,
    }
    let manifest: Manifest =
        toml::from_str(text).context("Repository package manifest is invalid")?;
    ensure!(
        manifest.package.name == "cap",
        "Repository package is not cap"
    );
    semver::Version::parse(&manifest.package.version).context("Repository version is invalid")?;
    ensure!(
        valid_revision(&manifest.dependencies.core.rev),
        "Repository core revision is invalid"
    );
    Ok(Available {
        version: manifest.package.version,
        revision: revision.to_owned(),
        core_revision: manifest.dependencies.core.rev,
    })
}

pub(super) fn build(
    latest: &Available,
    home: &Path,
    global: &GlobalOptions,
) -> Result<tempfile::TempDir> {
    let staging = tempfile::tempdir().context("Couldn't create update staging directory")?;
    // Reuse compilation artifacts, but never pull/reset/build inside the user's checkout.
    let target = home.join("update-build");
    fs::create_dir_all(&target)?;
    let mut command = Command::new("cargo");
    command
        .args([
            "install",
            "--git",
            REPOSITORY,
            "--rev",
            &latest.revision,
            "--locked",
            "--no-track",
            "--root",
        ])
        .arg(staging.path())
        .arg("--target-dir")
        .arg(&target)
        .arg("cap")
        .current_dir(staging.path())
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_NET_GIT_FETCH_WITH_CLI", "true")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("CARGO_BUILD_TARGET")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().context("Building updates needs Rust (Cargo), Git, and the native build tools on PATH. Install them, open a new terminal, and run `cap update` again.")?;
    let mut tail = std::collections::VecDeque::new();
    for line in BufReader::new(child.stderr.take().context("Build output unavailable")?).lines() {
        let line = line?;
        if !global.json && !global.quiet {
            let _ = writeln!(
                std::io::stderr().lock(),
                "{}",
                cap_effects::sanitize_text(&line)
            );
        }
        if tail.len() == 30 {
            tail.pop_front();
        }
        tail.push_back(line);
    }
    if !child.wait()?.success() {
        bail!("The source build failed; your installed cap is unchanged. Check Rust/Git and native build tools, then try again.\n{}", tail.into_iter().collect::<Vec<_>>().join("\n"));
    }
    Ok(staging)
}

pub(super) fn verify_binary(path: &Path, latest: &Available) -> Result<()> {
    let output = Command::new(path)
        .args(["--json", "--version"])
        .stdin(Stdio::null())
        .output()
        .context("Couldn't run the newly built cap")?;
    ensure!(
        output.status.success(),
        "New cap failed its version check; installed cap is unchanged"
    );
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let text = envelope["data"]["text"]
        .as_str()
        .context("New cap returned no version")?;
    ensure!(
        envelope["ok"] == true
            && text.split_whitespace().nth(1) == Some(latest.version.as_str())
            && text.contains(&format!("Capsule core {}", &latest.core_revision[..12])),
        "New cap does not match the expected version/core; installed cap is unchanged"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_only_the_cap_package_version_and_validates_core() {
        let manifest = include_str!("../../Cargo.toml");
        let result = parse_manifest(manifest, &"a".repeat(40)).unwrap();
        assert_eq!(result.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(result.core_revision, env!("CAP_CORE_REVISION"));
        assert!(parse_manifest(
            &manifest.replace("name = \"cap\"", "name = \"other\""),
            &"a".repeat(40)
        )
        .is_err());
        assert!(parse_manifest("[package]\nversion = '1.0.0'", "bad").is_err());
    }
}
