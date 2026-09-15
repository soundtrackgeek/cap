use super::{install::hash, Available};
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Read},
    time::Duration,
};

const MAX_ARCHIVE: u64 = 150 * 1024 * 1024;
const MAX_METADATA: u64 = 256 * 1024;

fn bounded(mut reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    reader.by_ref().take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Release download exceeds the size limit"
    );
    Ok(bytes)
}

pub(super) fn download(latest: &mut Available) -> Result<Option<tempfile::TempDir>> {
    #[cfg(feature = "test-hooks")]
    if let Some(fixture) = super::test_fixture::load() {
        return unpack(
            &fs::read(fixture.archive)?,
            &fixture.checksum,
            &format!("cap-{}-windows-x86_64", latest.version),
            latest,
        )
        .map(Some);
    }
    if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        return Ok(None);
    }
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("cap/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .build()?;
    let tag = format!("v{}", latest.version);
    let response = client
        .get(format!(
            "https://api.github.com/repos/soundtrackgeek/cap/releases/tags/{tag}"
        ))
        .send()?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let release: Value =
        serde_json::from_slice(&bounded(response.error_for_status()?, MAX_METADATA)?)?;
    ensure!(
        release["draft"] == false && release["tag_name"] == tag,
        "Release is not published for the expected version"
    );
    let artifact = format!("cap-{}-windows-x86_64", latest.version);
    let archive_name = format!("{artifact}.zip");
    let assets = release["assets"]
        .as_array()
        .context("Release has no assets")?;
    let url = |name: &str| -> Result<String> {
        let asset = assets
            .iter()
            .find(|asset| asset["name"] == name)
            .with_context(|| {
                format!("Release download {name} is not ready. Try `cap update` again later.")
            })?;
        let url = asset["browser_download_url"]
            .as_str()
            .context("Release asset has no URL")?;
        ensure!(
            url == format!("https://github.com/soundtrackgeek/cap/releases/download/{tag}/{name}"),
            "Unexpected release asset URL"
        );
        Ok(url.to_owned())
    };
    let checksum = bounded(
        client
            .get(url(&format!("{archive_name}.sha256"))?)
            .send()?
            .error_for_status()?,
        1024,
    )?;
    let bytes = bounded(
        client.get(url(&archive_name)?).send()?.error_for_status()?,
        MAX_ARCHIVE,
    )?;
    unpack(&bytes, std::str::from_utf8(&checksum)?, &artifact, latest).map(Some)
}

fn unpack(
    bytes: &[u8],
    checksum: &str,
    artifact: &str,
    latest: &mut Available,
) -> Result<tempfile::TempDir> {
    let parts: Vec<_> = checksum.split_whitespace().collect();
    ensure!(
        parts.len() == 2
            && parts[1] == format!("{artifact}.zip")
            && parts[0].eq_ignore_ascii_case(&format!("{:x}", Sha256::digest(bytes))),
        "Release checksum mismatch; installed cap is unchanged"
    );
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    // Read exactly two known members. Never extract archive-provided filesystem paths.
    let manifest: Value = serde_json::from_slice(&bounded(
        archive.by_name(&format!("{artifact}/manifest.json"))?,
        MAX_METADATA,
    )?)?;
    ensure!(
        manifest["version"] == latest.version
            && manifest["capsuleCoreRevision"] == latest.core_revision
            && manifest["dirty"] == false
            && manifest["platform"] == "windows-x86_64",
        "Release provenance does not match the expected cap version"
    );
    let revision = manifest["sourceRevision"]
        .as_str()
        .context("Release has no source revision")?;
    ensure!(
        revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Release source revision is invalid"
    );
    let binary = bounded(
        archive.by_name(&format!("{artifact}/bin/cap.exe"))?,
        MAX_ARCHIVE,
    )?;
    let expected = manifest["payload"]
        .as_array()
        .context("Release has no payload manifest")?
        .iter()
        .find(|item| item["path"] == "bin/cap.exe")
        .and_then(|item| item["sha256"].as_str())
        .context("Release has no executable hash")?;
    let staged = tempfile::tempdir()?;
    fs::create_dir(staged.path().join("bin"))?;
    let path = staged.path().join("bin/cap.exe");
    fs::write(&path, binary)?;
    ensure!(
        hash(&path)? == expected,
        "Release executable checksum mismatch; installed cap is unchanged"
    );
    // Documentation-only commits on master can follow the version's release commit.
    latest.revision = revision.to_owned();
    Ok(staged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn download_requires_both_checksums_and_extracts_only_the_executable() {
        let mut latest = Available {
            version: "1.0.0".into(),
            revision: "a".repeat(40),
            core_revision: "b".repeat(40),
        };
        let artifact = "cap-1.0.0-windows-x86_64";
        let binary = b"synthetic cap";
        let manifest = serde_json::json!({"version":"1.0.0","capsuleCoreRevision":latest.core_revision,
            "sourceRevision":"c".repeat(40),"dirty":false,"platform":"windows-x86_64",
            "payload":[{"path":"bin/cap.exe","sha256":format!("{:x}",Sha256::digest(binary))}]});
        let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
        for (name, bytes) in [
            (
                format!("{artifact}/manifest.json"),
                serde_json::to_vec(&manifest).unwrap(),
            ),
            (format!("{artifact}/bin/cap.exe"), binary.to_vec()),
            ("../escape.txt".into(), b"must not extract".to_vec()),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&bytes).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let checksum = format!("{:x}  {artifact}.zip", Sha256::digest(&bytes));
        let staged = unpack(&bytes, &checksum, artifact, &mut latest).unwrap();
        assert_eq!(fs::read(staged.path().join("bin/cap.exe")).unwrap(), binary);
        assert_eq!(fs::read_dir(staged.path()).unwrap().count(), 1);
        assert!(unpack(
            &bytes,
            &format!("{}  {artifact}.zip", "0".repeat(64)),
            artifact,
            &mut latest
        )
        .is_err());
        latest.version = "2.0.0".into();
        assert!(unpack(&bytes, &checksum, artifact, &mut latest).is_err());
        assert!(bounded(Cursor::new(b"too large"), 2).is_err());
    }
}
