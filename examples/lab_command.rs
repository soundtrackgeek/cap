//! Run an interactive cap command inside a previously generated synthetic lab.
//! Keeps console handles, clears environment, and checks Windows console modes
//! after the child exits. Never accepts a production database destination.

use std::{collections::BTreeMap, error::Error, fs, path::PathBuf, process::Command};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let manifest = PathBuf::from(
        args.next()
            .ok_or("Pass lab.json, cap.exe, and command arguments")?,
    )
    .canonicalize()?;
    let executable = PathBuf::from(args.next().ok_or("Pass cap.exe")?).canonicalize()?;
    let root = manifest.parent().ok_or("Lab has no parent")?;
    let temporary = std::env::temp_dir().canonicalize()?;
    if !root.starts_with(&temporary)
        || !root
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("cap lab "))
        || manifest.file_name().is_none_or(|name| name != "lab.json")
    {
        return Err("Only a generated cap lab under the OS temp directory is supported".into());
    }
    let lab: serde_json::Value = serde_json::from_slice(&fs::read(&manifest)?)?;
    if lab["synthetic"] != true
        || PathBuf::from(lab["root"].as_str().ok_or("Missing root")?).canonicalize()? != root
    {
        return Err("Invalid synthetic lab identity".into());
    }
    let environment: BTreeMap<String, String> = serde_json::from_value(lab["environment"].clone())?;
    for required in [
        "CAPSULE_DB_PATH",
        "CAPSULE_CONFIG_PATH",
        "CAPSULE_PATH_SETTINGS_PATH",
        "CAPSULE_BACKUP_DIR",
        "CAPSULE_IMAGES_MEDIA_ROOT",
        "CAPSULE_SYNC_PATH",
        "CAPSULE_HOME",
        "CAP_CONFIG_HOME",
        "APPDATA",
        "LOCALAPPDATA",
        "USERPROFILE",
        "HOME",
        "TEMP",
        "TMP",
    ] {
        let path = PathBuf::from(
            environment
                .get(required)
                .ok_or("Missing isolated environment field")?,
        )
        .canonicalize()?;
        if !path.starts_with(root) {
            return Err(format!("Lab field {required} escapes its owned root").into());
        }
    }
    let mut child = Command::new(executable);
    child.env_clear().current_dir(root);
    for key in ["PATH", "PATHEXT", "SYSTEMROOT", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            child.env(key, value);
        }
    }
    child.envs(environment).args(args);
    let before = console_modes();
    let status = child.status()?;
    let after = console_modes();
    if before != after {
        return Err(format!("Console modes were not restored: {before:?} -> {after:?}").into());
    }
    eprintln!(
        "Synthetic lab child exit={}; console modes restored: {after:?}",
        status.code().unwrap_or(-1)
    );
    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(windows)]
fn console_modes() -> [Option<u32>; 2] {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn GetConsoleMode(handle: *mut c_void, mode: *mut u32) -> i32;
    }
    [(-10i32) as u32, (-11i32) as u32].map(|which| {
        let mut mode = 0;
        // SAFETY: only reads this process's inherited standard console handles.
        (unsafe { GetConsoleMode(GetStdHandle(which), &mut mode) } != 0).then_some(mode)
    })
}

#[cfg(not(windows))]
fn console_modes() -> [Option<u32>; 2] {
    [None, None]
}
