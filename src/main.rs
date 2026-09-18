mod cli;
mod db_checker;
mod exporter;
mod manifest;
mod mysql_locator;
mod option_file;
mod progress;
mod runner;
mod scanner;
mod server_tuning;

use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use std::collections::{HashMap, HashSet};
use std::process;
use std::sync::Arc;

use cli::{Cli, Commands, ImportArgs};
use manifest::ManifestManager;
use option_file::MySqlOptionFile;
use progress::{print_summary, ProgressTracker};
use runner::execute_tasks;
use scanner::collect_tasks;
use server_tuning::ServerTuner;

fn main() {
    if let Err(err) = run() {
        eprintln!("\n{} {:#}", "ERROR:".red().bold(), err);
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Export(mut export_args)) => {
            if export_args.dir == std::path::Path::new("export-dumps")
                && cli.import.dir != std::path::Path::new(".")
            {
                export_args.dir = cli.import.dir;
            }
            exporter::run_export(export_args)
        }
        Some(Commands::Import(import_args)) => run_import(import_args),
        None => run_import(cli.import),
    }
}

fn run_import(mut cli: ImportArgs) -> Result<()> {
    // Locate MySQL binary (optional fallback for dry-run mode)
    let mysql_bin = if cli.dry_run {
        mysql_locator::find_mysql(cli.mysql_bin.as_deref())
            .unwrap_or_else(|_| std::path::PathBuf::from("mysql"))
    } else {
        mysql_locator::find_mysql(cli.mysql_bin.as_deref())?
    };

    // Scan backup folder / read retry list
    let scan = collect_tasks(
        &cli.dir,
        cli.retry_file.as_deref(),
        cli.database.as_deref(),
        cli.default_database.as_deref(),
        cli.filter.as_deref(),
        cli.scan_depth,
    )?;

    // Manifest for resume support
    let manifest = Arc::new(ManifestManager::new(
        &cli.log_dir,
        &cli.dir,
        cli.manifest.as_deref(),
    ));

    let total_scanned_count = scan.tasks.len();
    let total_scanned_bytes = scan.total_bytes;
    let mut tasks = scan.tasks;

    let manifest_count = manifest.count();
    if let Some(loaded_path) = manifest.loaded_from() {
        println!(
            "{}",
            format!(
                "[MANIFEST] Loaded {manifest_count} completed tables from: {}",
                loaded_path.display()
            )
            .cyan()
        );
    }

    if cli.resume {
        let before_count = tasks.len();
        let mut skipped_bytes = 0u64;

        tasks.retain(|t| {
            if manifest.is_completed(&t.file_name, Some(&t.file_path)) {
                skipped_bytes += t.size_bytes;
                false
            } else {
                true
            }
        });

        let skipped_count = before_count - tasks.len();
        let skipped_mb = (skipped_bytes as f64) / (1024.0 * 1024.0);
        let remaining_mb =
            (tasks.iter().map(|t| t.size_bytes).sum::<u64>() as f64) / (1024.0 * 1024.0);

        if skipped_count > 0 {
            println!(
                "{}",
                format!(
                    "Resuming: skipped {skipped_count} previously completed tables. ({skipped_mb:.2} MB skipped, {remaining} tables remaining - {remaining_mb:.2} MB).",
                    remaining = tasks.len()
                )
                .yellow()
                .bold()
            );
        } else if manifest_count > 0 {
            eprintln!(
                "{}",
                format!(
                    "[WARNING] Manifest contains {manifest_count} completed tables, but 0 matched files in '{}'!",
                    cli.dir.display()
                )
                .yellow()
                .bold()
            );
            let samples = manifest.sample_entries(3);
            eprintln!("  Sample manifest entries: {:?}", samples);
            let task_samples: Vec<String> =
                tasks.iter().take(3).map(|t| t.file_name.clone()).collect();
            eprintln!("  Sample scanned files:    {:?}", task_samples);
        } else {
            eprintln!(
                "{}",
                format!(
                    "[INFO] --resume specified, but no completed tables recorded in manifest (target: {}). Starting full import.",
                    manifest.file_path().display()
                )
                .yellow()
            );
        }
    } else if manifest_count > 0 {
        println!(
            "{}",
            format!(
                "[NOTICE] Found {} previously completed tables in manifest ({}). Run with '--resume' to skip them.",
                manifest_count,
                manifest.loaded_from().map_or_else(|| manifest.file_path().display().to_string(), |p| p.display().to_string())
            )
            .yellow()
            .bold()
        );
    }

    if tasks.is_empty() {
        println!(
            "{}",
            "No tables to import (all tasks completed or skipped).".green()
        );
        return Ok(());
    }

    // Recalculate database breakdown for active tasks
    let mut remaining_db_summaries: HashMap<String, (usize, u64)> = HashMap::new();
    let mut remaining_bytes = 0u64;
    for t in &tasks {
        remaining_bytes += t.size_bytes;
        let entry = remaining_db_summaries
            .entry(t.database.clone())
            .or_insert((0, 0));
        entry.0 += 1;
        entry.1 += t.size_bytes;
    }

    // Header Banner
    println!();
    println!(
        "{}",
        "==================================================".cyan()
    );
    println!("{}", " MYSQL TURBOLOAD ENTERPRISE v1.0.0".cyan().bold());
    println!("{}", " Concurrent Bulk Database Ingestion Engine".cyan());
    println!(
        "{}",
        "==================================================".cyan()
    );
    println!("Target Server : {}:{}", cli.host, cli.port);
    println!("MySQL User    : {}", cli.user);
    println!("MySQL Client  : {}", mysql_bin.display());
    println!("Source Folder : {}", cli.dir.display());
    println!("Manifest File : {}", manifest.file_path().display());
    println!("Workers       : {}", cli.resolved_workers());
    if cli.resume && total_scanned_count != tasks.len() {
        let skipped_count = total_scanned_count - tasks.len();
        let skipped_mb = ((total_scanned_bytes - remaining_bytes) as f64) / (1024.0 * 1024.0);
        let rem_mb = (remaining_bytes as f64) / (1024.0 * 1024.0);
        println!(
            "Total In Dump : {} files ({:.2} MB)",
            total_scanned_count,
            (total_scanned_bytes as f64) / (1024.0 * 1024.0)
        );
        println!(
            "Already Done  : {} files ({:.2} MB) [Skipped via manifest]",
            skipped_count.to_string().yellow(),
            skipped_mb
        );
        println!(
            "To Import     : {} files ({:.2} MB)",
            tasks.len().to_string().green().bold(),
            rem_mb
        );
    } else {
        println!("SQL Files     : {}", tasks.len());
        println!(
            "Total Size    : {:.2} MB",
            (remaining_bytes as f64) / (1024.0 * 1024.0)
        );
    }

    // Detected databases breakdown
    println!();
    println!("{}", "Target databases:".yellow().bold());
    let mut db_names: Vec<String> = remaining_db_summaries.keys().cloned().collect();
    db_names.sort();
    for db in &db_names {
        if let Some((count, bytes)) = remaining_db_summaries.get(db) {
            let mb = (*bytes as f64) / (1024.0 * 1024.0);
            if let Some((orig_count, _)) = scan.database_summaries.get(db) {
                if *orig_count > *count {
                    let skipped = orig_count - count;
                    println!(
                        "  {:<25} {:>4} files to import ({} skipped)  {:>10.2} MB",
                        db.cyan(),
                        count,
                        skipped.to_string().yellow(),
                        mb
                    );
                    continue;
                }
            }
            println!("  {:<25} {:>4} files  {:>10.2} MB", db.cyan(), count, mb);
        }
    }

    if !scan.skipped_files.is_empty() {
        println!();
        println!("{}", "Skipped files:".yellow());
        for (path, reason) in &scan.skipped_files {
            println!("  {} ({})", path.display(), reason.dimmed());
        }
    }
    println!();

    if cli.dry_run {
        println!(
            "{}",
            "[DRY RUN] Plan displayed. No database changes were made."
                .green()
                .bold()
        );
        return Ok(());
    }

    // Resolve credentials (prompt securely if omitted)
    cli.resolve_password()?;

    // Create temporary MySQL option file with RAII cleanup on drop
    let option_file =
        MySqlOptionFile::create(&cli.host, cli.port, &cli.user, cli.password.as_deref())?;

    // Ensure all target databases exist before starting parallel workers
    let required_databases: HashSet<String> = tasks.iter().map(|t| t.database.clone()).collect();
    println!(
        "{}",
        "Verifying database connection & schemas...".blue().bold()
    );
    db_checker::ensure_databases(&cli, &mysql_bin, option_file.path(), &required_databases)?;
    println!();

    println!(
        "{}",
        "==================================================".green()
    );
    println!("{}", " STARTING PARALLEL IMPORT".green().bold());
    println!(
        "{}",
        "==================================================".green()
    );

    // Optional server-side tuning (innodb_flush_log_at_trx_commit, sync_binlog)
    let mut tuner = ServerTuner::new(&cli, &mysql_bin, option_file.path());
    tuner.apply();

    let total_tasks = tasks.len();
    let total_bytes: u64 = tasks.iter().map(|t| t.size_bytes).sum();
    let tracker = Arc::new(ProgressTracker::new(
        total_tasks,
        total_bytes,
        cli.resolved_workers(),
        cli.no_progress,
    ));

    let tracker_callback = Arc::clone(&tracker);
    let summary = execute_tasks(
        &cli,
        &mysql_bin,
        option_file.path(),
        tasks,
        manifest,
        tracker.live_states(),
        move |event, state| {
            tracker_callback.handle_event(event, state);
        },
    )?;

    tracker.finish();

    // Restore server settings if they were tuned
    tuner.restore();

    print_summary(&summary, &cli.log_dir);

    if summary.cancelled || !summary.failed_tasks.is_empty() {
        process::exit(1);
    }

    Ok(())
}
