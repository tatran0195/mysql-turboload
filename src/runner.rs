use anyhow::Result;
use chrono::Utc;
use crossbeam_channel::unbounded;
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

use crate::cli::ImportArgs;
use crate::manifest::ManifestManager;
use crate::scanner::TableTask;

#[derive(Debug, Clone)]
pub struct FailedTask {
    pub file_path: PathBuf,
    pub file_name: String,
    pub database: String,
    pub size_mb: f64,
    pub exit_code: Option<i32>,
    pub error_snippet: String,
    pub log_path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum WorkerEvent {
    Started {
        worker_id: usize,
        file_name: String,
        database: String,
        size_bytes: u64,
    },
    Finished {
        worker_id: usize,
        file_name: String,
        database: String,
        size_bytes: u64,
        duration: Duration,
    },
    Failed {
        worker_id: usize,
        task: FailedTask,
    },
}

pub struct RunSummary {
    pub total_tasks: usize,
    pub completed_count: usize,
    pub failed_tasks: Vec<FailedTask>,
    pub total_bytes: u64,
    pub completed_bytes: u64,
    pub elapsed: Duration,
    pub cancelled: bool,
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

pub fn execute_tasks<F>(
    cli: &ImportArgs,
    mysql_bin: &Path,
    option_file: &Path,
    tasks: Vec<TableTask>,
    manifest: Arc<ManifestManager>,
    event_callback: F,
) -> Result<RunSummary>
where
    F: Fn(WorkerEvent, &RunSummaryState) + Send + Sync + 'static,
{
    let total_tasks = tasks.len();
    let total_bytes: u64 = tasks.iter().map(|t| t.size_bytes).sum();
    let num_workers = cli.resolved_workers().min(total_tasks.max(1));

    // Dynamic Multi-Producer Multi-Consumer (MPMC) Work Queue.
    // Unlike slice iterators, an MPMC queue ensures all workers dynamically pull
    // the next available task from a shared pool. If Worker 1 takes hours on a 6GB file,
    // Workers 2, 3, and 4 will continuously pull and finish the remaining 161 files
    // without ever sitting idle!
    let (task_tx, task_rx) = unbounded::<TableTask>();
    let (event_tx, event_rx) = unbounded::<WorkerEvent>();

    for task in tasks {
        task_tx.send(task).unwrap();
    }
    drop(task_tx); // Close queue so workers terminate when queue is empty

    // Shared cancellation flag and active process tracking via parking_lot
    let cancelled = Arc::new(AtomicBool::new(false));
    let active_pids: Arc<Mutex<HashMap<usize, u32>>> = Arc::new(Mutex::new(HashMap::new()));

    // Ctrl+C Signal Handler
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
        .expect("Error setting Ctrl-C handler");
    }

    // Ensure logs directory exists
    fs::create_dir_all(&cli.log_dir)?;

    let start_time = Instant::now();

