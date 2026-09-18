use anyhow::{bail, Context, Result};
use chrono::Utc;
use colored::Colorize;
use crossbeam_channel::unbounded;
use flate2::write::GzEncoder;
use flate2::Compression;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::cli::ExportArgs;
use crate::manifest::ManifestManager;
use crate::mysql_locator::{find_mysql, find_mysqldump};
use crate::option_file::MySqlOptionFile;
use crate::progress::{CountingReader, ProgressTracker, WorkerLiveState};
use crate::runner::{FailedTask, RunSummary, RunSummaryState, WorkerEvent};

#[derive(Debug, Clone)]
pub struct ExportTask {
    pub database: String,
    pub table_name: String,
    pub file_name: String,
    pub estimated_bytes: u64,
    pub _estimated_rows: u64,
    pub output_path: PathBuf,
}

#[cfg(windows)]
fn kill_os_pid(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .output();
}

#[cfg(unix)]
fn kill_os_pid(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
}

pub fn run_export(mut args: ExportArgs) -> Result<()> {
    // 1. Locate binaries
    let mysql_bin = if args.dry_run {
        find_mysql(args.mysql_bin.as_deref()).unwrap_or_else(|_| PathBuf::from("mysql"))
    } else {
        find_mysql(args.mysql_bin.as_deref())?
    };

    let mysqldump_bin = if args.dry_run {
        find_mysqldump(args.mysqldump_bin.as_deref(), Some(&mysql_bin))
            .unwrap_or_else(|_| PathBuf::from("mysqldump"))
    } else {
        find_mysqldump(args.mysqldump_bin.as_deref(), Some(&mysql_bin))?
    };

    // 2. Resolve credentials securely
    args.resolve_password()?;

    // 3. Create temporary MySQL option file with RAII cleanup
    let option_file =
        MySqlOptionFile::create(&args.host, args.port, &args.user, args.password.as_deref())?;

    // 4. Discover databases and tables from information_schema
    println!(
        "{}",
        "Discovering databases & schema metadata...".cyan().bold()
    );
    let mut tasks = collect_export_tasks(&args, &mysql_bin, option_file.path())?;

    if tasks.is_empty() {
        println!("{}", "No matching tables found to export.".yellow());
        return Ok(());
    }

    // Sort descending by size (Longest Processing Time first for optimal work-stealing)
    tasks.sort_by_key(|t| std::cmp::Reverse(t.estimated_bytes));

    let total_scanned_count = tasks.len();
    let total_scanned_bytes: u64 = tasks.iter().map(|t| t.estimated_bytes).sum();

    // 5. Manifest for resume support
    let log_dir = args.resolved_log_dir();

    let manifest = Arc::new(ManifestManager::new(
        &log_dir,
        &args.dir,
        args.manifest.as_deref(),
    ));

    let manifest_count = manifest.count();
    if let Some(loaded_path) = manifest.loaded_from() {
        println!(
            "{}",
            format!(
                "[MANIFEST] Loaded {manifest_count} completed export records from: {}",
                loaded_path.display()
            )
            .cyan()
        );
    }

    if args.resume {
        let before_count = tasks.len();
        let mut skipped_bytes = 0u64;

        tasks.retain(|t| {
            if manifest.is_completed(&t.file_name, Some(&t.output_path)) {
                skipped_bytes += t.estimated_bytes;
                false
            } else {
                true
            }
        });

        let skipped_count = before_count - tasks.len();
        let skipped_mb = (skipped_bytes as f64) / (1024.0 * 1024.0);
        let remaining_mb =
            (tasks.iter().map(|t| t.estimated_bytes).sum::<u64>() as f64) / (1024.0 * 1024.0);

        if skipped_count > 0 {
            println!(
                "{}",
                format!(
                    "Resuming: skipped {skipped_count} previously exported tables. ({skipped_mb:.2} MB skipped, {remaining} tables remaining - {remaining_mb:.2} MB).",
                    remaining = tasks.len()
                )
                .yellow()
                .bold()
            );
        } else if manifest_count > 0 {
            eprintln!(
                "{}",
                format!(
                    "[WARNING] Manifest contains {manifest_count} exported tables, but 0 matched target tables!",
                )
                .yellow()
                .bold()
            );
        }
    } else if manifest_count > 0 {
        println!(
            "{}",
            format!(
                "[NOTICE] Found {} previously exported tables in manifest. Run with '--resume' to skip them.",
                manifest_count
            )
            .yellow()
            .bold()
        );
    }

    if tasks.is_empty() {
        println!(
            "{}",
            "All requested tables have already been exported."
                .green()
                .bold()
        );
        return Ok(());
    }

    // Breakdown by database
    let mut db_summaries: HashMap<String, (usize, u64)> = HashMap::new();
    let mut remaining_bytes = 0u64;
    for t in &tasks {
        remaining_bytes += t.estimated_bytes;
        let entry = db_summaries.entry(t.database.clone()).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += t.estimated_bytes;
    }

    // Header Banner
    println!();
    println!(
        "{}",
        "==================================================".cyan()
    );
    println!("{}", " MYSQL TURBOLOAD ENTERPRISE v1.0.0".cyan().bold());
    println!("{}", " Concurrent Bulk Database Export Engine".cyan());
    println!(
        "{}",
        "==================================================".cyan()
    );
    println!("Target Server : {}:{}", args.host, args.port);
    println!("MySQL User    : {}", args.user);
    println!("MySQL Dump Bin: {}", mysqldump_bin.display());
    let dir_note = if !args.dir.exists() {
        " (will be created)"
    } else {
        ""
    };
    println!("Output Folder : {}{}", args.dir.display(), dir_note);
    println!("Manifest File : {}", manifest.file_path().display());
    println!("Workers       : {}", args.resolved_workers());
    if args.resume && total_scanned_count != tasks.len() {
        let skipped_count = total_scanned_count - tasks.len();
        let skipped_mb = ((total_scanned_bytes - remaining_bytes) as f64) / (1024.0 * 1024.0);
        let rem_mb = (remaining_bytes as f64) / (1024.0 * 1024.0);
        println!(
            "Total In Schemas: {} tables ({:.2} MB)",
            total_scanned_count,
            (total_scanned_bytes as f64) / (1024.0 * 1024.0)
        );
        println!(
            "Already Exported: {} tables ({:.2} MB) [Skipped via manifest]",
            skipped_count.to_string().yellow(),
            skipped_mb
        );
        println!(
            "To Export       : {} tables ({:.2} MB)",
            tasks.len().to_string().green().bold(),
            rem_mb
        );
    } else {
        println!("Tables To Dump: {}", tasks.len());
        println!(
            "Estimated Size: {:.2} MB",
            (remaining_bytes as f64) / (1024.0 * 1024.0)
        );
    }

    // Database breakdown
    println!();
    println!("{}", "Target databases:".yellow().bold());
    let mut db_names: Vec<String> = db_summaries.keys().cloned().collect();
    db_names.sort();
    for db in &db_names {
        if let Some((count, bytes)) = db_summaries.get(db) {
            let mb = (*bytes as f64) / (1024.0 * 1024.0);
            println!("  {:<25} {:>4} tables  {:>10.2} MB", db.cyan(), count, mb);
        }
    }
    println!();

    if args.dry_run {
        println!(
            "{}",
            "[DRY RUN] Plan displayed. No files were written to disk."
                .green()
                .bold()
        );
        return Ok(());
    }

    // Ensure output and log directories exist before starting parallel export
    fs::create_dir_all(&args.dir).with_context(|| {
        format!(
            "Failed to create output directory: {}",
            args.dir.display()
        )
    })?;
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("Failed to create log directory: {}", log_dir.display()))?;

    println!(
        "{}",
        "==================================================".green()
    );
    println!("{}", " STARTING PARALLEL EXPORT".green().bold());
    println!(
        "{}",
        "==================================================".green()
    );

    let total_tasks = tasks.len();
    let tracker = Arc::new(ProgressTracker::new(
        total_tasks,
        remaining_bytes,
        args.resolved_workers(),
        args.no_progress,
    ));

    let tracker_callback = Arc::clone(&tracker);
    let summary = execute_export_tasks(
        &args,
        &mysqldump_bin,
        option_file.path(),
        tasks,
        manifest,
        tracker.live_states(),
        move |event, state| {
            tracker_callback.handle_event(event, state);
        },
    )?;

    tracker.finish();

    print_export_summary(&summary, &log_dir);

    if summary.cancelled || !summary.failed_tasks.is_empty() {
        bail!("Export completed with errors or was cancelled.");
    }

    Ok(())
}

