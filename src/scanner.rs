use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use regex::Regex;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct TableTask {
    pub file_path: PathBuf,
    pub file_name: String,
    pub database: String,
    pub size_bytes: u64,
    pub size_mb: f64,
}

pub struct ScanResult {
    pub tasks: Vec<TableTask>,
    pub skipped_files: Vec<(PathBuf, String)>,
    pub total_bytes: u64,
    pub database_summaries: HashMap<String, (usize, u64)>, // (count, total_bytes)
}

/// Detects the target database name by inspecting the initial header lines of a SQL dump file.
pub fn detect_database(path: &Path, max_lines: usize) -> Option<String> {
    let file = File::open(path).ok()?;
    let is_gz = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gz"));

    let reader: Box<dyn BufRead> = if is_gz {
        let decoder = flate2::read::GzDecoder::new(file);
        Box::new(BufReader::new(decoder))
    } else {
        Box::new(BufReader::new(file))
    };

    // Regex 1: mysqldump / Workbench comments e.g.
    // "-- Host: localhost    Database: mydb"
    // "-- Current Database: `mydb`"
    // "-- Database: `9741425`"
    let re_comment =
        Regex::new(r#"(?i)(?:current\s+)?database:\s*[`'"]?([^\s`'";]+)[`'"]?"#).ok()?;

    // Regex 2: SQL statement "USE `mydb`;"
    let re_use = Regex::new(r#"(?i)^\s*USE\s+[`'"]?([^\s`'";]+)[`'"]?\s*;?"#).ok()?;

    // Regex 3: SQL statement "CREATE DATABASE [/*!...*/] [IF NOT EXISTS] `mydb`"
    let re_create_db = Regex::new(r#"(?i)^\s*CREATE\s+DATABASE(?:\s+/\*!.*?\*/)?(?:\s+IF\s+NOT\s+EXISTS)?\s+[`'"]?([^\s`'";]+)[`'"]?"#).ok()?;

    for (line_idx, line_res) in reader.lines().enumerate() {
        if line_idx >= max_lines {
            break;
        }

        let line = match line_res {
            Ok(l) => l,
            Err(_) => break, // binary or encoding issue
        };

        let trimmed = line.trim();

        // If we hit data or schema creation, header comment section has ended
        if trimmed.starts_with("CREATE TABLE")
            || trimmed.starts_with("INSERT INTO")
            || trimmed.starts_with("LOCK TABLES")
        {
            break;
        }

        // 1. Try comment pattern
        if let Some(caps) = re_comment.captures(trimmed) {
            if let Some(m) = caps.get(1) {
                return Some(m.as_str().to_string());
            }
        }

        // 2. Try USE statement
        if let Some(caps) = re_use.captures(trimmed) {
            if let Some(m) = caps.get(1) {
                return Some(m.as_str().to_string());
            }
        }

        // 3. Try CREATE DATABASE statement
        if let Some(caps) = re_create_db.captures(trimmed) {
            if let Some(m) = caps.get(1) {
                return Some(m.as_str().to_string());
            }
        }
    }

    None
}

/// Tries to guess database from filename patterns like `mydb.users.sql` or `mydb__users.sql`
fn guess_database_from_filename(filename: &str) -> Option<String> {
    let clean = if filename.to_lowercase().ends_with(".gz") {
        &filename[..filename.len() - 3]
    } else {
        filename
    };

    if let Some(idx) = clean.find("__") {
        if idx > 0 {
            return Some(clean[..idx].to_string());
        }
    }
    if let Some(idx) = clean.find('.') {
        // e.g. "mydb.users.sql" has two dots
        if let Some(last_idx) = clean.rfind('.') {
            if idx != last_idx && idx > 0 {
                return Some(clean[..idx].to_string());
            }
        }
    }
    None
}

enum CandidateResult {
    Task(TableTask),
    Skipped(PathBuf, String),
    Filtered,
}

/// Scans backup directory or reads retry list, resolving databases in parallel using Rayon.
pub fn collect_tasks(
    dir: &Path,
    retry_file: Option<&Path>,
    override_db: Option<&str>,
    default_db: Option<&str>,
    filter: Option<&str>,
    scan_depth: usize,
) -> Result<ScanResult> {
    let candidate_paths: Vec<PathBuf> = if let Some(retry_path) = retry_file {
        if !retry_path.exists() {
            bail!("Retry file not found: {}", retry_path.display());
        }
        let file = File::open(retry_path)
            .with_context(|| format!("Failed to open retry file: {}", retry_path.display()))?;
        let reader = BufReader::new(file);
        let mut paths = Vec::new();
        for line in reader.lines() {
            let l = line?.trim().to_string();
            if !l.is_empty() && !l.starts_with('#') {
                let p = PathBuf::from(&l);
                if p.is_file() {
                    paths.push(p);
                } else {
                    let joined = dir.join(&p);
                    if joined.is_file() {
                        paths.push(joined);
                    }
                }
            }
        }
        paths
    } else {
        if !dir.exists() {
            bail!("Backup directory not found: {}", dir.display());
        }
        if !dir.is_dir() {
            bail!("Path is not a directory: {}", dir.display());
        }

        // Fast parallel-friendly discovery using WalkDir
        let mut paths = Vec::new();
        for entry in WalkDir::new(dir)
            .max_depth(2)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    let is_target = if ext.eq_ignore_ascii_case("sql") {
                        true
                    } else if ext.eq_ignore_ascii_case("gz") {
                        path.file_stem()
                            .and_then(|s| Path::new(s).extension())
                            .is_some_and(|e| e.eq_ignore_ascii_case("sql"))
                    } else {
                        false
                    };
                    if is_target {
                        paths.push(path.to_path_buf());
                    }
                }
            }
        }
        paths.sort();
        paths
    };

    if candidate_paths.is_empty() {
        bail!("No .sql or .sql.gz dump files found to import.");
    }

    let filter_lower = filter.map(|s| s.to_lowercase());

    // Parallel processing with Rayon: inspect headers, metadata, and database names across CPU cores
    let processed: Vec<CandidateResult> = candidate_paths
        .into_par_iter()
        .map(|path| {
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            if let Some(ref pattern) = filter_lower {
                if !file_name.to_lowercase().contains(pattern) {
                    return CandidateResult::Filtered;
                }
            }

            let metadata = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(e) => {
                    return CandidateResult::Skipped(
                        path,
                        format!("Cannot read file metadata: {e}"),
                    );
                }
            };

            let size_bytes = metadata.len();
            let size_mb = (size_bytes as f64) / (1024.0 * 1024.0);

            // Resolve database name
            let db_name = if let Some(forced) = override_db {
                forced.to_string()
            } else if let Some(detected) = detect_database(&path, scan_depth) {
                detected
            } else if let Some(default) = default_db {
                default.to_string()
            } else if let Some(guessed) = guess_database_from_filename(&file_name) {
                guessed
            } else {
                return CandidateResult::Skipped(
                    path,
                    "Cannot detect target database from header or filename. Use --database or --default-database."
                        .to_string(),
                );
            };

            CandidateResult::Task(TableTask {
                file_path: path,
                file_name,
                database: db_name,
                size_bytes,
                size_mb,
            })
        })
        .collect();

    let mut tasks = Vec::new();
    let mut skipped_files = Vec::new();
    let mut database_summaries: HashMap<String, (usize, u64)> = HashMap::new();
    let mut total_bytes = 0u64;

    for item in processed {
        match item {
            CandidateResult::Task(task) => {
                total_bytes += task.size_bytes;
                let entry = database_summaries
                    .entry(task.database.clone())
                    .or_insert((0, 0));
                entry.0 += 1;
                entry.1 += task.size_bytes;
                tasks.push(task);
            }
            CandidateResult::Skipped(path, reason) => {
                skipped_files.push((path, reason));
            }
            CandidateResult::Filtered => {}
        }
    }

    // Sort descending by size (Longest Processing Time first for optimal work-stealing)
    tasks.sort_by_key(|a| std::cmp::Reverse(a.size_bytes));

    Ok(ScanResult {
        tasks,
        skipped_files,
        total_bytes,
        database_summaries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_detect_mysqldump_comment() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            "-- MySQL dump 10.13  Distrib 8.0.34, for Win64 (x86_64)"
        )
        .unwrap();
        writeln!(file, "--").unwrap();
        writeln!(file, "-- Host: localhost    Database: shop_production").unwrap();
        writeln!(
            file,
            "-- ------------------------------------------------------"
        )
        .unwrap();
        file.flush().unwrap();

        let detected = detect_database(file.path(), 30);
        assert_eq!(detected, Some("shop_production".to_string()));
    }

    #[test]
    fn test_detect_current_database_with_backticks() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "-- Current Database: `9741425`").unwrap();
        file.flush().unwrap();

        let detected = detect_database(file.path(), 30);
        assert_eq!(detected, Some("9741425".to_string()));
    }

    #[test]
    fn test_detect_database_simple_comment() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "-- Database: `custom_db`").unwrap();
        file.flush().unwrap();

        let detected = detect_database(file.path(), 30);
        assert_eq!(detected, Some("custom_db".to_string()));
    }

    #[test]
    fn test_detect_use_statement() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "/* comments */").unwrap();
        writeln!(file, "USE `analytics_db`;").unwrap();
        file.flush().unwrap();

        let detected = detect_database(file.path(), 30);
        assert_eq!(detected, Some("analytics_db".to_string()));
    }

    #[test]
    fn test_guess_database_filename() {
        assert_eq!(
            guess_database_from_filename("store__orders.sql"),
            Some("store".to_string())
        );
        assert_eq!(
            guess_database_from_filename("store.orders.sql"),
            Some("store".to_string())
        );
        assert_eq!(
            guess_database_from_filename("store__orders.sql.gz"),
            Some("store".to_string())
        );
        assert_eq!(
            guess_database_from_filename("store.orders.sql.gz"),
            Some("store".to_string())
        );
        assert_eq!(guess_database_from_filename("orders.sql"), None);
        assert_eq!(guess_database_from_filename("orders.sql.gz"), None);
    }

    #[test]
    fn test_detect_database_compressed_sql_gz() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use tempfile::Builder;

        let temp_file = Builder::new().suffix(".sql.gz").tempfile().unwrap();
        let mut encoder = GzEncoder::new(temp_file.as_file(), Compression::default());
        writeln!(encoder, "-- Host: localhost    Database: crm_production").unwrap();
        writeln!(encoder, "CREATE TABLE users (id INT);").unwrap();
        encoder.finish().unwrap();

        let detected = detect_database(temp_file.path(), 30);
        assert_eq!(detected, Some("crm_production".to_string()));
    }

    #[test]
    fn test_collect_tasks_with_sql_and_sql_gz() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use tempfile::tempdir;

        let temp = tempdir().unwrap();
        let dir = temp.path();

        // 1. Plain .sql
        let sql_path = dir.join("users.sql");
        let mut f1 = File::create(&sql_path).unwrap();
        writeln!(f1, "-- Current Database: `shop`").unwrap();
        writeln!(f1, "CREATE TABLE users (id INT);").unwrap();

        // 2. Compressed .sql.gz
        let gz_path = dir.join("orders.sql.gz");
        let f2 = File::create(&gz_path).unwrap();
        let mut enc = GzEncoder::new(f2, Compression::default());
        writeln!(enc, "-- Current Database: `shop`").unwrap();
        writeln!(enc, "CREATE TABLE orders (id INT);").unwrap();
        enc.finish().unwrap();

        let res = collect_tasks(dir, None, None, None, None, 50).unwrap();
        assert_eq!(res.tasks.len(), 2);
        let names: Vec<String> = res.tasks.into_iter().map(|t| t.file_name).collect();
        assert!(names.contains(&"users.sql".to_string()));
        assert!(names.contains(&"orders.sql.gz".to_string()));
    }
}
