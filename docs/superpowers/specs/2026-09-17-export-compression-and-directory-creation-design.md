# Design Document: Export Compression and Directory Resolution

- **Date:** 2026-09-17
- **Target:** `zephyr` (v1.0.0+)
- **Scope:** Export Gzip streaming compression, target directory resolution & auto-creation, and transparent import decompression.

---

## 1. Overview & Objectives

1. **Target Directory Handling**:
    - Ensure `zephyr export` supports `-d` and `--dir` as aliases for `-o` / `--output-dir`.
    - Prevent falling back to the default `"export-dumps"` directory if a directory was specified at the root command level (e.g. `zephyr -d ./dumps export`).
    - Automatically create non-existent directories and nested parent paths (`fs::create_dir_all`) when executing export instead of failing or using placeholder directories.
2. **Streaming Export Compression (`--compress` / `-z`)**:
    - Add a `--compress` (`-z`) flag to `export` to output Gzip compressed SQL dumps (`.sql.gz`).
    - Stream `mysqldump` stdout directly through `flate2::write::GzEncoder` with zero intermediate uncompressed disk spooling.
    - Include standard Workbench / Zephyr header directly inside the compressed archive.
3. **Import Round-Trip Decompression**:
    - Update `zephyr import` to discover both `.sql` and `.sql.gz` dump files.
    - Automatically decompress `.sql.gz` header comments in `scanner.rs` to detect database schemas.
    - Transparently stream decompressed SQL into `mysql` worker process stdin via `flate2::read::GzDecoder` in `runner.rs`.
4. **Manifest & Resume Consistency**:
    - Store exact filenames (`<table_name>.sql.gz`) in `manifest.json`.
    - Ensure `--resume` accurately skips completed `.sql.gz` files on both export and import.

---

## 2. Architecture & Component Changes

### 2.1 Dependency Management (`Cargo.toml`)

- Add `flate2 = "1.0"` to `[dependencies]` with default `miniz_oxide` pure-Rust backend.
- Guarantees zero C toolchain dependencies, ensuring full cross-platform compatibility across Windows and Linux.

### 2.2 CLI Definitions (`src/cli.rs`)

- On `ExportArgs`:
    - Add `short_alias = 'd'` and `alias = "dir"` to `output_dir`:
        ```rust
        #[arg(
            short = 'o',
            short_alias = 'd',
            long = "output-dir",
            alias = "dir",
            value_name = "DIR",
            default_value = "export-dumps"
        )]
        pub output_dir: PathBuf,
        ```
    - Add `--compress` flag:
        ```rust
        /// Compress exported dump files with gzip (.sql.gz)
        #[arg(short = 'z', long = "compress")]
        pub compress: bool,
        ```
- Add unit tests verifying:
    - `zephyr export -d ./target` sets `output_dir` to `./target`.
    - `zephyr export --dir ./target` sets `output_dir` to `./target`.
    - `zephyr export -o ./target` sets `output_dir` to `./target`.
    - `zephyr export --compress` and `export -z` enable `compress: true`.

### 2.3 Subcommand Argument Dispatch (`src/main.rs`)

- In `run()`:
    - If `Commands::Export(mut export_args)` is executed:
        - If `export_args.output_dir == PathBuf::from("export-dumps")` and root `cli.import.dir != PathBuf::from(".")`:
            - Rebind `export_args.output_dir = cli.import.dir;`
    - Pass the resolved `export_args` to `exporter::run_export(export_args)`.

### 2.4 Parallel Export Engine (`src/exporter.rs`)

- **Directory Creation**:
    - `fs::create_dir_all(&args.output_dir)` creates the entire directory hierarchy (including missing parent folders) before writing dumps or manifests.
    - In `--dry-run`, if `args.output_dir` does not exist, display:
      `Output Folder : <path> (will be created)`.
- **Naming Convention**:
    - If `args.compress` is active, files are named:
        - Single database: `{table}.sql.gz`
        - Multi-database: `{db}__{table}.sql.gz`
    - If uncompressed, names remain `{table}.sql` and `{db}__{table}.sql`.
- **Worker Process & Gzip Streaming**:
    - When `args.compress` is true:
        - Open `task.output_path` with `File::create`.
        - Wrap with `std::io::BufWriter::with_capacity(128 * 1024, file)`.
        - Wrap with `flate2::write::GzEncoder::new(buf_writer, flate2::Compression::default())`.
        - Write header directly into the encoder.
        - Spawn `mysqldump` with `cmd.stdout(Stdio::piped())`.
        - Concurrently read chunks from child stdout (64 KB buffers) and write to encoder.
        - Call `encoder.finish()?` upon completion of `mysqldump`.
    - When `args.compress` is false:
        - Retain zero-copy kernel streaming pipe (`Stdio::from(append_file)`).

### 2.5 Parallel Import Engine (`src/scanner.rs` & `src/runner.rs`)

- **Scanner Discovery (`src/scanner.rs`)**:
    - Update `WalkDir` filtering to accept both `.sql` and `.sql.gz` extensions:
        ```rust
        let is_sql = ext.eq_ignore_ascii_case("sql");
        let is_sql_gz = ext.eq_ignore_ascii_case("gz")
            && path.file_stem()
                .and_then(|s| Path::new(s).extension())
                .map_or(false, |e| e.eq_ignore_ascii_case("sql"));
        ```
- **Database Header Parsing**:
    - In `detect_database`, check if file ends in `.gz`.
    - If `.gz`: wrap `File::open(path)` with `flate2::read::GzDecoder::new(file)` before line scanning.
    - In `guess_database_from_filename`: strip both `.sql.gz` and `.sql`.
- **Ingestion Streaming (`src/runner.rs`)**:
    - In worker task execution:
        - If `task.file_path` ends with `.gz`:
            - Spawn `mysql` with `cmd.stdin(Stdio::piped())`.
            - Worker thread streams chunks from `flate2::read::GzDecoder::new(file)` into `child.stdin`.
        - If `.sql`:
            - Retain zero-copy direct file descriptor pipe (`Stdio::from(file)`).

---

## 3. Error Handling & Signals

- **Signal Interruption (`Ctrl+C`)**:
    - If export worker is cancelled while compressing, active `mysqldump` process is killed via `kill_os_pid(pid)`.
    - Partial corrupted `.sql.gz` file is deleted from disk on early failure/cancellation so that resumed jobs re-export cleanly.
- **Corrupt Archive Handling**:
    - If an imported `.sql.gz` archive is truncated or corrupted, `GzDecoder` returns an `std::io::Error`, which logs the error in `<db>__<table>.err.log` and records the task as failed in `failed_files.txt`.

---

## 4. Verification Plan

1. **Unit Tests**:
    - `test_export_args_dir_aliases`: verify `-d` and `--dir` parse into `output_dir`.
    - `test_export_args_compress_flags`: verify `--compress` and `-z` parse into `compress: true`.
2. **Integration Tests (`tests/e2e_test.rs`)**:
    - Export with `--compress` to a non-existent multi-level directory (e.g. `dumps/level1/level2`).
    - Verify directory created successfully.
    - Verify all generated files are valid Gzip archives containing expected MySQL DDL and INSERT statements.
    - Run `import` against the exported directory to ensure complete roundtrip ingestion passes.
3. **Format & Quality Checks**:
    - Run `cargo fmt --all -- --check`.
    - Run `cargo clippy --all-targets --all-features -- -D warnings`.
    - Run `cargo test --all-targets`.
