use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::runner::{RunSummary, RunSummaryState, WorkerEvent};

/// Transparent wrapper around any `Read` stream that counts bytes read using an `AtomicU64`.
pub struct CountingReader<R> {
    inner: R,
    bytes_read: Arc<AtomicU64>,
}

impl<R> CountingReader<R> {
    pub fn new(inner: R, bytes_read: Arc<AtomicU64>) -> Self {
        Self { inner, bytes_read }
    }
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.bytes_read.fetch_add(n as u64, Ordering::Relaxed);
        }
        Ok(n)
    }
}

/// Thread-safe shared state for an individual active worker.
pub struct WorkerLiveState {
    pub worker_id: usize,
    pub is_active: AtomicBool,
    pub database: parking_lot::RwLock<String>,
    pub file_name: parking_lot::RwLock<String>,
    pub total_bytes: AtomicU64,
    pub bytes_current: Arc<AtomicU64>,
    pub start_time: parking_lot::RwLock<Option<Instant>>,
    pub is_committing: AtomicBool,
    pub smoothed_rate: parking_lot::RwLock<f64>, // MB/s
    pub last_sample_bytes: AtomicU64,
    pub last_sample_time: parking_lot::RwLock<Option<Instant>>,
}

impl WorkerLiveState {
    pub fn new(worker_id: usize) -> Self {
        Self {
            worker_id,
            is_active: AtomicBool::new(false),
            database: parking_lot::RwLock::new(String::new()),
            file_name: parking_lot::RwLock::new(String::new()),
            total_bytes: AtomicU64::new(0),
            bytes_current: Arc::new(AtomicU64::new(0)),
            start_time: parking_lot::RwLock::new(None),
            is_committing: AtomicBool::new(false),
            smoothed_rate: parking_lot::RwLock::new(0.0),
            last_sample_bytes: AtomicU64::new(0),
            last_sample_time: parking_lot::RwLock::new(None),
        }
    }

    pub fn start_task(&self, database: &str, file_name: &str, total_bytes: u64) {
        *self.database.write() = database.to_string();
        *self.file_name.write() = file_name.to_string();
        self.total_bytes.store(total_bytes, Ordering::SeqCst);
        self.bytes_current.store(0, Ordering::SeqCst);
        self.last_sample_bytes.store(0, Ordering::SeqCst);
        let now = Instant::now();
        *self.start_time.write() = Some(now);
        *self.last_sample_time.write() = Some(now);
        *self.smoothed_rate.write() = 0.0;
        self.is_committing.store(false, Ordering::SeqCst);
        self.is_active.store(true, Ordering::SeqCst);
    }

    pub fn finish_task(&self) {
        self.is_active.store(false, Ordering::SeqCst);
        self.is_committing.store(false, Ordering::SeqCst);
    }

    pub fn mark_committing(&self) {
        self.is_committing.store(true, Ordering::SeqCst);
    }
}

/// Calculates ETA based on remaining bytes and current throughput (bytes/sec).
pub fn calculate_eta(bytes_done: u64, total_bytes: u64, speed_bps: f64) -> Option<Duration> {
    if bytes_done >= total_bytes || speed_bps <= 10.0 {
        return None;
    }
    let remaining_bytes = total_bytes.saturating_sub(bytes_done);
    let seconds = (remaining_bytes as f64) / speed_bps;
    if seconds.is_nan() || seconds.is_infinite() || seconds > 86400.0 * 30.0 {
        None
    } else {
        Some(Duration::from_secs_f64(seconds))
    }
}

/// Formats duration into compact MM:SS or HH:MM:SS.
pub fn format_duration_compact(d: Duration) -> String {
    let secs = d.as_secs();
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let rem_secs = secs % 60;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{rem_secs:02}")
    } else {
        format!("{mins:02}:{rem_secs:02}")
    }
}


pub struct ProgressTracker {
    pub multi_progress: Option<MultiProgress>,
    pub main_bar: Option<ProgressBar>,
    pub worker_bars: Vec<ProgressBar>,
    pub no_progress: bool,
}

