use anyhow::{ensure, Context, Result};
use capsule_core::{
    capture_entry_with_hooks_for_database, reconcile_capture_for_database, BackupPolicy,
    CaptureRequest, FileIdentity,
};
use chrono::DateTime;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

struct Running(Option<Child>);
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl Running {
    fn finish(mut self) -> Result<Value> {
        let started = Instant::now();
        loop {
            if self.0.as_mut().unwrap().try_wait()?.is_some() {
                break;
            }
            ensure!(
                started.elapsed() < Duration::from_secs(30),
                "child did not finish within 30 seconds"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let output = self.0.take().unwrap().wait_with_output()?;
        serde_json::from_slice(&output.stdout).with_context(|| {
            format!(
                "invalid child output: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })
    }
    fn ready(&mut self, path: &Path) -> Result<()> {
        let started = Instant::now();
        while !path.is_file() {
            ensure!(
                self.0.as_mut().unwrap().try_wait()?.is_none(),
                "child exited before ready marker"
            );
            ensure!(
                started.elapsed() < Duration::from_secs(20),
                "child did not reach checkpoint"
            );
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}
struct Lab {
    _temp: tempfile::TempDir,
    root: PathBuf,
    db: PathBuf,
}
impl Lab {
    fn new() -> Result<Self> {
        let temp = tempfile::Builder::new().prefix("cap-core-lab-").tempdir()?;
        let root = temp.path().canonicalize()?;
        fs::write(root.join("owned-probe"), "synthetic cap core v1")?;
        fs::create_dir(root.join("backups"))?;
        let db = root.join("capsule.db");
        let connection = Connection::open(&db)?;
        connection.execute_batch(include_str!("../tests/fixtures/capsule.sql"))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_checkpoint(TRUNCATE);")?;
        drop(connection);
        Ok(Self {
            _temp: temp,
            root,
            db,
        })
    }
    fn request(&self, label: &str) -> CaptureRequest {
        let mut request = CaptureRequest::new(
            format!("  Synthetic process {label}\r\nsecond line  "),
            format!("capture-{label}"),
            format!("entry_probe_{label}"),
            self.db.clone(),
            DateTime::parse_from_rfc3339("2026-09-14T12:34:56+02:00").unwrap(),
        )
        .with_backup_policy(BackupPolicy::new(self.root.join("backups"), 5));
        request.database_identity = Some(FileIdentity::for_path(&self.db));
        request.tags = vec![" Process ".into(), "PROCESS".into(), "probe".into()];
        request.mood = Some("calm".into());
        request.title = Some("Synthetic title".into());
        request.summary = Some("Synthetic summary".into());
        request.starred = true;
        request.pinned = true;
        request.continue_from_uuid = Some("entry_root".into());
        request
    }
    fn spawn(&self, request: &CaptureRequest, mode: &str, checkpoint: &str) -> Result<Running> {
        let path = self.root.join(format!("{}.json", request.reserved_uuid));
        fs::write(&path, serde_json::to_vec(request)?)?;
        let mut cmd = Command::new(std::env::current_exe()?);
        cmd.env_clear();
        cmd.env("TEMP", &self.root).env("TMP", &self.root);
        for key in ["PATH", "PATHEXT", "SYSTEMROOT", "WINDIR"] {
            if let Some(v) = std::env::var_os(key) {
                cmd.env(key, v);
            }
        }
        cmd.args(["--child", mode])
            .arg(&path)
            .arg(checkpoint)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Ok(Running(Some(cmd.spawn()?)))
    }
    fn capture(&self, request: &CaptureRequest) -> Result<Value> {
        self.spawn(request, "capture", "")?.finish()
    }
    fn count(&self) -> Result<i64> {
        Ok(Connection::open(&self.db)?
            .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))?)
    }
    fn invariants(&self, expected: i64) -> Result<()> {
        let c = Connection::open(&self.db)?;
        ensure!(
            self.count()? == expected,
            "wrong number of persisted entries"
        );
        let integrity: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        ensure!(integrity == "ok", "integrity check failed");
        let bad_fk: i64 =
            c.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })?;
        ensure!(bad_fk == 0, "foreign keys damaged");
        for sql in [
            "SELECT count(*) FROM entry_tags t LEFT JOIN entries e ON e.id=t.entry_id WHERE e.id IS NULL",
            "SELECT count(*) FROM history h LEFT JOIN entries e ON e.id=h.entry_id WHERE e.id IS NULL",
            "SELECT count(*) FROM entry_continuations t LEFT JOIN entries p ON p.uuid=t.parent_entry_uuid LEFT JOIN entries c ON c.uuid=t.child_entry_uuid WHERE p.uuid IS NULL OR c.uuid IS NULL",
            "SELECT count(*) FROM entries e LEFT JOIN entries_fts f ON f.rowid=e.id WHERE f.rowid IS NULL OR f.text<>e.text_plain",
        ] { let n:i64=c.query_row(sql,[],|r|r.get(0))?; ensure!(n==0,"relation/index mismatch: {sql}"); }
        let mut backups = 0;
        for item in fs::read_dir(self.root.join("backups"))? {
            let p = item?.path();
            if p.extension().and_then(|x| x.to_str()) == Some("db") {
                backups += 1;
                let bc =
                    Connection::open_with_flags(&p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                let integrity: String = bc.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
                ensure!(integrity == "ok", "unverified published backup");
                ensure!(
                    p.with_extension("json").exists(),
                    "published backup missing manifest"
                );
            }
        }
        ensure!(
            backups > 0 && backups <= 5,
            "retention count violated: {backups}"
        );
        Ok(())
    }
}
fn child(args: &[String]) -> Result<()> {
    let request_path = PathBuf::from(&args[2]);
    let root = request_path.parent().context("no parent")?.canonicalize()?;
    ensure!(
        root.starts_with(std::env::temp_dir().canonicalize()?),
        "child root outside temporary directory"
    );
    ensure!(
        fs::read_to_string(root.join("owned-probe"))? == "synthetic cap core v1",
        "not an owned lab"
    );
    let request: CaptureRequest = serde_json::from_slice(&fs::read(request_path)?)?;
    ensure!(
        request.database_path == root.join("capsule.db"),
        "request escaped lab"
    );
    let ready = root.join(format!("{}.ready", request.reserved_uuid));
    let pause = || -> Result<()> {
        fs::write(&ready, "ready")?;
        loop {
            thread::sleep(Duration::from_millis(10));
        }
    };
    match args[1].as_str() {
        "sql-lock" => {
            let c = Connection::open(&request.database_path)?;
            c.execute_batch("BEGIN IMMEDIATE")?;
            pause()?;
        }
        "sidecar-lock" => {
            capsule_core::with_mutation_lock_for_database(&request.database_path, |_| pause())?;
        }
        _ => {
            let checkpoint = args.get(3).cloned().unwrap_or_default();
            let capture_started = Instant::now();
            let checkpoints = Mutex::new(BTreeMap::<String, f64>::new());
            let result = capture_entry_with_hooks_for_database(request, &|point| {
                checkpoints.lock().unwrap().insert(
                    format!("{point:?}"),
                    capture_started.elapsed().as_secs_f64() * 1000.0,
                );
                if format!("{point:?}") == checkpoint {
                    pause()?;
                } else if format!("resume:{point:?}") == checkpoint {
                    fs::write(&ready, "ready")?;
                    let started = Instant::now();
                    while !ready.with_extension("resume").exists() {
                        ensure!(
                            started.elapsed() < Duration::from_secs(10),
                            "resume checkpoint timed out"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                }
                Ok(())
            });
            println!(
                "{}",
                match result {
                    Ok(receipt) =>
                        json!({"ok":true,"receipt":receipt,"captureElapsedMs":capture_started.elapsed().as_millis(),"checkpointsMs":checkpoints.into_inner().unwrap()}),
                    Err(error) =>
                        json!({"ok":false,"error":error,"captureElapsedMs":capture_started.elapsed().as_millis()}),
                }
            );
        }
    }
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|x| x == "--child") {
        return child(&args);
    }
    if args.as_slice() == ["--bench"] {
        return benchmark();
    }
    ensure!(
        args.is_empty(),
        "coordinator accepts no journal path or other arguments"
    );
    let mut evidence = Vec::new();
    let lab = Lab::new()?;
    let jobs = (0..8)
        .map(|i| lab.spawn(&lab.request(&format!("parallel{i}")), "capture", ""))
        .collect::<Result<Vec<_>>>()?;
    for job in jobs {
        let result = job.finish()?;
        ensure!(result["ok"] == true, "parallel capture failed: {result}");
    }
    lab.invariants(13)?;
    evidence.push(json!({"case":"eight_independent_writers","ok":true}));
    eprintln!(
        "Passed: eight independent writers, SQLite integrity, references, FTS, backup retention"
    );
    for point in ["AfterBackup", "BeforeCommit", "AfterCommit"] {
        let lab = Lab::new()?;
        let request = lab.request(&point.to_ascii_lowercase());
        let mut child = lab.spawn(&request, "capture", point)?;
        child.ready(&lab.root.join(format!("{}.ready", request.reserved_uuid)))?;
        drop(child);
        let status = reconcile_capture_for_database(&request)?;
        let expected = if point == "AfterCommit" {
            "committed"
        } else {
            "not_committed"
        };
        ensure!(
            serde_json::to_value(status.outcome)? == expected,
            "wrong reconciliation after kill at {point}"
        );
        let retry = lab.capture(&request)?;
        ensure!(retry["ok"] == true, "replay failed: {retry}");
        let again = lab.capture(&request)?;
        ensure!(again["ok"] == true, "second replay failed: {again}");
        lab.invariants(6)?;
        evidence.push(json!({"case":format!("kill_{point}_and_replay"),"ok":true}));
        eprintln!("Passed: kill at {point}, read-only reconciliation, exact-once replay");
    }
    let lab = Lab::new()?;
    let request = lab.request("replace");
    let replacement = lab.root.join("replacement.db");
    let c = Connection::open(&replacement)?;
    c.execute_batch(include_str!("../tests/fixtures/capsule.sql"))?;
    drop(c);
    fs::rename(&replacement, &lab.db)?;
    let replaced = lab.capture(&request)?;
    ensure!(
        replaced["ok"] == false && replaced["error"]["code"] == "database_replaced",
        "replacement not rejected: {replaced}"
    );
    ensure!(lab.count()? == 5, "replacement wrote to changed DB");
    evidence.push(json!({"case":"same_path_file_replacement","ok":true}));
    eprintln!("Passed: same-path database replacement rejected");
    let lab = Lab::new()?;
    let request = lab.request("replaceafterpreflight");
    let mut writer = lab.spawn(&request, "capture", "resume:BeforeBackup")?;
    let ready = lab.root.join("entry_probe_replaceafterpreflight.ready");
    writer.ready(&ready)?;
    let replacement = lab.root.join("replacement.db");
    fs::copy(&lab.db, &replacement)?;
    fs::remove_file(&lab.db)?;
    fs::rename(replacement, &lab.db)?;
    let before = fs::read(&lab.db)?;
    fs::write(ready.with_extension("resume"), b"resume")?;
    let result = writer.finish()?;
    ensure!(
        result["error"]["code"] == "database_replaced",
        "wrong replacement result: {result}"
    );
    ensure!(fs::read(&lab.db)? == before, "replacement mutated");
    ensure!(
        !fs::read_dir(lab.root.join("backups"))?.any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("capsule_backup_")
        }),
        "replacement was backed up before refusal"
    );
    evidence.push(json!({"case":"replacement_after_preflight_before_backup","ok":true}));
    let lab = Lab::new()?;
    let mut request = lab.request("blockedbackup");
    let blocked = lab.root.join("backup-is-a-file");
    fs::write(&blocked, b"owned synthetic obstacle")?;
    request.backup_policy = Some(BackupPolicy::new(blocked, 5));
    let failed = lab.capture(&request)?;
    ensure!(
        failed["ok"] == false && failed["error"]["code"] == "backup_failed",
        "wrong backup failure: {failed}"
    );
    ensure!(lab.count()? == 5, "backup failure wrote an entry");
    evidence.push(json!({"case":"failed_backup_prevents_insert","ok":true}));
    eprintln!("Passed: backup failure prevents insert");
    let lab = Lab::new()?;
    let hold_sql = lab.request("sqlhold");
    let mut sql = lab.spawn(&hold_sql, "sql-lock", "")?;
    sql.ready(&lab.root.join("entry_probe_sqlhold.ready"))?;
    let hold_side = lab.request("sidehold");
    let mut side = lab.spawn(&hold_side, "sidecar-lock", "")?;
    side.ready(&lab.root.join("entry_probe_sidehold.ready"))?;
    let request = lab.request("contended");
    let started = Instant::now();
    let writer = lab.spawn(&request, "capture", "")?;
    thread::sleep(Duration::from_secs(3));
    drop(side);
    let result = writer.finish()?;
    let elapsed = started.elapsed().as_millis();
    drop(sql);
    ensure!(
        result["ok"] == false,
        "contended write unexpectedly succeeded"
    );
    ensure!(
        elapsed <= 16_500,
        "coordination + SQLite multiplied the 15s budget: {elapsed}ms; result={result}"
    );
    ensure!(
        result["captureElapsedMs"]
            .as_u64()
            .context("missing operation timing")?
            <= 15_500,
        "capture itself exceeded the 15-second budget: {result}"
    );
    ensure!(
        result["error"]["code"] == "database_busy",
        "busy error not classified: {result}"
    );
    ensure!(lab.count()? == 5, "busy failure saved a row");
    ensure!(
        lab.capture(&request)?["ok"] == true,
        "post-kill lock release failed"
    );
    lab.invariants(6)?;
    evidence.push(json!({"case":"shared_15_second_lock_budget_and_process_death_release","elapsedMs":elapsed,"captureElapsedMs":result["captureElapsedMs"],"ok":true}));
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"synthetic":true,"cases":evidence}))?
    );
    Ok(())
}

