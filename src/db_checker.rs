use anyhow::{bail, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::cli::ImportArgs;

/// Verifies MySQL connection and ensures all required databases exist with live visual indicators and timeout protection.
pub fn ensure_databases(
    cli: &ImportArgs,
    mysql_bin: &Path,
    option_file: &Path,
    databases: &HashSet<String>,
) -> Result<()> {
    let mut sorted_dbs: Vec<_> = databases.iter().collect();
    sorted_dbs.sort();
    let total = sorted_dbs.len();

    let timeout_duration = Duration::from_secs(10);

    for (idx, db) in sorted_dbs.iter().enumerate() {
        let spinner = if !cli.no_progress {
            let sp = ProgressBar::new_spinner();
            sp.set_style(
                ProgressStyle::default_spinner()
                    .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                    .template("{spinner:.cyan} [{pos}/{len}] Checking database: {msg}")
                    .unwrap(),
            );
            sp.set_position((idx + 1) as u64);
            sp.set_length(total as u64);
            sp.set_message(format!("{}", db.cyan()));
            sp.enable_steady_tick(Duration::from_millis(80));
            Some(sp)
        } else {
            println!("[{}/{}] Checking database: {}", idx + 1, total, db);
            None
        };

        // 1. FAST CHECK: Run read-only "SHOW DATABASES LIKE '...'"
        // This never acquires exclusive metadata locks, resolving in milliseconds even if other processes are running.
        let escaped_db_like = db.replace('\'', "\\'");
        let check_query = format!("SHOW DATABASES LIKE '{}';", escaped_db_like);

        let mut check_cmd = Command::new(mysql_bin);
        check_cmd.arg(format!("--defaults-extra-file={}", option_file.display()));
        check_cmd.arg("-s");
        check_cmd.arg("-N");
        check_cmd.arg("-e");
        check_cmd.arg(&check_query);
        check_cmd.stdin(Stdio::null());
        check_cmd.stdout(Stdio::piped());
        check_cmd.stderr(Stdio::piped());

        let mut child = match check_cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                if let Some(sp) = spinner {
                    sp.finish_and_clear();
                }
                bail!(
                    "Failed to execute mysql client at {}: {}",
                    mysql_bin.display(),
                    e
                );
            }
        };

        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) => {
                    if start.elapsed() > timeout_duration {
                        let _ = child.kill();
                        let _ = child.wait();
                        if let Some(sp) = spinner {
                            sp.finish_and_clear();
                        }
                        bail!(
                            "Connection timed out after 10s connecting to MySQL at {}:{}.\n\
                             Possible causes:\n \
                             1. MySQL Server is not responding on 127.0.0.1:3306\n \
                             2. Windows Firewall or antivirus is blocking the connection\n \
                             3. Authentication is stalled or invalid password entered",
                            cli.host,
                            cli.port
                        );
                    }
                    thread::sleep(Duration::from_millis(40));
                }
                Err(e) => {
                    if let Some(sp) = spinner {
                        sp.finish_and_clear();
                    }
                    bail!("Error waiting for mysql process: {}", e);
                }
            }
        };

        let mut stdout_bytes = Vec::new();
        if let Some(mut stream) = child.stdout.take() {
            let _ = stream.read_to_end(&mut stdout_bytes);
        }
        let mut stderr_bytes = Vec::new();
        if let Some(mut stream) = child.stderr.take() {
            let _ = stream.read_to_end(&mut stderr_bytes);
        }

        if !status.success() {
            if let Some(sp) = spinner {
                sp.finish_and_clear();
            }
            let err_msg = String::from_utf8_lossy(&stderr_bytes);
            bail!(
                "Failed to connect to MySQL server ({}:{}):\n{}",
                cli.host,
                cli.port,
                err_msg.trim()
            );
        }

        let stdout_str = String::from_utf8_lossy(&stdout_bytes);
        let exists = stdout_str.trim().eq_ignore_ascii_case(db);

        if exists {
            if let Some(sp) = spinner {
                sp.finish_and_clear();
            }
            println!(
                "  {} [{}/{}] Database '{}' ready (already exists).",
                "✔".green(),
                idx + 1,
                total,
                db.cyan()
            );
            continue;
        }

        // 2. Database does not exist: CREATE DATABASE with short lock_wait_timeout
        let escaped_db_name = db.replace('`', "``");
        let create_query = format!(
            "SET SESSION lock_wait_timeout = 5; CREATE DATABASE IF NOT EXISTS `{}` DEFAULT CHARACTER SET {} COLLATE {}_unicode_ci;",
            escaped_db_name, cli.charset, cli.charset
        );

        let mut create_cmd = Command::new(mysql_bin);
        create_cmd.arg(format!("--defaults-extra-file={}", option_file.display()));
        create_cmd.arg("-e");
        create_cmd.arg(&create_query);
        create_cmd.stdin(Stdio::null());
        create_cmd.stdout(Stdio::null());
        create_cmd.stderr(Stdio::piped());

        let mut child_create = create_cmd.spawn()?;
        let start_create = Instant::now();
        let status_create = loop {
            match child_create.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) => {
                    if start_create.elapsed() > timeout_duration {
                        let _ = child_create.kill();
                        let _ = child_create.wait();
                        if let Some(sp) = spinner {
                            sp.finish_and_clear();
                        }
                        bail!(
                            "Timed out after 10s creating database '{}'. Another process may be holding a lock.",
                            db
                        );
                    }
                    thread::sleep(Duration::from_millis(40));
                }
                Err(e) => bail!("Error waiting for mysql process: {}", e),
            }
        };

        if !status_create.success() {
            let mut err_bytes = Vec::new();
            if let Some(mut stream) = child_create.stderr.take() {
                let _ = stream.read_to_end(&mut err_bytes);
            }
            let err_msg = String::from_utf8_lossy(&err_bytes);
            if let Some(sp) = spinner {
                sp.finish_and_clear();
            }
            bail!(
                "Failed to create database '{}' on MySQL server ({}:{}):\n{}",
                db,
                cli.host,
                cli.port,
                err_msg.trim()
            );
        }

        if let Some(sp) = spinner {
            sp.finish_and_clear();
        }
        println!(
            "  {} [{}/{}] Database '{}' created successfully.",
            "✔".green(),
            idx + 1,
            total,
            db.cyan()
        );
    }

    Ok(())
}
