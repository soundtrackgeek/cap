//! Measure real CLI processes against NEW synthetic journals only.
//! Build with `cargo build --release --examples --locked`, then pass the release
//! cap executable and fixture_lab executable. No existing DB path is accepted.

use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    error::Error,
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !(2..=4).contains(&args.len()) {
        return Err("Usage: benchmark_capture <cap.exe> <fixture_lab.exe> [samples=20] [sizes=1000,10000,100000]".into());
    }
    let cap = PathBuf::from(&args[0]).canonicalize()?;
    let generator = PathBuf::from(&args[1]).canonicalize()?;
    let samples = args
        .get(2)
        .map(|v| v.to_string_lossy().parse::<usize>())
        .transpose()?
        .unwrap_or(20);
    if !(2..=100).contains(&samples) {
        return Err("Samples must be between 2 and 100".into());
    }
    let sizes = args
        .get(3)
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_else(|| "1000,10000,100000".to_owned());
    let mut reports = Vec::new();
    for size in sizes.split(',').map(str::parse::<u32>) {
        let size = size?;
        if !(5..=100_000).contains(&size) {
            return Err("Fixture sizes must be between 5 and 100000".into());
        }
        let generated = Command::new(&generator).arg(size.to_string()).output()?;
        if !generated.status.success() {
            return Err("Synthetic fixture generator failed".into());
        }
        let lab: Value = serde_json::from_slice(&generated.stdout)?;
        if lab["synthetic"] != true {
            return Err("Generator did not identify a synthetic lab".into());
        }
        let root = PathBuf::from(lab["root"].as_str().ok_or("Missing lab root")?).canonicalize()?;
        if !root.starts_with(std::env::temp_dir().canonicalize()?) {
            return Err("Refusing a lab outside the OS temporary directory".into());
        }
        let db_path = root.join("capsule.db");
        let db_bytes = std::fs::metadata(&db_path)?.len();
        let mut timings = Vec::new();
        for index in 0..samples {
            let body =
                format!("Synthetic benchmark capture {index}: a quiet walk beside the harbor.");
            let mut command = Command::new(&cap);
            command.env_clear().current_dir(&root);
            for key in ["PATH", "PATHEXT", "SYSTEMROOT", "WINDIR"] {
                if let Some(value) = std::env::var_os(key) {
                    command.env(key, value);
                }
            }
            for (key, value) in lab["environment"]
                .as_object()
                .ok_or("Missing lab environment")?
            {
                command.env(key, value.as_str().ok_or("Invalid lab environment value")?);
            }
            command.args(["--plain", "--no-context", "add", &body]);
            command
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let started = Instant::now();
            let mut child = command.spawn()?;
            let stdout = child.stdout.take().ok_or("Missing stdout")?;
            let mut stderr = child.stderr.take().ok_or("Missing stderr")?;
            let out_thread = thread::spawn(move || {
                let mut first_line_ms = None;
                let mut lines = Vec::new();
                for line in BufReader::new(stdout).lines() {
                    let line = line?;
                    if !line.trim().is_empty() {
                        first_line_ms.get_or_insert(started.elapsed().as_secs_f64() * 1000.0);
                    }
                    lines.push(line);
                }
                Ok::<_, std::io::Error>((first_line_ms, lines.join("\n")))
            });
            let err_thread = thread::spawn(move || {
                let mut bytes = Vec::new();
                stderr.read_to_end(&mut bytes).map(|_| bytes)
            });
            let status = loop {
                if let Some(status) = child.try_wait()? {
                    break status;
                }
                if started.elapsed() > Duration::from_secs(120) {
                    child.kill()?;
                    child.wait()?;
                    return Err(
                        "Synthetic capture exceeded the 120-second benchmark safety limit".into(),
                    );
                }
                thread::sleep(Duration::from_millis(5));
            };
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let (first_line_ms, stdout) =
                out_thread.join().map_err(|_| "stdout reader panicked")??;
            let stderr = err_thread.join().map_err(|_| "stderr reader panicked")??;
            if !status.success() {
                return Err(format!(
                    "Synthetic capture failed ({status}): {}",
                    String::from_utf8_lossy(&stderr)
                )
                .into());
            }
            let connection =
                Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            let actual: i64 =
                connection.query_row("SELECT count(*) FROM entries", [], |r| r.get(0))?;
            let matching: i64 = connection.query_row(
                "SELECT count(*) FROM entries WHERE text=?1",
                [&body],
                |r| r.get(0),
            )?;
            if actual != i64::from(size) + index as i64 + 1 || matching != 1 {
                return Err("Benchmark capture did not persist exactly one expected entry".into());
            }
            if stdout.contains('\u{1b}') {
                return Err("Plain benchmark output contained ANSI".into());
            }
            timings.push(json!({"sample":index, "firstLineMs":first_line_ms, "totalMs":elapsed_ms, "warningCount":String::from_utf8_lossy(&stderr).lines().count()}));
        }
        let totals = timings
            .iter()
            .map(|t| t["totalMs"].as_f64().unwrap())
            .collect::<Vec<_>>();
        let first_lines = timings
            .iter()
            .filter_map(|t| t["firstLineMs"].as_f64())
            .collect::<Vec<_>>();
        reports.push(json!({"entriesBefore":size,"databaseBytesBefore":db_bytes,"lab":root,"samples":timings,"totalMs":percentiles(totals),"firstLineMs":percentiles(first_lines)}));
        eprintln!("Completed {samples} synthetic captures starting at {size} entries.");
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"schemaVersion":1,"synthetic":true,"capExecutable":cap,"measurement":"Fresh process per sample; first line is transport arrival, not an inferred SQLite commit time. Filesystem cache is not forcibly cold. No context or decoration.","reports":reports})
        )?
    );
    Ok(())
}

fn percentiles(mut values: Vec<f64>) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    values.sort_by(f64::total_cmp);
    let at = |p: f64| values[((p * values.len() as f64).ceil() as usize).saturating_sub(1)];
    json!({"p50":at(0.50),"p95":at(0.95),"min":values[0],"max":values[values.len()-1]})
}
