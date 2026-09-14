//! Native-console interruption check using only synthetic FX. Run in a console
//! with the built cap executable as the single argument. Never opens a journal.

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        os::windows::process::CommandExt,
        process::Command,
        time::{Duration, Instant},
    };
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GenerateConsoleCtrlEvent(event: u32, process_group: u32) -> i32;
    }
    let executable = std::env::args_os()
        .nth(1)
        .ok_or("Pass the built cap.exe path")?;
    let started = Instant::now();
    // Only the newly spawned child group receives CTRL_BREAK. The surrounding
    // shell is unaffected, so its exit semantics cannot hide cap's real status.
    let mut child = Command::new(executable)
        .args(["--color", "always", "--motion", "full", "fx", "ocean"])
        .creation_flags(0x0000_0200) // CREATE_NEW_PROCESS_GROUP
        .spawn()?;
    std::thread::sleep(Duration::from_millis(800));
    // SAFETY: GenerateConsoleCtrlEvent takes scalar values; the nonzero group
    // ID belongs to our live child. It does not signal unrelated process groups.
    if unsafe { GenerateConsoleCtrlEvent(1, child.id()) } == 0 {
        let error = std::io::Error::last_os_error();
        child.kill()?;
        child.wait()?;
        return Err(error.into());
    }
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(5) {
            child.kill()?;
            child.wait()?;
            return Err("FX process did not terminate after its interrupt".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    println!(
        "\nSynthetic interruption probe: child exit={}, elapsed={}ms",
        status.code().unwrap_or(-1),
        started.elapsed().as_millis()
    );
    if status.code() != Some(130) {
        return Err("Expected cap interruption exit code 130".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This development probe requires a Windows console.");
}
