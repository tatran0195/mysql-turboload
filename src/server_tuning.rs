use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::cli::ImportArgs;

pub struct ServerTuner<'a> {
    cli: &'a ImportArgs,
    mysql_bin: &'a Path,
    option_file: &'a Path,
    original_settings: HashMap<String, String>,
    is_tuned: bool,
}

impl<'a> ServerTuner<'a> {
    pub fn new(cli: &'a ImportArgs, mysql_bin: &'a Path, option_file: &'a Path) -> Self {
        Self {
            cli,
            mysql_bin,
            option_file,
            original_settings: HashMap::new(),
            is_tuned: false,
        }
    }

    /// Queries current global settings and temporarily tunes them for maximum bulk import throughput.
    pub fn apply(&mut self) {
        if !self.cli.tune_server && self.cli.buffer_pool_size.is_none() {
            return;
        }

        let spinner = if !self.cli.no_progress {
            let sp = ProgressBar::new_spinner();
            sp.set_style(
                ProgressStyle::default_spinner()
                    .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                    .template("{spinner:.yellow} {msg}")
                    .unwrap(),
            );
            sp.set_message("Applying server optimizations (--tune-server, buffer pool, IOPS)...");
            sp.enable_steady_tick(Duration::from_millis(80));
            Some(sp)
        } else {
            println!("Applying server optimizations (--tune-server, buffer pool, IOPS)...");
            None
        };

        // 1. FAST QUERY: original values for all relevant variables
        let query_vars = [
            "innodb_flush_log_at_trx_commit",
            "sync_binlog",
            "innodb_io_capacity",
            "innodb_io_capacity_max",
            "innodb_change_buffer_max_size",
            "innodb_buffer_pool_size",
            "innodb_doublewrite",
        ];

        for var in &query_vars {
            let sql = format!("SELECT @@GLOBAL.{};", var);
            if let Some(val) = self.run_query_with_timeout(&sql, Duration::from_secs(2)) {
                let trimmed = val.trim().to_string();
                if !trimmed.is_empty() {
                    self.original_settings.insert(var.to_string(), trimmed);
                }
            }
        }

        let mut applied_notes = Vec::new();

        // 2. CORE LOGGING & IO TUNING:
        // Sets flush_log=2, sync_binlog=0, io_capacity=2500, io_capacity_max=5000, change_buffer=50
        if self.cli.tune_server {
            let io_cap = self.cli.io_capacity.unwrap_or(2500);
            let io_cap_max = io_cap * 2;
            let core_cmd = format!(
                "SET GLOBAL innodb_flush_log_at_trx_commit = 2; \
                 SET GLOBAL sync_binlog = 0; \
                 SET GLOBAL innodb_io_capacity = {}; \
                 SET GLOBAL innodb_io_capacity_max = {}; \
                 SET GLOBAL innodb_change_buffer_max_size = 50;",
                io_cap, io_cap_max
            );

            if self.run_cmd_with_timeout(&core_cmd, Duration::from_secs(4)) {
                self.is_tuned = true;
                applied_notes.push(format!(
                    "flush_log=2, sync_binlog=0, IOPS={}/{}",
                    io_cap, io_cap_max
                ));
            }
        }

        // 3. BUFFER POOL TUNING
        let target_bytes = if let Some(ref bp_str) = self.cli.buffer_pool_size {
            parse_size_to_bytes(bp_str)
        } else if self.cli.tune_server {
            // Default with --tune-server: expand to 4GB if currently small (<= 256MB)
            let curr_bytes = self
                .original_settings
                .get("innodb_buffer_pool_size")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(134217728);

            if curr_bytes <= 268435456 {
                Some(4 * 1024 * 1024 * 1024)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(bytes) = target_bytes {
            let bp_query = format!("SET GLOBAL innodb_buffer_pool_size = {};", bytes);
            if self.run_cmd_with_timeout(&bp_query, Duration::from_secs(5)) {
                self.is_tuned = true;
                applied_notes.push(format!(
                    "buffer_pool={:.1}GB",
                    (bytes as f64) / (1024.0 * 1024.0 * 1024.0)
                ));
            } else {
                eprintln!(
                    "  Notice: Buffer pool resize timed out or busy. Continuing with current pool."
                );
            }
        }

        // 4. ADVANCED TUNING: Redo log capacity (2GB on MySQL 8.0.30+) & Doublewrite (0 on MySQL 8.0.20+)
        // Run with short timeout so older versions or restrictions don't block
        let _ = self.run_cmd_with_timeout(
            "SET GLOBAL innodb_redo_log_capacity = 2147483648;",
            Duration::from_secs(2),
        );
        let _ =
            self.run_cmd_with_timeout("SET GLOBAL innodb_doublewrite = 0;", Duration::from_secs(2));

        if let Some(sp) = spinner {
            sp.finish_and_clear();
        }

        if !applied_notes.is_empty() {
            println!(
                "  {} Server tuning applied: {}",
                "⚡".yellow().bold(),
                applied_notes.join(", ").cyan()
            );
        } else {
            println!("  Notice: Server tuning skipped or already optimized.");
        }
    }

    fn run_query_with_timeout(&self, sql: &str, timeout: Duration) -> Option<String> {
        let mut cmd = Command::new(self.mysql_bin);
        cmd.arg(format!(
            "--defaults-extra-file={}",
            self.option_file.display()
        ));
        cmd.arg("-s");
        cmd.arg("-N");
        cmd.arg("-e");
        cmd.arg(sql);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());

        let mut child = cmd.spawn().ok()?;
        let start = Instant::now();

        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => {
                    let mut out = String::new();
                    if let Some(mut stream) = child.stdout.take() {
                        let _ = std::io::Read::read_to_string(&mut stream, &mut out);
                    }
                    return Some(out);
                }
                Ok(Some(_)) => return None,
                Ok(None) => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        return None;
                    }
                    thread::sleep(Duration::from_millis(30));
                }
                Err(_) => return None,
            }
        }
    }

    fn run_cmd_with_timeout(&self, sql: &str, timeout: Duration) -> bool {
        let mut cmd = Command::new(self.mysql_bin);
        cmd.arg(format!(
            "--defaults-extra-file={}",
            self.option_file.display()
        ));
        cmd.arg("-e");
        cmd.arg(sql);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(_) => return false,
        };
        let start = Instant::now();

        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status.success(),
                Ok(None) => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        return false;
                    }
                    thread::sleep(Duration::from_millis(30));
                }
                Err(_) => return false,
            }
        }
    }

    /// Restores the original server settings when import finishes or on Ctrl+C.
    pub fn restore(&mut self) {
        if !self.is_tuned {
            return;
        }

        let mut stmts = Vec::new();
        for (var, val) in &self.original_settings {
            stmts.push(format!("SET GLOBAL {} = {};", var, val));
        }

        if !stmts.is_empty() {
            let restore_query = stmts.join(" ");
            let mut cmd = Command::new(self.mysql_bin);
            cmd.arg(format!(
                "--defaults-extra-file={}",
                self.option_file.display()
            ));
            cmd.arg("-e");
            cmd.arg(&restore_query);
            cmd.stdin(Stdio::null());
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());

            let _ = cmd.output();
            println!(
                "  {} Restored {} original server variables.",
                "✔".green(),
                self.original_settings.len()
            );
        }

        self.is_tuned = false;
    }
}

pub fn parse_size_to_bytes(s: &str) -> Option<u64> {
    let s = s.trim().to_uppercase();
    if let Some(num) = s.strip_suffix("GB").or_else(|| s.strip_suffix('G')) {
        num.trim()
            .parse::<u64>()
            .ok()
            .map(|n| n * 1024 * 1024 * 1024)
    } else if let Some(num) = s.strip_suffix("MB").or_else(|| s.strip_suffix('M')) {
        num.trim().parse::<u64>().ok().map(|n| n * 1024 * 1024)
    } else if let Some(num) = s.strip_suffix("KB").or_else(|| s.strip_suffix('K')) {
        num.trim().parse::<u64>().ok().map(|n| n * 1024)
    } else {
        s.parse::<u64>().ok()
    }
}

impl<'a> Drop for ServerTuner<'a> {
    fn drop(&mut self) {
        self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size_to_bytes("4G"), Some(4 * 1024 * 1024 * 1024));
        assert_eq!(parse_size_to_bytes("4GB"), Some(4 * 1024 * 1024 * 1024));
        assert_eq!(parse_size_to_bytes("512M"), Some(512 * 1024 * 1024));
        assert_eq!(parse_size_to_bytes("1024"), Some(1024));
    }
}
