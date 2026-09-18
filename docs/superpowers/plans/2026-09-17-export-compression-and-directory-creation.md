# Export Compression & Directory Resolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add streaming Gzip compression (`--compress` / `-z`) to `mysql-turboload export`, support `-d` and `--dir` aliases with recursive directory auto-creation, and enable transparent `.sql.gz` decompression during `mysql-turboload import`.

**Architecture:** Integrate `flate2` (pure-Rust `miniz_oxide` backend) into both the export and import streaming pipelines. During export, pipe `mysqldump` stdout into `flate2::write::GzEncoder` directly to disk without temporary spooling. In import, transparently decode `.sql.gz` streams into `mysql` stdin via `flate2::read::GzDecoder`. Unify CLI directory flags so `-d`, `--dir`, and `-o` reliably target or create the intended destination folder.

**Tech Stack:** Rust 2021 edition, `flate2 = "1.0"`, `clap = "4.5"`, `crossbeam-channel`, Rayon.

## Global Constraints
- Pure Rust compression backend (`miniz_oxide`) without C toolchain or external runtime dependencies.
- Zero disk spooling: compression and decompression stream on-the-fly in worker threads.
- 100% backwards compatibility with standard `.sql` imports and exports.
- Passing `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`, and all integration tests.

---

### Task 1: Add Dependencies and CLI Argument Aliases

**Files:**
- Modify: `Cargo.toml:11-28`
- Modify: `src/cli.rs:177-200, 341-382`

**Interfaces:**
- Consumes: Clap derive macros.
- Produces: `ExportArgs.compress: bool`, alias `-d` and `--dir` on `ExportArgs.output_dir`.

- [ ] **Step 1: Write the failing unit tests in `src/cli.rs`**
Add unit tests in `src/cli.rs` under `mod tests`:
```rust
#[test]
fn test_export_args_dir_aliases() {
    let cli_short = Cli::try_parse_from(["mysql-turboload", "export", "-d", "./my-target"]).unwrap();
    if let Some(Commands::Export(export_args)) = cli_short.command {
        assert_eq!(export_args.output_dir, PathBuf::from("./my-target"));
    } else {
        panic!("Expected export command");
    }

    let cli_long = Cli::try_parse_from(["mysql-turboload", "export", "--dir", "./my-target"]).unwrap();
    if let Some(Commands::Export(export_args)) = cli_long.command {
        assert_eq!(export_args.output_dir, PathBuf::from("./my-target"));
    } else {
        panic!("Expected export command");
    }
}

#[test]
fn test_export_args_compress_flags() {
    let cli_flag = Cli::try_parse_from(["mysql-turboload", "export", "--compress"]).unwrap();
    if let Some(Commands::Export(export_args)) = cli_flag.command {
        assert!(export_args.compress);
    } else {
        panic!("Expected export command");
    }

    let cli_short = Cli::try_parse_from(["mysql-turboload", "export", "-z"]).unwrap();
    if let Some(Commands::Export(export_args)) = cli_short.command {
        assert!(export_args.compress);
    } else {
        panic!("Expected export command");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test --bin mysql-turboload test_export_args_dir_aliases`
Expected: Compilation failure because `-d`, `--dir`, and `--compress` are not defined on `ExportArgs`.

- [ ] **Step 3: Update `Cargo.toml` and `src/cli.rs`**
In `Cargo.toml`, add `flate2 = "1.0"`:
```toml
flate2 = "1.0"
```

