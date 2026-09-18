use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::time::Duration;

use crate::runner::{RunSummary, RunSummaryState, WorkerEvent};

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