impl ProgressTracker {
    pub fn new(
        total_tasks: usize,
        _total_bytes: u64,
        num_workers: usize,
        no_progress: bool,
    ) -> Self {
        if no_progress {
            return Self {
                multi_progress: None,
                main_bar: None,
                worker_bars: Vec::new(),
                no_progress: true,
            };
        }

        let mp = MultiProgress::new();

        let style = ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:36.cyan/blue}] {pos}/{len} ({percent}%) | {msg}")
            .unwrap()
            .progress_chars("#>-");

        let pb = mp.add(ProgressBar::new(total_tasks as u64));
        pb.set_style(style);
        pb.enable_steady_tick(Duration::from_millis(150));
        pb.set_message("Starting workers...");

        let mut worker_bars = Vec::new();
        let worker_style = ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("  {spinner:.blue} [W{prefix}] {msg}")
            .unwrap();

        for i in 1..=num_workers {
            let wp = mp.add(ProgressBar::new_spinner());
            wp.set_style(worker_style.clone());
            wp.set_prefix(i.to_string());
            wp.set_message("Waiting for task...".dimmed().to_string());
            wp.enable_steady_tick(Duration::from_millis(120));
            worker_bars.push(wp);
        }

        Self {
            multi_progress: Some(mp),
            main_bar: Some(pb),
            worker_bars,
            no_progress: false,
        }
    }

    pub fn handle_event(&self, event: WorkerEvent, state: &RunSummaryState) {
        if self.no_progress {
            match event {
                WorkerEvent::Started {
                    worker_id,
                    file_name,
                    database,
                    size_bytes,
                } => {
                    let mb = (size_bytes as f64) / (1024.0 * 1024.0);
                    println!("[W{worker_id}] START {database} / {file_name} [{mb:.1} MB]");
                }
                WorkerEvent::Finished {
                    worker_id,
                    file_name,
                    database,
                    size_bytes,
                    duration,
                } => {
                    let mb = (size_bytes as f64) / (1024.0 * 1024.0);
                    println!(
                        "[W{worker_id}] DONE  {database} / {file_name} [{mb:.1} MB] in {duration:.1?}"
                    );
                }
                WorkerEvent::Failed { worker_id, task } => {
                    eprintln!(
                        "{}",
                        format!(
                            "[W{worker_id}] FAILED {task_db} / {task_file} : {snippet}",
                            task_db = task.database,
                            task_file = task.file_name,
                            snippet = task.error_snippet
                        )
                        .red()
                    );
                }
            }
            return;
        }

        match &event {
            WorkerEvent::Started {
                worker_id,
                file_name,
                database,
                size_bytes,
            } => {
                if let Some(wp) = self.worker_bars.get(worker_id - 1) {
                    let mb = (*size_bytes as f64) / (1024.0 * 1024.0);
                    wp.set_message(format!(
                        "{} / {} ({:.1} MB)",
                        database.cyan(),
                        file_name,
                        mb
                    ));
                }
            }
            WorkerEvent::Finished {
                worker_id,
                file_name,
                size_bytes,
                duration,
                ..
            } => {
                if let Some(wp) = self.worker_bars.get(worker_id - 1) {
                    let mb = (*size_bytes as f64) / (1024.0 * 1024.0);
                    wp.set_message(format!(
                        "{} ({:.1} MB in {:02}:{:02}) - next task...",
                        file_name.dimmed(),
                        mb,
                        duration.as_secs() / 60,
                        duration.as_secs() % 60
                    ));
                }
            }
            WorkerEvent::Failed { worker_id, task } => {
                if let Some(wp) = self.worker_bars.get(worker_id - 1) {
                    wp.set_message(format!("FAILED {}", task.file_name).red().to_string());
                }
                if let Some(ref mp) = self.multi_progress {
                    let _ = mp.println(format!(
                        "{} [W{}] Failed {}/{} (Exit: {:?}): {}",
                        "✖".red().bold(),
                        worker_id,
                        task.database,
                        task.file_name,
                        task.exit_code,
                        task.error_snippet
                    ));
                }
            }
        }

        if let Some(ref pb) = self.main_bar {
            let processed = state.completed_count + state.failed_tasks.len();
            pb.set_position(processed as u64);

            let mb_done = (state.completed_bytes as f64) / (1024.0 * 1024.0);
            let mb_total = (state.total_bytes as f64) / (1024.0 * 1024.0);

            let active_count = state.worker_status.len();
            let msg = if state.failed_tasks.is_empty() {
                format!("{mb_done:.1}/{mb_total:.1} MB | {active_count} active")
            } else {
                format!(
                    "{mb_done:.1}/{mb_total:.1} MB | {active_count} active | {} failed",
                    state.failed_tasks.len().to_string().red()
                )
            };
            pb.set_message(msg);
        }
    }

    pub fn finish(&self) {
        for wp in &self.worker_bars {
            wp.finish_and_clear();
        }
        if let Some(ref pb) = self.main_bar {
            pb.finish_with_message("Completed");
        }
    }
}

