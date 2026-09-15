use super::{atomic_write, Available};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

pub(super) fn hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

struct Metadata {
    path: PathBuf,
    original: Vec<u8>,
    updated: Vec<u8>,
}

pub(super) struct Installation {
    executable: PathBuf,
    original_hash: String,
    receipt: Option<(PathBuf, Vec<u8>, Value)>,
}

impl Installation {
    pub(super) fn inspect(executable: &Path) -> Result<Self> {
        let executable = executable.canonicalize()?;
        let original_hash = hash(&executable)?;
        let receipt_path = executable
            .parent()
            .and_then(Path::parent)
            .context("No installation root")?
            .join(".cap-install.json");
        let receipt = if receipt_path.try_exists()? {
            let bytes = fs::read(&receipt_path)?;
            let value: Value =
                serde_json::from_slice(&bytes).context("The cap install receipt is invalid")?;
            ensure!(
                value["schemaVersion"] == 1,
                "Unsupported cap install receipt"
            );
            let binary_path = value["binaryPath"]
                .as_str()
                .context("Receipt has no binary path")?;
            ensure!(
                Path::new(binary_path).canonicalize()? == executable,
                "Receipt belongs to a different cap executable"
            );
            ensure!(
                value["binarySha256"].as_str() == Some(&original_hash),
                "Installed cap differs from its receipt. Reinstall cap before updating."
            );
            ensure!(
                value["fileHashes"].is_object(),
                "Receipt has no file hashes"
            );
            Some((receipt_path, bytes, value))
        } else {
            None
        };
        Ok(Self {
            executable,
            original_hash,
            receipt,
        })
    }

    fn metadata(&self, latest: &Available, new_hash: &str) -> Result<Vec<Metadata>> {
        let Some((path, original, receipt)) = &self.receipt else {
            return Ok(vec![]);
        };
        ensure!(
            fs::read(path)? == *original,
            "Install receipt changed during the update. Try again."
        );
        let mut receipt = receipt.clone();
        let mut metadata = vec![];
        let root = path.parent().context("Receipt has no parent")?;
        for name in ["manifest.json", "checksums.sha256"] {
            let Some(expected) = receipt["fileHashes"][name].as_str() else {
                continue;
            };
            let path = root.join(name);
            ensure!(
                hash(&path)? == expected,
                "Installed {name} differs from its receipt. Reinstall cap before updating."
            );
            let original = fs::read(&path)?;
            let updated = if name == "manifest.json" {
                let mut manifest: Value = serde_json::from_slice(&original)?;
                manifest["version"] = json!(latest.version);
                manifest["sourceRevision"] = json!(latest.revision);
                manifest["capsuleCoreRevision"] = json!(latest.core_revision);
                manifest["builtAtUtc"] = json!(chrono::Utc::now().to_rfc3339());
                manifest["updatedVia"] = json!("cap update");
                manifest["dirty"] = json!(false);
                if let Some(platform) = manifest["platform"].as_str() {
                    manifest["artifact"] = json!(format!("cap-{}-{platform}", latest.version));
                }
                if let Some(payload) = manifest["payload"].as_array_mut() {
                    for entry in payload {
                        if matches!(entry["path"].as_str(), Some("bin/cap.exe" | "bin\\cap.exe")) {
                            entry["sha256"] = json!(new_hash);
                        }
                    }
                }
                serde_json::to_vec_pretty(&manifest)?
            } else {
                let text = std::str::from_utf8(&original)?;
                let mut found = false;
                let lines: Vec<_> = text
                    .lines()
                    .map(|line| {
                        if matches!(
                            line.split_whitespace().nth(1),
                            Some("bin/cap.exe" | "bin\\cap.exe")
                        ) {
                            found = true;
                            format!("{new_hash}  bin/cap.exe")
                        } else {
                            line.to_owned()
                        }
                    })
                    .collect();
                ensure!(found, "Installed checksum file has no cap executable entry");
                (lines.join("\n") + "\n").into_bytes()
            };
            receipt["fileHashes"][name] = json!(format!("{:x}", Sha256::digest(&updated)));
            metadata.push(Metadata {
                path,
                original,
                updated,
            });
        }
        receipt["binarySha256"] = json!(new_hash);
        receipt["fileHashes"]["bin\\cap.exe"] = json!(new_hash);
        if let Some(files) = receipt["files"].as_array_mut() {
            if !files.iter().any(|file| file == "bin\\.cap-update.lock") {
                files.push(json!("bin\\.cap-update.lock"));
            }
            receipt["fileHashes"]["bin\\.cap-update.lock"] =
                json!(format!("{:x}", Sha256::digest([])));
        }
        receipt["version"] = json!(latest.version);
        receipt["sourceRevision"] = json!(latest.revision);
        receipt["updatedAtUtc"] = json!(chrono::Utc::now().to_rfc3339());
        metadata.push(Metadata {
            path: path.clone(),
            original: original.clone(),
            updated: serde_json::to_vec_pretty(&receipt)?,
        });
        Ok(metadata)
    }