fn benchmark() -> Result<()> {
    let mut evidence = Vec::new();
    for size in [1_000i64, 10_000, 100_000] {
        let lab = Lab::new()?;
        let mut connection = Connection::open(&lab.db)?;
        let tx = connection.transaction()?;
        for index in 6..=size {
            let text = format!(
                "Synthetic benchmark memory {index}. A quiet walk by the harbor before dinner."
            );
            tx.execute("INSERT INTO entries(uuid,created_at,updated_at,text,text_plain,content_format,hidden) VALUES(?1,'2026-09-14 12:00:00','2026-09-14 12:00:00',?2,?2,'markdown',0)", rusqlite::params![format!("entry_lab_{index:06}"), text])?;
            tx.execute(
                "INSERT INTO entries_fts(rowid,text) VALUES(?1,?2)",
                rusqlite::params![tx.last_insert_rowid(), text],
            )?;
        }
        tx.commit()?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        drop(connection);
        let database_bytes = fs::metadata(&lab.db)?.len();
        let mut samples = Vec::new();
        for index in 0..20 {
            let started = Instant::now();
            let result = lab.capture(&lab.request(&format!("bench{index}")))?;
            ensure!(
                result["ok"] == true,
                "benchmark capture failed at {size}: {result}"
            );
            ensure!(lab.count()? == size + index + 1, "benchmark count mismatch");
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let checkpoint = |name: &str| -> Result<f64> {
                result["checkpointsMs"][name]
                    .as_f64()
                    .with_context(|| format!("missing checkpoint {name}"))
            };
            samples.push(json!({
                "sample":index, "processMs":elapsed_ms,
                "captureMs":result["captureElapsedMs"],
                "resolveBeforeBackupMs":checkpoint("BeforeBackup")?,
                "backupAndCoordinationMs":checkpoint("AfterBackup")? - checkpoint("BeforeBackup")?,
                "legacyRepairAndReconcileMs":checkpoint("BeforeBegin")? - checkpoint("AfterBackup")?,
                "beginAndAllocationMs":checkpoint("BeforeInsert")? - checkpoint("BeforeBegin")?,
                "entryAndRelationsMs":checkpoint("BeforeResequence")? - checkpoint("BeforeInsert")?,
                "resequenceMs":checkpoint("BeforeCommit")? - checkpoint("BeforeResequence")?,
                "commitAndGuardFinalizationMs":checkpoint("AfterCommit")? - checkpoint("BeforeCommit")?,
            }));
        }
        lab.invariants(size + 20)?;
        let mut percentiles = BTreeMap::new();
        for key in [
            "processMs",
            "captureMs",
            "resolveBeforeBackupMs",
            "backupAndCoordinationMs",
            "legacyRepairAndReconcileMs",
            "beginAndAllocationMs",
            "entryAndRelationsMs",
            "resequenceMs",
            "commitAndGuardFinalizationMs",
        ] {
            let mut values = samples
                .iter()
                .map(|sample| sample[key].as_f64().unwrap())
                .collect::<Vec<_>>();
            values.sort_by(f64::total_cmp);
            percentiles.insert(
                key,
                json!({"p50":values[9], "p95":values[18], "min":values[0], "max":values[19]}),
            );
        }
        eprintln!(
            "Core synthetic benchmark: {size} entries, 20 samples, capture p50={}ms p95={}ms",
            percentiles["captureMs"]["p50"], percentiles["captureMs"]["p95"]
        );
        evidence.push(json!({"entries":size,"databaseBytes":database_bytes,"samples":samples,"phasesMs":percentiles}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"synthetic":true,"coreRevision":env!("CAP_CORE_REVISION"),"measurement":"Separate process per sample, shared-core capture only, filesystem cache not controlled. Phase boundaries are injected core checkpoints; backup includes coordination/verification, finalization includes commit and guard/receipt work. No CLI pending/context/rendering work.","samples":evidence})
        )?
    );
    Ok(())
}