fn collect_export_tasks(
    args: &ExportArgs,
    mysql_bin: &Path,
    option_file: &Path,
) -> Result<Vec<ExportTask>> {
    let mut query = String::from(
        "SELECT TABLE_SCHEMA, TABLE_NAME, COALESCE(DATA_LENGTH + INDEX_LENGTH, 0), COALESCE(TABLE_ROWS, 0) \
         FROM information_schema.TABLES \
         WHERE TABLE_TYPE = 'BASE TABLE' "
    );

    if let Some(ref db) = args.database {
        query.push_str(&format!("AND TABLE_SCHEMA = '{}' ", db.replace('\'', "''")));
    } else if let Some(ref dbs) = args.databases {
        let db_list: Vec<String> = dbs
            .split(',')
            .map(|s| format!("'{}'", s.trim().replace('\'', "''")))
            .collect();
        query.push_str(&format!("AND TABLE_SCHEMA IN ({}) ", db_list.join(",")));
    } else if args.all_databases {
        query.push_str(
            "AND TABLE_SCHEMA NOT IN ('information_schema', 'performance_schema', 'mysql', 'sys') ",
        );
    } else {
        bail!("Must specify a database to export using --database <NAME>, --databases <NAMES>, or --all-databases.");
    }

    if let Some(ref tbls) = args.tables {
        let tbl_list: Vec<String> = tbls
            .split(',')
            .map(|s| format!("'{}'", s.trim().replace('\'', "''")))
            .collect();
        query.push_str(&format!("AND TABLE_NAME IN ({}) ", tbl_list.join(",")));
    }

    if let Some(ref excl) = args.exclude_tables {
        let excl_list: Vec<String> = excl
            .split(',')
            .map(|s| format!("'{}'", s.trim().replace('\'', "''")))
            .collect();
        query.push_str(&format!("AND TABLE_NAME NOT IN ({}) ", excl_list.join(",")));
    }

    query.push(';');

    let output = Command::new(mysql_bin)
        .arg(format!("--defaults-extra-file={}", option_file.display()))
        .arg("-N")
        .arg("-B")
        .arg("-e")
        .arg(&query)
        .output()
        .with_context(|| "Failed to execute schema discovery query on MySQL server")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("MySQL schema discovery error: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut tasks = Vec::new();

    let single_db = args.database.is_some();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 4 {
            let db = parts[0].trim().to_string();
            let table = parts[1].trim().to_string();
            let bytes = parts[2].trim().parse::<u64>().unwrap_or(0);
            let rows = parts[3].trim().parse::<u64>().unwrap_or(0);

            let ext = if args.compress { "sql.gz" } else { "sql" };
            let file_name = if single_db {
                format!("{table}.{ext}")
            } else {
                format!("{db}__{table}.{ext}")
            };

            let output_path = args.dir.join(&file_name);

            tasks.push(ExportTask {
                database: db,
                table_name: table,
                file_name,
                estimated_bytes: bytes,
                _estimated_rows: rows,
                output_path,
            });
        }
    }

    Ok(tasks)
}