    // Spawn dedicated worker threads pulling dynamically from the shared channel
    let mut worker_handles = Vec::new();
    for worker_id in 1..=num_workers {
        let task_rx = task_rx.clone();
        let event_tx = event_tx.clone();
        let mysql_bin_buf = mysql_bin.to_path_buf();
        let option_file_buf = option_file.to_path_buf();
        let cli_clone = cli.clone();
        let manifest = Arc::clone(&manifest);
        let cancelled = Arc::clone(&cancelled);
        let active_pids = Arc::clone(&active_pids);

        let handle = thread::spawn(move || {
            while let Ok(task) = task_rx.recv() {
                if cancelled.load(Ordering::SeqCst) {
                    break;
                }

                // Notify UI that task has started
                let _ = event_tx.send(WorkerEvent::Started {
                    worker_id,
                    file_name: task.file_name.clone(),
                    database: task.database.clone(),
                    size_bytes: task.size_bytes,
                });

                let task_start = Instant::now();

                // Open SQL file to stream directly into stdin (zero-copy kernel pipe)
                let file = match File::open(&task.file_path) {
                    Ok(f) => f,
                    Err(e) => {
                        let log_path = cli_clone
                            .log_dir
                            .join(format!("{}__{}.err.log", task.database, task.file_name));
                        let _ = fs::write(
                            &log_path,
                            format!(
                                "Failed to open file: {}\nError: {}",
                                task.file_path.display(),
                                e
                            ),
                        );
                        let _ = event_tx.send(WorkerEvent::Failed {
                            worker_id,
                            task: FailedTask {
                                file_path: task.file_path.clone(),
                                file_name: task.file_name.clone(),
                                database: task.database.clone(),
                                size_mb: task.size_mb,
                                exit_code: None,
                                error_snippet: e.to_string(),
                                log_path,
                            },
                        });
                        continue;
                    }
                };

                // Prepare MySQL client command using option file
                let mut cmd = Command::new(&mysql_bin_buf);
                cmd.arg(format!(
                    "--defaults-extra-file={}",
                    option_file_buf.display()
                ));
                cmd.arg(format!("--database={}", task.database));
                cmd.arg(format!(
                    "--max_allowed_packet={}",
                    cli_clone.max_allowed_packet
                ));
                cmd.arg("--net_buffer_length=1M");
                cmd.arg(format!("--default-character-set={}", cli_clone.charset));
                cmd.arg("--quick");
                cmd.arg("--binary-mode");

                if !cli_clone.no_disable_foreign_keys {
                    cmd.arg("--init-command=SET SESSION foreign_key_checks=0; SET SESSION unique_checks=0; SET SESSION sql_log_bin=0; SET SESSION sql_mode='NO_AUTO_VALUE_ON_ZERO'; SET SESSION transaction_isolation='READ-UNCOMMITTED';");
                }

                let is_gz = task
                    .file_path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("gz"));

                let (stdin_cfg, file_for_gz) = if is_gz {
                    (Stdio::piped(), Some(file))
                } else {
                    (Stdio::from(file), None)
                };

                cmd.stdin(stdin_cfg);
                cmd.stdout(Stdio::null());
                cmd.stderr(Stdio::piped());

                let mut child = match cmd.spawn() {
                    Ok(c) => c,
                    Err(e) => {
                        let log_path = cli_clone
                            .log_dir
                            .join(format!("{}__{}.err.log", task.database, task.file_name));
                        let _ = fs::write(&log_path, format!("Failed to spawn mysql: {}", e));
                        let _ = event_tx.send(WorkerEvent::Failed {
                            worker_id,
                            task: FailedTask {
                                file_path: task.file_path.clone(),
                                file_name: task.file_name.clone(),
                                database: task.database.clone(),
                                size_mb: task.size_mb,
                                exit_code: None,
                                error_snippet: e.to_string(),
                                log_path,
                            },
                        });
                        continue;
                    }
                };

                let feeder_handle = if let Some(gz_file) = file_for_gz {
                    let mut child_stdin = child.stdin.take().unwrap();
                    Some(thread::spawn(move || -> Result<(), std::io::Error> {
                        let mut decoder = flate2::read::GzDecoder::new(gz_file);
                        let mut buf = [0u8; 65536];
                        loop {
                            let n = std::io::Read::read(&mut decoder, &mut buf)?;
                            if n == 0 {
                                break;
                            }
                            if child_stdin.write_all(&buf[..n]).is_err() {
                                break;
                            }
                        }
                        Ok(())
                    }))
                } else {
                    None
                };

                // Drain stderr asynchronously in a dedicated reader thread so pipe buffer never deadlocks
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

                // Poll loop with cancellation checking
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
                        Err(_) => {
                            break;
                        }
                    }
                }

                // Remove from active PID registry
                {
                    let mut guard = active_pids.lock();
                    guard.remove(&worker_id);
                }

                let stderr_bytes = err_reader.join().unwrap_or_default();
                if let Some(h) = feeder_handle {
                    let _ = h.join();
                }

                if cancelled.load(Ordering::SeqCst) {
                    break;
                }

                let elapsed_task = task_start.elapsed();

                match exit_status {
                    Some(status) if status.success() => {
                        let _ = manifest.mark_completed(&task.file_name);
                        let _ = event_tx.send(WorkerEvent::Finished {
                            worker_id,
                            file_name: task.file_name.clone(),
                            database: task.database.clone(),
                            size_bytes: task.size_bytes,
                            duration: elapsed_task,
                        });
                    }
                    Some(status) => {
                        let stderr_str = String::from_utf8_lossy(&stderr_bytes).to_string();

                        let log_path = cli_clone
                            .log_dir
                            .join(format!("{}__{}.err.log", task.database, task.file_name));

                        let log_content = format!(
                            "================================================================================\n\
                             MYSQL IMPORT FAILURE\n\
                             Database:    {}\n\
                             File:        {}\n\
                             Size:        {:.2} MB\n\
                             Exit Code:   {:?}\n\
                             Time:        {}\n\
                             ================================================================================\n\
                             STDERR OUTPUT:\n\
                             {}\n\
                             ================================================================================\n",
                            task.database,
                            task.file_path.display(),
                            task.size_mb,
                            status.code(),
                            Utc::now().to_rfc3339(),
                            stderr_str.trim()
                        );

                        let _ = fs::write(&log_path, log_content);

                        let snippet = stderr_str
                            .lines()
                            .find(|l| l.contains("ERROR") || l.contains("Error"))
                            .unwrap_or(if stderr_str.trim().is_empty() {
                                "Process exited with error (no stderr output)"
                            } else {
                                stderr_str.trim().lines().next().unwrap_or("Unknown error")
                            })
                            .to_string();

                        let _ = event_tx.send(WorkerEvent::Failed {
                            worker_id,
                            task: FailedTask {
                                file_path: task.file_path.clone(),
                                file_name: task.file_name.clone(),
                                database: task.database.clone(),
                                size_mb: task.size_mb,
                                exit_code: status.code(),
                                error_snippet: snippet,
                                log_path,
                            },
                        });
                    }
                    None => {}
                }
            }
        });
        worker_handles.push(handle);
    }
    drop(event_tx); // Drop outer sender so event_rx terminates when all workers finish

    // Main coordinator state
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

    // Wait for all worker threads to cleanly exit
    for h in worker_handles {
        let _ = h.join();
    }

    let is_cancelled = cancelled.load(Ordering::SeqCst);
    let elapsed = start_time.elapsed();

    // If there were failed tasks, generate failed_files.txt for easy retrying
    if !state.failed_tasks.is_empty() {
        let retry_list_path = cli.log_dir.join("failed_files.txt");
        if let Ok(mut f) = File::create(&retry_list_path) {
            for failed in &state.failed_tasks {
                let _ = writeln!(f, "{}", failed.file_path.display());
            }
        }
    }

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

#[derive(Debug, Clone)]
pub struct RunSummaryState {
    pub _total_tasks: usize,
    pub completed_count: usize,
    pub failed_tasks: Vec<FailedTask>,
    pub total_bytes: u64,
    pub completed_bytes: u64,
    pub worker_status: HashMap<usize, String>,
}