In `src/cli.rs`, update `ExportArgs`:
```rust
#[derive(Args, Debug, Clone)]
pub struct ExportArgs {
    /// Output directory to store exported .sql files
    #[arg(
        short = 'o',
        short_alias = 'd',
        long = "output-dir",
        alias = "dir",
        value_name = "DIR",
        default_value = "export-dumps"
    )]
    pub output_dir: PathBuf,

    /// Compress exported dump files with gzip (.sql.gz)
    #[arg(short = 'z', long = "compress")]
    pub compress: bool,
    ...
```

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test --bin mysql-turboload test_export_args_`
Expected: PASS for both `test_export_args_dir_aliases` and `test_export_args_compress_flags`.

---

### Task 2: Subcommand Argument Dispatch and Directory Auto-Creation

**Files:**
- Modify: `src/main.rs:34-43`
- Modify: `src/exporter.rs:87-97, 238-246`

**Interfaces:**
- Consumes: `Cli.import.dir`, `ExportArgs.output_dir`.
- Produces: Inherited `output_dir` from root CLI if specified; robust `fs::create_dir_all(&args.output_dir)`.

- [ ] **Step 1: Write test for root directory inheritance**
In `src/cli.rs` tests:
```rust
#[test]
fn test_root_dir_inheritance_logic() {
    let cli = Cli::try_parse_from(["mysql-turboload", "-d", "./root-dumps", "export"]).unwrap();
    assert_eq!(cli.import.dir, PathBuf::from("./root-dumps"));
    if let Some(Commands::Export(mut export_args)) = cli.command {
        if export_args.output_dir == PathBuf::from("export-dumps") && cli.import.dir != PathBuf::from(".") {
            export_args.output_dir = cli.import.dir;
        }
        assert_eq!(export_args.output_dir, PathBuf::from("./root-dumps"));
    } else {
        panic!("Expected export command");
    }
}
```

- [ ] **Step 2: Update `src/main.rs` dispatch logic**
Update `run()` in `src/main.rs`:
```rust
fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Export(mut export_args)) => {
            if export_args.output_dir == std::path::PathBuf::from("export-dumps")
                && cli.import.dir != std::path::PathBuf::from(".")
            {
                export_args.output_dir = cli.import.dir;
            }
            exporter::run_export(export_args)
        }
        Some(Commands::Import(import_args)) => run_import(import_args),
        None => run_import(cli.import),
    }
}
```

- [ ] **Step 3: Update `src/exporter.rs` directory resolution & dry-run reporting**
Ensure `args.output_dir` creation has detailed error context:
```rust
    let log_dir = args.resolved_log_dir();
    fs::create_dir_all(&args.output_dir)
        .with_context(|| format!("Failed to create output directory: {}", args.output_dir.display()))?;
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("Failed to create log directory: {}", log_dir.display()))?;
```
And under dry-run:
```rust
    if args.dry_run {
        let dir_note = if !args.output_dir.exists() {
            " (will be created)"
        } else {
            ""
        };
        println!("Output Folder : {}{}", args.output_dir.display(), dir_note);
    }
```

- [ ] **Step 4: Run unit tests to verify**
Run: `cargo test --bin mysql-turboload`
Expected: PASS.

---

### Task 3: Streaming Export Gzip Compression in `exporter.rs`

**Files:**
- Modify: `src/exporter.rs:349-379, 456-600`

**Interfaces:**
- Consumes: `ExportArgs.compress`, `mysqldump` stdout stream.
- Produces: `.sql.gz` dump archives when `compress: true`, `.sql` files otherwise.

- [ ] **Step 1: Update task filename generation in `collect_export_tasks`**
In `src/exporter.rs`:
```rust
    let ext = if args.compress { "sql.gz" } else { "sql" };
    let file_name = if single_db {
        format!("{table}.{ext}")
    } else {
        format!("{db}__{table}.{ext}")
    };
```

- [ ] **Step 2: Implement streaming Gzip compression in worker thread**
In `execute_export_tasks`:
```rust
    if args_clone.compress {
        let out_file = match File::create(&task.output_path) {
            Ok(f) => f,
            Err(e) => {
                // Record failure event & log
                ...
                continue;
            }
        };

        let buf_writer = std::io::BufWriter::with_capacity(128 * 1024, out_file);
        let mut encoder = flate2::write::GzEncoder::new(buf_writer, flate2::Compression::default());

        if let Err(e) = encoder.write_all(header.as_bytes()) {
            // Record failure event & log
            ...
            continue;
        }

        let mut cmd = Command::new(&mysqldump_bin_buf);
        // Add options...
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = match cmd.spawn() { ... };
        
        // Spawn streaming thread copying child.stdout to encoder
        let mut child_stdout = child.stdout.take().unwrap();
        let mut copy_err = None;
        let mut buffer = [0u8; 65536];
        loop {
            match std::io::Read::read(&mut child_stdout, &mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    if let Err(e) = encoder.write_all(&buffer[..n]) {
                        copy_err = Some(e);
                        break;
                    }
                }
                Err(e) => {
                    copy_err = Some(e);
                    break;
                }
            }
        }

        if let Err(e) = encoder.finish() {
            if copy_err.is_none() {
                copy_err = Some(e);
            }
        }
        ...
    } else {
        // Retain direct file descriptor streaming for uncompressed exports
    }