fn execute_export_tasks<F>(
    args: &ExportArgs,
    mysqldump_bin: &Path,
    option_file: &Path,
    tasks: Vec<ExportTask>,
    manifest: Arc<ManifestManager>,
    live_states: Vec<Arc<WorkerLiveState>>,
    event_callback: F,
) -> Result<RunSummary>
where
    F: Fn(WorkerEvent, &RunSummaryState) + Send + Sync + 'static,
{
    let total_tasks = tasks.len();
    let total_bytes: u64 = tasks.iter().map(|t| t.estimated_bytes).sum();
    let num_workers = args.resolved_workers().min(total_tasks.max(1));

    let (task_tx, task_rx) = unbounded::<ExportTask>();
    let (event_tx, event_rx) = unbounded::<WorkerEvent>();

    for task in tasks {
        task_tx.send(task).unwrap();
    }
    drop(task_tx);

    let cancelled = Arc::new(AtomicBool::new(false));
    let active_pids: Arc<Mutex<HashMap<usize, u32>>> = Arc::new(Mutex::new(HashMap::new()));

    // Signal handler for Ctrl+C
    {
        let cancelled_clone = Arc::clone(&cancelled);
        let active_pids_clone = Arc::clone(&active_pids);
        ctrlc::set_handler(move || {
            cancelled_clone.store(true, Ordering::SeqCst);
            let pids: Vec<u32> = {
                let guard = active_pids_clone.lock();
                guard.values().copied().collect()
            };
            for pid in pids {
                kill_os_pid(pid);
            }
        })
        .expect("Error setting Ctrl-C handler for export");
    }

    let start_time = Instant::now();
    let mut worker_handles = Vec::new();
    let log_dir = args.resolved_log_dir();

    for worker_id in 1..=num_workers {
        let task_rx = task_rx.clone();
        let event_tx = event_tx.clone();
        let mysqldump_bin_buf = mysqldump_bin.to_path_buf();
        let option_file_buf = option_file.to_path_buf();
        let args_clone = args.clone();
        let log_dir_clone = log_dir.clone();
        let manifest = Arc::clone(&manifest);
        let cancelled = Arc::clone(&cancelled);
        let active_pids = Arc::clone(&active_pids);
        let live_state = Arc::clone(&live_states[worker_id - 1]);

        let handle = thread::spawn(move || {
            while let Ok(task) = task_rx.recv() {
                if cancelled.load(Ordering::SeqCst) {
                    break;
                }

                live_state.start_task(&task.database, &task.table_name, task.estimated_bytes);

                let _ = event_tx.send(WorkerEvent::Started {
                    worker_id,
                    file_name: task.file_name.clone(),
                    database: task.database.clone(),
                    size_bytes: task.estimated_bytes,
                });

                let task_start = Instant::now();

                // Create standard Workbench/mysqldump header
                let header = format!(
                    "-- ------------------------------------------------------\n\
                     -- MySQL TurboLoad Enterprise Dump\n\
                     -- Host: {}    Database: {}\n\
                     -- ------------------------------------------------------\n\
                     -- Current Database: `{}`\n\
                     --\n\n",
                    args_clone.host, task.database, task.database
                );

                let mut cmd = Command::new(&mysqldump_bin_buf);
                cmd.arg(format!(
                    "--defaults-extra-file={}",
                    option_file_buf.display()
                ));
                cmd.arg("--single-transaction");
                cmd.arg("--quick");
                cmd.arg(format!(
                    "--max-allowed-packet={}",
                    args_clone.max_allowed_packet
                ));
                cmd.arg(format!(
                    "--net-buffer-length={}",
                    args_clone.net_buffer_length
                ));
                cmd.arg("--extended-insert");
                cmd.arg("--skip-add-locks");
                cmd.arg("--skip-lock-tables");
                cmd.arg("--default-character-set=utf8mb4");

                if args_clone.no_data {
                    cmd.arg("--no-data");
                }
                if args_clone.no_create_info {
                    cmd.arg("--no-create-info");
                }
                if args_clone.routines {
                    cmd.arg("--routines");
                }
                if args_clone.events {
                    cmd.arg("--events");
                }
                if args_clone.triggers {
                    cmd.arg("--triggers");
                }

                cmd.arg(&task.database);
                cmd.arg(&task.table_name);

                cmd.stdout(Stdio::piped());
                cmd.stderr(Stdio::piped());

                let mut child = match cmd.spawn() {
                    Ok(c) => c,
                    Err(e) => {
                        live_state.finish_task();
                        let log_path = log_dir_clone.join(format!(
                            "{}__{}.export.err.log",
                            task.database, task.table_name
                        ));
                        let _ = fs::write(&log_path, format!("Failed to spawn mysqldump: {e}"));
                        let _ = event_tx.send(WorkerEvent::Failed {
                            worker_id,
                            task: FailedTask {
                                file_path: task.output_path.clone(),
                                file_name: task.file_name.clone(),
                                database: task.database.clone(),
                                size_mb: (task.estimated_bytes as f64) / (1024.0 * 1024.0),
                                exit_code: None,
                                error_snippet: e.to_string(),
                                log_path,
                            },
                        });
                        continue;
                    }
                };

                let stdout_stream = child.stdout.take().unwrap();
                let live_counter = Arc::clone(&live_state.bytes_current);
                let live_committing = Arc::clone(&live_state);
                let counting_stdout = CountingReader::new(stdout_stream, live_counter);
                let out_path = task.output_path.clone();
                let header_content = header.clone();
                let is_compress = args_clone.compress;

                let writer_handle = thread::spawn(move || -> Result<()> {
                    let mut counting_reader = counting_stdout;
                    let mut transfer_buf = vec![0u8; 256 * 1024];

                    if is_compress {
                        let file = File::create(&out_path).with_context(|| {
                            format!("Failed to create output file: {}", out_path.display())
                        })?;
                        let buf_writer = std::io::BufWriter::with_capacity(256 * 1024, file);
                        let mut encoder = GzEncoder::new(buf_writer, Compression::default());
                        encoder.write_all(header_content.as_bytes())?;

                        loop {
                            let n = match std::io::Read::read(&mut counting_reader, &mut transfer_buf) {
                                Ok(0) => break,
                                Ok(n) => n,
                                Err(e) => {
                                    if e.kind() == std::io::ErrorKind::BrokenPipe {
                                        break;
                                    }
                                    return Err(e.into());
                                }
                            };
                            encoder.write_all(&transfer_buf[..n])?;
                        }
                        live_committing.mark_committing();
                        encoder.finish()?;
                    } else {
                        let file = File::create(&out_path).with_context(|| {
                            format!("Failed to create output file: {}", out_path.display())
                        })?;
                        let mut buf_writer = std::io::BufWriter::with_capacity(256 * 1024, file);
                        buf_writer.write_all(header_content.as_bytes())?;

                        loop {
                            let n = match std::io::Read::read(&mut counting_reader, &mut transfer_buf) {
                                Ok(0) => break,
                                Ok(n) => n,
                                Err(e) => {
                                    if e.kind() == std::io::ErrorKind::BrokenPipe {
                                        break;
                                    }
                                    return Err(e.into());
                                }
                            };
                            buf_writer.write_all(&transfer_buf[..n])?;
                        }
                        live_committing.mark_committing();
                        buf_writer.flush()?;
                    }

                    Ok(())
                });

                // Drain stderr asynchronously to avoid pipe deadlock
                let stderr_stream = child.stderr.take();
                let err_reader = thread::spawn(move || {
                    let mut buf = Vec::new();
                    if let Some(mut stream) = stderr_stream {
                        let _ = std::io::Read::read_to_end(&mut stream, &mut buf);
                    }
                    buf
                });

                let child_pid = child.id();
                {
                    let mut guard = active_pids.lock();
                    guard.insert(worker_id, child_pid);
                }

                let mut exit_status = None;
                loop {
                    if cancelled.load(Ordering::SeqCst) {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }

                    match child.try_wait() {
                        Ok(Some(status)) => {
                            exit_status = Some(status);
                            break;
                        }
                        Ok(None) => {
                            thread::sleep(Duration::from_millis(30));
                        }
                        Err(_) => break,
                    }
                }

                {
                    let mut guard = active_pids.lock();
                    guard.remove(&worker_id);
                }

                let stderr_bytes = err_reader.join().unwrap_or_default();
                let writer_res = match writer_handle.join() {
                    Ok(res) => res,
                    Err(_) => Err(anyhow::anyhow!("Writer thread panicked")),
                };

                live_state.finish_task();

                if cancelled.load(Ordering::SeqCst) {
                    let _ = fs::remove_file(&task.output_path);
                    break;
                }

                let elapsed_task = task_start.elapsed();

                match (exit_status, writer_res) {
                    (Some(status), Ok(())) if status.success() => {
                        let _ = manifest.mark_completed(&task.file_name);
                        let _ = event_tx.send(WorkerEvent::Finished {
                            worker_id,
                            file_name: task.file_name.clone(),
                            database: task.database.clone(),
                            size_bytes: task.estimated_bytes,
                            duration: elapsed_task,
                        });
                    }
                    (Some(status), writer_result) => {
                        let _ = fs::remove_file(&task.output_path);
                        let stderr_str = String::from_utf8_lossy(&stderr_bytes).to_string();
                        let log_path = log_dir_clone.join(format!(
                            "{}__{}.export.err.log",
                            task.database, task.table_name
                        ));

                        let writer_err_str = if let Err(e) = writer_result {
                            format!("\nSTREAM WRITER ERROR: {e:#}\n")
                        } else {
                            String::new()
                        };

                        let log_content = format!(
                            "================================================================================\n\
                             MYSQL EXPORT FAILURE\n\
                             Database:    {}\n\
                             Table:       {}\n\
                             Exit Code:   {:?}\n\
                             Time:        {}\n\
                             ================================================================================\n\
                             STDERR OUTPUT:\n\
                             {}{}\n\
                             ================================================================================\n",
                            task.database,
                            task.table_name,
                            status.code(),
                            Utc::now().to_rfc3339(),
                            stderr_str.trim(),
                            writer_err_str
                        );
                        let _ = fs::write(&log_path, log_content);

                        let snippet = if !writer_err_str.is_empty() {
                            "Export streaming error".to_string()
                        } else {
                            stderr_str
                                .lines()
                                .find(|l| l.contains("ERROR") || l.contains("Error"))
                                .unwrap_or(if stderr_str.trim().is_empty() {
                                    "Process exited with error"
                                } else {
                                    stderr_str.trim().lines().next().unwrap_or("Unknown error")
                                })
                                .to_string()
                        };

                        let _ = event_tx.send(WorkerEvent::Failed {
                            worker_id,
                            task: FailedTask {
                                file_path: task.output_path.clone(),
                                file_name: task.file_name.clone(),
                                database: task.database.clone(),
                                size_mb: (task.estimated_bytes as f64) / (1024.0 * 1024.0),
                                exit_code: status.code(),
                                error_snippet: snippet,
                                log_path,
                            },
                        });
                    }
                    (None, _) => {
                        let _ = fs::remove_file(&task.output_path);
                    }
                }
            }
        });
        worker_handles.push(handle);
    }
    drop(event_tx);

    let mut state = RunSummaryState {
        _total_tasks: total_tasks,
        completed_count: 0,
        failed_tasks: Vec::new(),
        total_bytes,
        completed_bytes: 0,
        worker_status: HashMap::new(),
    };

    while let Ok(event) = event_rx.recv() {
        match &event {
            WorkerEvent::Started {
                worker_id,
                file_name,
                database: _,
                size_bytes,
            } => {
                state.worker_status.insert(
                    *worker_id,
                    format!(
                        "{} ({:.1} MB)",
                        file_name,
                        (*size_bytes as f64) / (1024.0 * 1024.0)
                    ),
                );
            }
            WorkerEvent::Finished {
                worker_id,
                size_bytes,
                ..
            } => {
                state.completed_count += 1;
                state.completed_bytes += size_bytes;
                state.worker_status.remove(worker_id);
            }
            WorkerEvent::Failed { worker_id, task } => {
                state.failed_tasks.push(task.clone());
                state.worker_status.remove(worker_id);
            }
        }
        event_callback(event, &state);
    }

    for h in worker_handles {
        let _ = h.join();
    }

    let is_cancelled = cancelled.load(Ordering::SeqCst);
    let elapsed = start_time.elapsed();

    Ok(RunSummary {
        total_tasks,
        completed_count: state.completed_count,
        failed_tasks: state.failed_tasks,
        total_bytes,
        completed_bytes: state.completed_bytes,
        elapsed,
        cancelled: is_cancelled,
    })
}

