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
    #[allow(dead_code)]
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
    pub live_states: Vec<Arc<WorkerLiveState>>,
    pub completed_bytes: Arc<AtomicU64>,
    pub failed_count: Arc<AtomicU64>,
    pub stop_ticker: Arc<AtomicBool>,
    pub ticker_handle: parking_lot::Mutex<Option<std::thread::JoinHandle<()>>>,
    pub no_progress: bool,
}

impl ProgressTracker {
    pub fn new(
        total_tasks: usize,
        total_bytes: u64,
        num_workers: usize,
        no_progress: bool,
    ) -> Self {
        let mut live_states = Vec::new();
        for i in 1..=num_workers {
            live_states.push(Arc::new(WorkerLiveState::new(i)));
        }

        if no_progress {
            return Self {
                multi_progress: None,
                main_bar: None,
                worker_bars: Vec::new(),
                live_states,
                completed_bytes: Arc::new(AtomicU64::new(0)),
                failed_count: Arc::new(AtomicU64::new(0)),
                stop_ticker: Arc::new(AtomicBool::new(false)),
                ticker_handle: parking_lot::Mutex::new(None),
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

        let completed_bytes = Arc::new(AtomicU64::new(0));
        let failed_count = Arc::new(AtomicU64::new(0));
        let stop_ticker = Arc::new(AtomicBool::new(false));

        // Background sampling ticker: reads lock-free atomics every 200ms and updates UI
        let stop_clone = Arc::clone(&stop_ticker);
        let live_states_clone = live_states.clone();
        let worker_bars_clone = worker_bars.clone();
        let main_bar_clone = pb.clone();
        let completed_bytes_ref = Arc::clone(&completed_bytes);
        let failed_count_ref = Arc::clone(&failed_count);

        let ticker_thread = std::thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(200));
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }

                let mut max_active_eta: Option<Duration> = None;
                let mut active_count = 0usize;
                let mut live_active_bytes = 0u64;

                for (i, wp) in worker_bars_clone.iter().enumerate() {
                    if let Some(state) = live_states_clone.get(i) {
                        if !state.is_active.load(Ordering::Relaxed) {
                            continue;
                        }
                        active_count += 1;

                        let db = state.database.read().clone();
                        let file = state.file_name.read().clone();
                        let total = state.total_bytes.load(Ordering::Relaxed);
                        let current = state.bytes_current.load(Ordering::Relaxed);
                        let is_committing = state.is_committing.load(Ordering::Relaxed);
                        let start_opt = *state.start_time.read();

                        live_active_bytes += current;

                        let elapsed = start_opt.map_or(Duration::from_secs(0), |s| s.elapsed());

                        // Update EWMA rolling speed
                        let now = Instant::now();
                        let prev_sample_bytes =
                            state.last_sample_bytes.swap(current, Ordering::Relaxed);
                        let prev_sample_time = state.last_sample_time.write().replace(now);

                        if let Some(prev_time) = prev_sample_time {
                            let dt = (now - prev_time).as_secs_f64();
                            if dt >= 0.1 {
                                let d_bytes = current.saturating_sub(prev_sample_bytes);
                                let inst_rate_mb = (d_bytes as f64) / (1024.0 * 1024.0) / dt;
                                let mut rate = state.smoothed_rate.write();
                                *rate = if *rate <= 0.001 {
                                    inst_rate_mb
                                } else {
                                    0.25 * inst_rate_mb + 0.75 * (*rate)
                                };
                            }
                        }

                        let rate_mb = *state.smoothed_rate.read();
                        let speed_bps = rate_mb * 1024.0 * 1024.0;

                        if is_committing || (total > 0 && current >= total) {
                            let elapsed_str = format_duration_compact(elapsed);
                            let total_mb = (total as f64) / (1024.0 * 1024.0);
                            wp.set_message(format!(
                                "{}/{} ({:.1} MB - 100%) [{}]",
                                db.cyan(),
                                file,
                                total_mb,
                                format!("Finalizing in MySQL... ({elapsed_str})")
                                    .yellow()
                                    .bold()
                            ));
                        } else {
                            let cur_mb = (current as f64) / (1024.0 * 1024.0);
                            let tot_mb = (total as f64) / (1024.0 * 1024.0);
                            let pct = if total > 0 {
                                ((current as f64) / (total as f64) * 100.0).min(99.9)
                            } else {
                                0.0
                            };

                            let eta_opt = calculate_eta(current, total, speed_bps);
                            if let Some(eta) = eta_opt {
                                if max_active_eta.map_or(true, |cur_max| eta > cur_max) {
                                    max_active_eta = Some(eta);
                                }
                            }

                            let rate_str = if elapsed.as_secs_f64() < 3.0 {
                                "[Calculating ETA...]".dimmed().to_string()
                            } else if rate_mb < 0.05 && elapsed.as_secs_f64() > 5.0 {
                                format!("[{:.1} MB/s | Stalled on DB I/O]", rate_mb)
                                    .yellow()
                                    .to_string()
                            } else if let Some(eta) = eta_opt {
                                format!(
                                    "[{:.1} MB/s | ETA: {}]",
                                    rate_mb,
                                    format_duration_compact(eta)
                                )
                            } else {
                                format!("[{:.1} MB/s]", rate_mb)
                            };

                            wp.set_message(format!(
                                "{}/{} : {:.1}/{:.1} MB ({:.0}%) {}",
                                db.cyan(),
                                file,
                                cur_mb,
                                tot_mb,
                                pct,
                                rate_str
                            ));
                        }
                    }
                }

                // Update main bar message with live streaming MB and critical-path overall ETA
                let done_from_finished = completed_bytes_ref.load(Ordering::Relaxed);
                let total_done_mb =
                    ((done_from_finished + live_active_bytes) as f64) / (1024.0 * 1024.0);
                let total_mb_val = (total_bytes as f64) / (1024.0 * 1024.0);
                let failed_count_val = failed_count_ref.load(Ordering::Relaxed);

                let failed_str = if failed_count_val > 0 {
                    format!(" | {} failed", failed_count_val.to_string().red())
                } else {
                    String::new()
                };

                let eta_str = if let Some(eta) = max_active_eta {
                    format!(" | ETA: {}", format_duration_compact(eta).cyan().bold())
                } else {
                    String::new()
                };

                let msg = format!(
                    "{:.1}/{:.1} MB | {} active{}{}",
                    total_done_mb, total_mb_val, active_count, eta_str, failed_str
                );
                main_bar_clone.set_message(msg);
            }
        });

        Self {
            multi_progress: Some(mp),
            main_bar: Some(pb),
            worker_bars,
            live_states,
            completed_bytes,
            failed_count,
            stop_ticker,
            ticker_handle: parking_lot::Mutex::new(Some(ticker_thread)),
            no_progress: false,
        }
    }

    pub fn live_states(&self) -> Vec<Arc<WorkerLiveState>> {
        self.live_states.clone()
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
            WorkerEvent::Started { .. } => {}
            WorkerEvent::Finished {
                worker_id,
                file_name,
                size_bytes,
                duration,
                ..
            } => {
                self.completed_bytes
                    .fetch_add(*size_bytes, Ordering::SeqCst);
                if let Some(wp) = self.worker_bars.get(worker_id - 1) {
                    let mb = (*size_bytes as f64) / (1024.0 * 1024.0);
                    wp.set_message(format!(
                        "{} ({:.1} MB in {}) - next task...",
                        file_name.dimmed(),
                        mb,
                        format_duration_compact(*duration)
                    ));
                }
            }
            WorkerEvent::Failed { worker_id, task } => {
                self.failed_count.fetch_add(1, Ordering::SeqCst);
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
        }
    }

    pub fn finish(&self) {
        self.stop_ticker.store(true, Ordering::SeqCst);
        if let Some(handle) = self.ticker_handle.lock().take() {
            let _ = handle.join();
        }
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
        println!("  zephyr --retry-file {}", retry_file.display());
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