```

- [ ] **Step 3: Handle partial cleanup on error or cancellation**
If task fails or was cancelled, remove the partial `.sql.gz` file:
```rust
if !status.success() || cancelled.load(Ordering::SeqCst) {
    let _ = fs::remove_file(&task.output_path);
}
```

- [ ] **Step 4: Verify compilation**
Run: `cargo check`
Expected: Succeeded with 0 errors.

---

### Task 4: Import Round-Trip Decompression Support in `scanner.rs` & `runner.rs`

**Files:**
- Modify: `src/scanner.rs:26-87, 89-105, 153-172`
- Modify: `src/runner.rs:188-230`

**Interfaces:**
- Consumes: Dump files ending in `.sql` or `.sql.gz`.
- Produces: Validated `TableTask` items and streaming decompressed bytes to `mysql` stdin.

- [ ] **Step 1: Update file scanner for `.sql.gz` support**
In `src/scanner.rs`:
```rust
    if path.is_file() {
        let is_target = path.extension().map_or(false, |ext| {
            if ext.eq_ignore_ascii_case("sql") {
                true
            } else if ext.eq_ignore_ascii_case("gz") {
                path.file_stem()
                    .and_then(|s| Path::new(s).extension())
                    .map_or(false, |e| e.eq_ignore_ascii_case("sql"))
            } else {
                false
            }
        });
        if is_target {
            paths.push(path.to_path_buf());
        }
    }
```

- [ ] **Step 2: Update header scanner in `detect_database`**
In `src/scanner.rs`:
```rust
pub fn detect_database(path: &Path, max_lines: usize) -> Option<String> {
    let file = File::open(path).ok()?;
    let is_gz = path.extension().map_or(false, |ext| ext.eq_ignore_ascii_case("gz"));

    let reader: Box<dyn BufRead> = if is_gz {
        let decoder = flate2::read::GzDecoder::new(file);
        Box::new(BufReader::new(decoder))
    } else {
        Box::new(BufReader::new(file))
    };
    ...
```

- [ ] **Step 3: Update `guess_database_from_filename`**
Strip `.sql.gz` and `.sql` suffixes:
```rust
fn clean_file_stem(filename: &str) -> &str {
    let mut s = filename;
    if s.ends_with(".gz") {
        s = &s[..s.len() - 3];
    }
    if s.ends_with(".sql") {
        s = &s[..s.len() - 4];
    }
    s
}
```

- [ ] **Step 4: Update ingestion worker in `runner.rs` for `.gz` files**
In `src/runner.rs`, if `task.file_path.extension().map_or(false, |ext| ext.eq_ignore_ascii_case("gz"))`:
```rust
    let mut cmd = Command::new(&mysql_bin_buf);
    // Standard connection args...
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    let mut child_stdin = child.stdin.take().unwrap();
    let file = File::open(&task.file_path)?;
    let mut decoder = flate2::read::GzDecoder::new(file);

    // Stream decompressed chunks to child_stdin
    let stream_handle = thread::spawn(move || {
        let mut buf = [0u8; 65536];
        while let Ok(n) = std::io::Read::read(&mut decoder, &mut buf) {
            if n == 0 { break; }
            if child_stdin.write_all(&buf[..n]).is_err() { break; }
        }
    });
```

- [ ] **Step 5: Verify compilation**
Run: `cargo check`
Expected: Succeeded with 0 errors.

---

### Task 5: Integration Tests, End-to-End Roundtrip, and Formatting

**Files:**
- Modify: `tests/e2e_test.rs`
- Modify: `README.md`

**Interfaces:**
- Consumes: Full export and import pipelines.
- Produces: Green test suite, clean formatting, zero Clippy warnings.

- [ ] **Step 1: Add integration tests in `tests/e2e_test.rs`**
Add:
- `test_export_compressed_and_import_roundtrip`: exports with `--compress` to a non-existent nested folder (e.g. `temp_path.join("nested/level/dumps")`), verifies directory is created, verify `.sql.gz` files exist and are valid Gzip archives, then runs `import --dir` on that directory to verify successful ingestion.

- [ ] **Step 2: Run tests**
Run: `cargo test --all-targets`
Expected: All tests pass.

- [ ] **Step 3: Run Clippy and formatting checks**
Run: `cargo fmt --all -- --check`
Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: 0 warnings, formatting matches style rules.

- [ ] **Step 4: Update `README.md` documentation**
Document `--compress` (`-z`), `-d` / `--dir` aliases, directory auto-creation, and `.sql.gz` import support.