fn print_export_summary(summary: &RunSummary, log_dir: &Path) {
    println!();
    println!(
        "{}",
        "==================================================".cyan()
    );
    if summary.cancelled {
        println!("{}", " EXPORT CANCELLED BY USER".yellow().bold());
    } else if summary.failed_tasks.is_empty() {
        println!("{}", " EXPORT FINISHED SUCCESSFULLY".green().bold());
    } else {
        println!("{}", " EXPORT FINISHED WITH ERRORS".red().bold());
    }
    println!(
        "{}",
        "==================================================".cyan()
    );

    let completed_mb = (summary.completed_bytes as f64) / (1024.0 * 1024.0);
    let total_secs = summary.elapsed.as_secs_f64().max(0.001);
    let mb_per_sec = completed_mb / total_secs;

    println!(
        "Tables Exported : {} / {}",
        summary.completed_count.to_string().green(),
        summary.total_tasks
    );
    if !summary.failed_tasks.is_empty() {
        println!(
            "Failed Tables   : {}",
            summary.failed_tasks.len().to_string().red().bold()
        );
    }
    println!(
        "Estimated Volume: {:.2} MB ({:.1} MB/s)",
        completed_mb, mb_per_sec
    );
    println!(
        "Elapsed Time    : {:02}:{:02}:{:02}",
        (summary.elapsed.as_secs() / 3600),
        (summary.elapsed.as_secs() % 3600) / 60,
        (summary.elapsed.as_secs() % 60)
    );

    if !summary.failed_tasks.is_empty() {
        println!();
        println!("{}", "FAILED TABLES SUMMARY:".red().bold());
        for task in &summary.failed_tasks {
            println!(
                "  {} [{}] {} ({:.2} MB, Exit {:?})",
                "•".red(),
                task.database.yellow(),
                task.file_name,
                task.size_mb,
                task.exit_code
            );
            println!("    Reason : {}", task.error_snippet.dimmed());
            println!("    Log    : {}", task.log_path.display());
        }

        let retry_manifest = log_dir.join("manifest.json");
        println!();
        println!(
            "{}",
            "TO RESUME AND FINISH REMAINING TABLES:".yellow().bold()
        );
        println!(
            "  mysql-turboload export --resume --manifest {}",
            retry_manifest.display()
        );
    }
    println!();
}