    pub(super) fn replace(
        self,
        candidate: &Path,
        latest: &Available,
        replace: impl FnOnce(&Path) -> io::Result<()>,
    ) -> Result<()> {
        ensure!(
            hash(&self.executable)? == self.original_hash,
            "Installed cap changed during the build. Try again."
        );
        let metadata = self.metadata(latest, &hash(candidate)?)?;
        // Keep a separate old binary: Windows replacement may fail after renaming the running file.
        let backup = tempfile::NamedTempFile::new_in(
            self.executable.parent().context("No binary directory")?,
        )?;
        fs::copy(&self.executable, backup.path())?;
        backup.as_file().sync_all()?;
        let mut published = 0;
        let result = (|| -> Result<()> {
            for item in &metadata {
                atomic_write(&item.path, &item.updated)?;
                published += 1;
            }
            replace(candidate).context("Could not replace cap.exe")?;
            Ok(())
        })();
        if let Err(error) = result {
            let mut recovery_errors = vec![];
            if hash(&self.executable).ok().as_ref() != Some(&self.original_hash) {
                if let Err(restore) = fs::copy(backup.path(), &self.executable) {
                    recovery_errors.push(format!("binary: {restore}"));
                }
            }
            for item in metadata[..published].iter().rev() {
                if let Err(restore) = atomic_write(&item.path, &item.original) {
                    recovery_errors.push(format!("{}: {restore}", item.path.display()));
                }
            }
            if !recovery_errors.is_empty() {
                let (_, saved) = backup.keep()?;
                bail!("{error:#}. Recovery needs attention: {}. The previous executable is saved at {}.", recovery_errors.join("; "), saved.display());
            }
            return Err(error.context("The previous cap installation was preserved"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, Available) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("bin")).unwrap();
        let exe = dir.path().join("bin/cap.exe");
        fs::write(&exe, b"old cap").unwrap();
        let candidate = dir.path().join("candidate.exe");
        fs::write(&candidate, b"new cap").unwrap();
        let hash = hash(&exe).unwrap();
        let manifest = serde_json::to_vec(&json!({"version":"0.0.0","platform":"windows-x86_64",
            "payload":[{"path":"bin/cap.exe","sha256":hash}]}))
        .unwrap();
        let checksums = format!("{hash}  bin/cap.exe\n");
        fs::write(dir.path().join("manifest.json"), &manifest).unwrap();
        fs::write(dir.path().join("checksums.sha256"), &checksums).unwrap();
        let receipt = json!({"schemaVersion":1,"binaryPath":exe,"binarySha256":hash,
            "fileHashes":{"bin\\cap.exe":hash,
                "manifest.json":format!("{:x}",Sha256::digest(&manifest)),
                "checksums.sha256":format!("{:x}",Sha256::digest(checksums.as_bytes()))},
            "pathEntry":true,"completionActivation":{"keep":"me"}});
        fs::write(
            dir.path().join(".cap-install.json"),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        let latest = Available {
            version: "1.0.0".into(),
            revision: "a".repeat(40),
            core_revision: "b".repeat(40),
        };
        (dir, exe, candidate, latest)
    }

    #[test]
    fn replacement_updates_receipt_and_preserves_path_and_completion_ownership() {
        let (dir, exe, candidate, latest) = fixture();
        Installation::inspect(&exe)
            .unwrap()
            .replace(&candidate, &latest, |path| fs::copy(path, &exe).map(|_| ()))
            .unwrap();
        let receipt: Value =
            serde_json::from_slice(&fs::read(dir.path().join(".cap-install.json")).unwrap())
                .unwrap();
        assert_eq!(receipt["binarySha256"], hash(&exe).unwrap());
        assert_eq!(receipt["fileHashes"]["bin\\cap.exe"], hash(&exe).unwrap());
        assert_eq!(receipt["pathEntry"], true);
        assert_eq!(receipt["completionActivation"]["keep"], "me");
        assert_eq!(receipt["version"], "1.0.0");
        for name in ["manifest.json", "checksums.sha256"] {
            assert_eq!(
                receipt["fileHashes"][name],
                hash(&dir.path().join(name)).unwrap()
            );
        }
        let manifest: Value =
            serde_json::from_slice(&fs::read(dir.path().join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["payload"][0]["sha256"], hash(&exe).unwrap());
    }

    #[test]
    fn failed_replacement_restores_binary_and_exact_receipt_even_after_rename() {
        let (dir, exe, candidate, latest) = fixture();
        let receipt = fs::read(dir.path().join(".cap-install.json")).unwrap();
        let result = Installation::inspect(&exe)
            .unwrap()
            .replace(&candidate, &latest, |_| {
                fs::remove_file(&exe)?;
                Err(io::Error::other("simulated Windows failure after rename"))
            });
        assert!(result.is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"old cap");
        assert_eq!(
            fs::read(dir.path().join(".cap-install.json")).unwrap(),
            receipt
        );
    }

    #[test]
    fn modified_executable_and_changed_receipt_are_refused() {
        let (dir, exe, candidate, latest) = fixture();
        let plan = Installation::inspect(&exe).unwrap();
        fs::write(dir.path().join(".cap-install.json"), b"changed").unwrap();
        assert!(plan
            .replace(&candidate, &latest, |_| panic!("must not replace"))
            .is_err());
        fs::write(&exe, b"changed").unwrap();
        assert!(Installation::inspect(&exe).is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"changed");
    }

    #[cfg(windows)]
    #[test]
    fn locked_receipt_rolls_back_already_published_manifest_and_checksums() {
        use std::os::windows::fs::OpenOptionsExt;
        let (dir, exe, candidate, latest) = fixture();
        let plan = Installation::inspect(&exe).unwrap();
        let names = ["manifest.json", "checksums.sha256", ".cap-install.json"];
        let before: Vec<_> = names
            .iter()
            .map(|name| fs::read(dir.path().join(name)).unwrap())
            .collect();
        // Permit reads but deny replacement, so failure occurs after metadata publication.
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(dir.path().join(".cap-install.json"))
            .unwrap();
        assert!(plan
            .replace(&candidate, &latest, |_| panic!("must not replace"))
            .is_err());
        drop(held);
        for (name, bytes) in names.iter().zip(before) {
            assert_eq!(fs::read(dir.path().join(name)).unwrap(), bytes);
        }
        assert_eq!(fs::read(exe).unwrap(), b"old cap");
    }
}