pub fn print_summary(summary: &RunSummary, log_dir: &std::path::Path) {
    println!();
    println!(
        "{}",
        "==================================================".cyan()
    );
    if summary.cancelled {
        println!("{}", " IMPORT CANCELLED BY USER".yellow().bold());
    } else if summary.failed_tasks.is_empty() {
        println!("{}", " IMPORT FINISHED SUCCESSFULLY".green().bold());
    } else {
        println!("{}", " IMPORT FINISHED WITH ERRORS".red().bold());
    }
    println!(
        "{}",
        "==================================================".cyan()
    );

    let total_mb = (summary.total_bytes as f64) / (1024.0 * 1024.0);
    let completed_mb = (summary.completed_bytes as f64) / (1024.0 * 1024.0);
    let total_secs = summary.elapsed.as_secs_f64().max(0.001);
    let mb_per_sec = completed_mb / total_secs;

    println!(
        "Tables Imported : {} / {}",
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
        "Volume Processed: {:.2} MB / {:.2} MB ({:.1} MB/s)",
        completed_mb, total_mb, mb_per_sec
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

        let retry_file = log_dir.join("failed_files.txt");
        println!();
        println!("{}", "TO RETRY ONLY FAILED TABLES:".yellow().bold());
        println!("  mysql-turboload --retry-file {}", retry_file.display());
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_counting_reader_tracks_exact_bytes() {
        let data = b"Hello, MySQL TurboLoad Enterprise!";
        let counter = Arc::new(AtomicU64::new(0));
        let mut reader = CountingReader::new(Cursor::new(data), Arc::clone(&counter));
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut buf).unwrap();
        assert_eq!(counter.load(Ordering::Relaxed), data.len() as u64);
        assert_eq!(buf, data);
    }

    #[test]
    fn test_eta_calculation() {
        // 50 MB done out of 100 MB at 10 MB/s -> exactly 5 seconds ETA
        let eta = calculate_eta(50 * 1024 * 1024, 100 * 1024 * 1024, 10.0 * 1024.0 * 1024.0);
        assert_eq!(eta.map(|d| d.as_secs()), Some(5));

        // When done >= total -> None
        assert_eq!(calculate_eta(100, 100, 1000.0), None);
        assert_eq!(calculate_eta(105, 100, 1000.0), None);

        // When speed is zero or negligible -> None
        assert_eq!(calculate_eta(50, 100, 0.0), None);
    }

    #[test]
    fn test_format_duration_compact() {
        assert_eq!(format_duration_compact(Duration::from_secs(5)), "00:05");
        assert_eq!(format_duration_compact(Duration::from_secs(65)), "01:05");
        assert_eq!(format_duration_compact(Duration::from_secs(3665)), "01:01:05");
    }

    #[test]
    fn test_worker_live_state_lifecycle() {
        let state = WorkerLiveState::new(1);
        assert!(!state.is_active.load(Ordering::SeqCst));
        assert!(!state.is_committing.load(Ordering::SeqCst));

        state.start_task("mydb", "table.sql.gz", 1000);
        assert!(state.is_active.load(Ordering::SeqCst));
        assert_eq!(*state.database.read(), "mydb");
        assert_eq!(*state.file_name.read(), "table.sql.gz");
        assert_eq!(state.total_bytes.load(Ordering::SeqCst), 1000);

        state.mark_committing();
        assert!(state.is_committing.load(Ordering::SeqCst));

        state.finish_task();
        assert!(!state.is_active.load(Ordering::SeqCst));
        assert!(!state.is_committing.load(Ordering::SeqCst));
    }
}

