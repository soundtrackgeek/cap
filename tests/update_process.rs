#[test]
fn update_is_a_reserved_command_and_offline_json_is_clean() {
    for args in [
        vec!["--json", "--offline", "update"],
        vec!["--json", "update", "--nonsense"],
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_cap"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], false);
        assert!(output.stderr.is_empty());
        assert!(!output.stdout.contains(&0x1b));
    }
}

#[cfg(all(windows, feature = "test-hooks"))]
mod windows {
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        io::{Cursor, Write},
        path::PathBuf,
        process::Command,
    };

    struct Lab {
        root: tempfile::TempDir,
        exe: PathBuf,
        hash: String,
    }

    impl Lab {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join("bin")).unwrap();
            fs::create_dir(root.path().join("config")).unwrap();
            fs::write(root.path().join("CAP_TEST_LAB_MARKER"), "").unwrap();
            let exe = root.path().join("bin/cap.exe");
            let binary = fs::read(env!("CARGO_BIN_EXE_cap")).unwrap();
            fs::write(&exe, &binary).unwrap();
            let hash = format!("{:x}", Sha256::digest(&binary));
            let receipt = json!({"schemaVersion":1,"binaryPath":exe,"binarySha256":hash,
                "files":["bin\\cap.exe"],"fileHashes":{"bin\\cap.exe":hash},"pathEntry":true,"completionActivation":null});
            fs::write(
                root.path().join(".cap-install.json"),
                serde_json::to_vec(&receipt).unwrap(),
            )
            .unwrap();
            let version = env!("CARGO_PKG_VERSION");
            let core = env!("CAP_CORE_REVISION");
            let artifact = format!("cap-{version}-windows-x86_64");
            let manifest = json!({"version":version,"capsuleCoreRevision":core,"sourceRevision":"a".repeat(40),
                "dirty":false,"platform":"windows-x86_64","payload":[{"path":"bin/cap.exe","sha256":hash}]});
            let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
            for (name, bytes) in [
                (
                    format!("{artifact}/manifest.json"),
                    serde_json::to_vec(&manifest).unwrap(),
                ),
                (format!("{artifact}/bin/cap.exe"), binary),
            ] {
                zip.start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(&bytes).unwrap();
            }
            let bytes = zip.finish().unwrap().into_inner();
            let checksum = format!("{:x}  {artifact}.zip", Sha256::digest(&bytes));
            let archive = root.path().join("update.zip");
            fs::write(&archive, bytes).unwrap();
            let fixture = json!({"current_version":"0.0.0","latest":{"version":version,"core_revision":core,"revision":"a".repeat(40)},"archive":archive,"checksum":checksum});
            fs::write(
                root.path().join("update-fixture.json"),
                serde_json::to_vec(&fixture).unwrap(),
            )
            .unwrap();
            Self { root, exe, hash }
        }

        fn command(&self) -> Command {
            let mut command = Command::new(&self.exe);
            command
                .env("CAP_TEST_UPDATE_ROOT", self.root.path())
                .env("CAP_CONFIG_HOME", self.root.path().join("config"))
                .env(
                    "CAPSULE_DB_PATH",
                    self.root.path().join("must-not-exist.db"),
                );
            command
        }

        fn fixture(&self, change: impl FnOnce(&mut Value)) {
            let path = self.root.path().join("update-fixture.json");
            let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            change(&mut value);
            fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        }

        fn run(&self, args: &[&str]) -> Value {
            let output = self.command().args(args).output().unwrap();
            assert!(
                output.stderr.is_empty(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(output.status.success(), value["ok"] == true, "{value}");
            assert!(!output.stdout.contains(&0x1b));
            value
        }
    }

    #[test]
    fn running_windows_executable_updates_and_remains_runnable_with_a_valid_receipt() {
        let lab = Lab::new();
        let before = fs::read(lab.root.path().join(".cap-install.json")).unwrap();
        let check = lab.run(&["--json", "update", "--check"]);
        assert_eq!(check["data"]["updateAvailable"], true);
        assert_eq!(check["data"]["updated"], false);
        assert_eq!(
            fs::read(lab.root.path().join(".cap-install.json")).unwrap(),
            before
        );
        let result = lab.run(&["--json", "update"]);
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(result["data"]["updated"], true);
        assert_eq!(lab.run(&["--json", "--version"])["ok"], true);
        let receipt: Value =
            serde_json::from_slice(&fs::read(lab.root.path().join(".cap-install.json")).unwrap())
                .unwrap();
        assert_eq!(receipt["binarySha256"], lab.hash);
        assert_eq!(receipt["pathEntry"], true);
        assert!(!lab.root.path().join("must-not-exist.db").exists());
        // The Windows cleanup helper runs after process exit; give it time before TempDir cleanup.
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    #[test]
    fn corrupt_download_and_equal_version_leave_the_installed_copy_unchanged() {
        let lab = Lab::new();
        let before = fs::read(lab.root.path().join(".cap-install.json")).unwrap();
        lab.fixture(|fixture| fixture["checksum"] = json!("bad hash"));
        let result = lab.run(&["--json", "update"]);
        assert_eq!(result["ok"], false);
        assert!(result["error"]["message"]
            .as_str()
            .unwrap()
            .contains("checksum mismatch"));
        assert_eq!(
            fs::read(lab.root.path().join(".cap-install.json")).unwrap(),
            before
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(&lab.exe).unwrap())),
            lab.hash
        );
        lab.fixture(|fixture| fixture["current_version"] = json!(env!("CARGO_PKG_VERSION")));
        assert_eq!(
            lab.run(&["--json", "update"])["data"]["updateAvailable"],
            false
        );
    }
}
