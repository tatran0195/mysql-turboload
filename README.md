# Zephyr (v1.0.0)

[![CI](https://github.com/enterprise/zephyr/actions/workflows/ci.yml/badge.svg)](https://github.com/enterprise/zephyr/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust 1.75+](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)

**Zephyr** is a high-performance, concurrent bulk data migration, restoration, and export engine engineered in Rust. Built as a modern replacement for legacy PowerShell and shell migration scripts, Zephyr eliminates worker thread starvation via dynamic work stealing, coordinates non-blocking server-side InnoDB tuning, prevents pipe buffer deadlocks, and guarantees fault-tolerant resumption through atomic JSON state manifests.

Zephyr provides **bidirectional acceleration**:
1. **Parallel Ingestion (`import`)**: Multi-worker bulk restoration of MySQL Workbench dump project folders (supporting both plain `.sql` and compressed `.sql.gz` archives with transparent streaming decompression).
2. **Parallel Extraction (`export`)**: High-throughput multi-worker table extraction generating 100% Workbench-compatible dumps, with real-time zero-spool Gzip streaming compression (`--compress` / `-z`) and recursive directory auto-creation.

---

## Architectural Comparison: Enterprise vs. Legacy Tools

| Feature / Metric | Traditional Tools (`mysqldump` / PowerShell) | Zephyr |
| :--- | :--- | :--- |
| **Export Concurrency** | Single-threaded sequential dump. Dumps one table at a time; takes hours on large schemas. | **Parallel Multi-Worker Extraction**. Concurrently extracts tables across 4 to 16 native OS threads sorted LPT. |
| **Compression & Storage** | Uncompressed dumps or external gzip subprocess pipes requiring 2x disk space and temp spooling. | **Zero-Spool Streaming Gzip (`--compress` / `-z`)**. Direct in-memory streaming from `mysqldump` to `.sql.gz` with zero disk temp files. Transparent streaming decompression on `import`. |
| **Directory Resilience** | Fails or errors if destination folders don't exist; strict, non-standard flag naming. | **Recursive Auto-Creation**. Creates deep nested target folders automatically; interchangeable `-d` / `--dir` and `-o` / `--output-dir`. |
| **Worker Scheduling** | Static modulo partition (`$workerItems[$i % $Workers]`). Workers sit idle for hours when large tables are assigned unevenly. | **Dynamic MPMC Work-Stealing Channel** (`crossbeam-channel`). All workers pull from a central queue sorted LPT. Zero worker starvation. |
| **Process Model** | Heavyweight interpreters (`powershell.exe Start-Job`) consuming 150–300 MB RAM per worker. | **Native OS Threads** (`std::thread`). Sub-millisecond thread startup, < 15 MB total RAM footprint. |
| **I/O & Piping** | Writes stdout/stderr logs to disk per file; polls disk every second with `Get-ChildItem`. | **Zero-Copy Kernel Streaming Pipe**. Streams SQL directly to MySQL stdin via file descriptors. Stderr drained in background; logs written only on error. |
| **Credential Security** | Plaintext credentials in `$env:TEMP` option files; risk of disk leakage on unhandled crashes. | **Zero Plaintext on Disk**. Ephemeral option files with restricted permissions (`0o600`), and process environment variables (`MYSQL_PWD`). |
| **Signal Handling** | Interrupted scripts orphan background `mysql.exe` processes, causing table locks and corrupt state. | **Atomic OS Process Group Termination**. Catches `Ctrl+C` (`SIGINT`/`SIGTERM`) and terminates active child process trees without orphaned locks. |
| **InnoDB Engine Optimization** | Manual or unmanaged; default `innodb_io_capacity = 200` throttles flushing to ~3.2 MB/s. | **Automated Non-Blocking Server Tuning**. Dynamically optimizes `innodb_flush_log_at_trx_commit=2`, `sync_binlog=0`, `innodb_io_capacity=2500`, and change buffering with RAII restoration. |
| **Stateful Resume & Fault Recovery** | Binary pass/fail; requires restarting entire multi-hour dump from scratch if one file fails. | **Atomic Multi-Location Manifest & Targeted Retries**. Stateful JSON manifest (`manifest.json`) with atomic `.tmp` swap, `.bak` protection, and `--resume`. |

---

## Technical Architecture & Core Engine

```
┌─────────────────────────────────────────────────────────────────┐
│                             Zephyr                              │
├─────────────────────────────────────────────────────────────────┤
│ 1. Schema Scanner & Metadata Discovery                          │
│    • High-speed regex header parsing (USE, Database:, CREATE)    │
│    • Non-blocking pre-flight schema creation (utf8mb4)          │
│    • information_schema table size & row estimation             │
├─────────────────────────────────────────────────────────────────┤
│ 2. Server Tuner (RAII Controlled - Import Mode)                 │
│    • Non-blocking variable updates (2s-5s child process timeout)│
│    • innodb_flush_log_at_trx_commit=2, sync_binlog=0             │
│    • innodb_io_capacity=2500, innodb_change_buffer_max_size=50  │
│    • Automatic restoration of original variables on Drop / INT  │
├─────────────────────────────────────────────────────────────────┤
│ 3. Dynamic MPMC Work Distribution                               │
│    • LPT (Longest Processing Time) sorted task queue            │
│    • Unbounded Crossbeam MPMC dynamic work stealing             │
│    • Zero worker starvation during long-running table operations │
├─────────────────────────────────────────────────────────────────┤
│ 4. Worker Streaming Threads (Zero-Copy)                         │
│    • Async background stderr pipe draining (anti-deadlock)      │
│    • Session: foreign_key_checks=0, unique_checks=0              │
│    • Session: transaction_isolation='READ-UNCOMMITTED'          │
│    • Real-time atomic progress & state updates                  │
├─────────────────────────────────────────────────────────────────┤
│ 5. Atomic State Manifest Engine (Import & Export)               │
│    • Multi-location discovery (--manifest, log-dir, source-dir) │
│    • Atomic rename persistence + backup (.bak) protection       │
│    • Case-insensitive and path-stripped table matching          │
│    • Isolated per-table error logs (logs/*.err.log)              │
└─────────────────────────────────────────────────────────────────┘
```

---

## Installation & Compilation

### Prerequisites
- [Rust toolchain](https://rustup.rs/) (1.75+)
- Standard MySQL or MariaDB client binaries (`mysql` / `mysqldump` or `.exe` on Windows).

### Build Optimized Enterprise Release

```bash
cargo build --release
```

The release profile is pre-configured with maximum production optimizations:
- `opt-level = 3` (Full aggressive optimization)
- `lto = "fat"` (Full cross-crate link-time optimization)
- `codegen-units = 1` (Single code generator unit for optimal inlining)
- `panic = "abort"` (Eliminates stack unwinding bloat)
- `strip = true` (Removes debug symbols, shrinking binary size to ~2.7 MB)

The compiled standalone binary is located at:
- **Linux / macOS**: `target/release/zephyr`
- **Windows**: `target/release/zephyr.exe`

To install directly to your system's global binary PATH:
```bash
cargo install --path .
```

---

## Production Usage: PARALLEL EXPORT

Extract tables concurrently across multiple worker threads at maximum throughput, with optional real-time streaming Gzip compression and resilient directory auto-creation:

### 1. Fast Parallel Database Export (with Streaming Compression)
Streams tables directly from `mysqldump` through an in-memory Gzip encoder (`flate2`) to generate `.sql.gz` archives. No uncompressed temporary files are spooled to disk, saving up to 80% storage and disk I/O bandwidth.

```bash
# Linux / macOS (Bash)
zephyr export \
  --database production_db \
  -d ./Dump20260917 \
  --compress \
  --workers 6 \
  --host 127.0.0.1 \
  --user root -p

# Windows (PowerShell)
.\target\release\zephyr.exe export `
  --database production_db `
  -d "D:\Backups\2026\09\production_db" `
  --compress `
  --workers 6 `
  --host 127.0.0.1 `
  -u root -p
```
> [!TIP]
> **Directory Resilience & Ergonomics**:
> - You can use `-d`, `--dir`, `-o`, or `--output-dir` interchangeably.
> - If the target directory path (e.g. `D:\Backups\2026\09\production_db`) does not exist, Zephyr automatically creates the entire directory tree recursively.
> - Under `--dry-run`, missing directories are marked as `(will be created)` without touching disk state.
> - If interrupted or cancelled (`Ctrl+C`), partial uncompressed or corrupted `.sql.gz` files are cleanly removed.

---

### 2. Export All Databases Concurrently
Dumps every non-system database into separate table files in parallel:

```bash
# Linux / macOS (Bash)
zephyr export \
  --all-databases \
  --dir ./EnterpriseFullBackup \
  --compress \
  --workers 8 \
  --host 127.0.0.1 \
  --user root -p

# Windows (PowerShell)
.\target\release\zephyr.exe export `
  --all-databases `
  --dir "D:\Backups\EnterpriseFullBackup" `
  --compress `
  --workers 8 `
  --host 127.0.0.1 `
  -u root -p
```

---

### 3. Selective Table Export & Filtering
Export only specific critical tables, or export a schema while excluding heavy temporary or log tables:

```bash
# Linux / macOS (Bash) - Specific tables only
zephyr export \
  --database production_db \
  -d ./CoreTables \
  --tables "users,orders,order_items,payments" \
  --compress

# Windows (PowerShell) - Exclude audit/log tables
.\target\release\zephyr.exe export `
  --database production_db `
  -d "D:\Backups\CoreSnapshot" `
  --exclude-tables "audit_logs,session_cache,request_traces" `
  --compress
```

---

### 4. Schema-Only or Data-Only Snapshots
```bash
# Linux / macOS (Bash) - Dump DDL structure only (no row data)
zephyr export --database production_db -d ./SchemaOnly --no-data

# Windows (PowerShell) - Dump raw table data only (no CREATE TABLE statements)
.\target\release\zephyr.exe export --database production_db -d "D:\Backups\DataOnly" --no-create-info --compress
```

---

### 5. Dry-Run Pre-Flight Inspection
Inspect table schemas, estimated row counts, and data sizes via `information_schema` without writing any dump files or running `mysqldump`:

```bash
# Linux / macOS (Bash)
zephyr export --database production_db -d ./NonExistentFolder --dry-run

# Windows (PowerShell)
.\target\release\zephyr.exe export --database production_db -d "D:\Backups\FutureDump" --dry-run
```

---

### 6. Resuming an Interrupted Export
If an export was stopped or the network disconnected, `--resume` reads the `manifest.json` state file and instantly skips all previously completed tables:

```bash
# Linux / macOS (Bash)
zephyr export --database production_db --dir ./Dump20260917 --compress --resume

# Windows (PowerShell)
.\target\release\zephyr.exe export --database production_db --dir "D:\Backups\2026\09\production_db" --compress --resume
```

---

## Production Usage: PARALLEL IMPORT

Restore MySQL Workbench dump project folders at wire speed. Zephyr automatically discovers and restores both uncompressed `.sql` files and Gzip-compressed `.sql.gz` dump archives with **transparent on-the-fly streaming decompression** (zero disk temp files):

### 1. High-Performance Bulk Load with Engine Tuning (Recommended)
Automatically enables memory-buffered transaction logging, sets I/O capacity to 2,500 IOPS, buffers secondary indexes in RAM, and allocates a 4 GB buffer pool:

```bash
# Linux / macOS (Bash)
zephyr import \
  --dir "./Dump20260917" \
  --host 127.0.0.1 \
  --user root -p \
  --workers 6 \
  --tune-server \
  --buffer-pool-size 4G \
  --io-capacity 2500

# Windows (PowerShell)
.\target\release\zephyr.exe import `
  -d "D:\Backups\2026\09\production_db" `
  -u root -p `
  --workers 6 `
  --tune-server `
  --buffer-pool-size 4G `
  --io-capacity 2500
```
> [!NOTE]
> - **Zero-Config Compressed Restores**: Dump archives ending in `.sql.gz` are automatically detected and decompressed in-memory directly into the `mysql` client process stdin. You do not need external utilities (`gzip`, `7-Zip`, or `tar`).
> - **Backwards Compatibility**: Running `zephyr -d "..."` without the `import` keyword works identically.

---

### 2. Resuming an Interrupted Ingestion
If an ingestion job was stopped (or cancelled via `Ctrl+C`), restart it with `--resume`. Zephyr automatically discovers `manifest.json` across standard paths and skips all completed tables in milliseconds:

```bash
# Linux / macOS (Bash)
zephyr import --dir "./Dump20260917" --resume --tune-server

# Windows (PowerShell)
.\target\release\zephyr.exe import -d "D:\Backups\2026\09\production_db" --resume --tune-server
```

---

### 3. Dry-Run Verification & Schema Pre-Flight
Inspect table sizes, SQL header detection (scans up to `--scan-depth` lines even inside `.sql.gz` files), and execution order (LPT sorted) without modifying server state:

```bash
# Linux / macOS (Bash)
zephyr import --dir "./Dump20260917" --dry-run

# Windows (PowerShell)
.\target\release\zephyr.exe import -d "D:\Backups\2026\09\production_db" --dry-run
```

---

### 4. Retrying Only Failed Tables
If a subset of tables failed due to bad SQL syntax or network drops, target only the failed files listed in the error log:

```bash
# Linux / macOS (Bash)
zephyr import --dir "./Dump20260917" --retry-file "logs/failed_files.txt"

# Windows (PowerShell)
.\target\release\zephyr.exe import -d "D:\Backups\2026\09\production_db" --retry-file "logs\failed_files.txt"
```

---

### 5. Filtered Table Import
Restore only tables matching a specific substring (e.g. `users` or `orders`):

```bash
# Linux / macOS (Bash)
zephyr import --dir "./Dump20260917" --filter "users"

# Windows (PowerShell)
.\target\release\zephyr.exe import -d "D:\Backups\2026\09\production_db" --filter "users"
```

---

## Persistent Configuration File (`zephyr.toml`)

Zephyr supports persistent configuration via a TOML file, eliminating the need to pass repetitive connection and performance parameters on every CLI invocation.

### Auto-Discovery Order
When `--config` is not explicitly passed, `zephyr` automatically searches for a configuration file in the following order:
1. `./zephyr.toml` (Current Working Directory)
2. `./mysql-zephyr.toml` (Current Working Directory)
3. `./turboload.toml` & `./mysql-turboload.toml` (Backwards-compatibility fallbacks)
4. Platform user configuration directory:
   * **Windows**: `%APPDATA%\zephyr\config.toml` (fallback: `%APPDATA%\mysql-turboload\config.toml`)
   * **Linux / macOS**: `~/.config/zephyr/config.toml` (fallback: `~/.config/mysql-turboload/config.toml`)

### Precedence Hierarchy
Settings resolve according to this strict hierarchy:
1. **Explicit CLI Flags** (e.g. `-H 192.168.1.50`, `-w 16`, `-d ./custom-dir`)
2. **Environment Variables** (e.g. `MYSQL_PWD`)
3. **TOML Configuration File** (`[connection]`, `[import]`, `[export]`)
4. **Built-in Defaults**

To bypass configuration loading entirely (e.g. in CI/CD pipelines), pass the `--no-config` flag. To specify a custom configuration file path, use `--config <FILE>`.

### Example `zephyr.toml`

```toml
[connection]
host = "127.0.0.1"
port = 3306
user = "root"
# password = "secret"    # Optional; CLI prompt or MYSQL_PWD env var recommended for security
charset = "utf8mb4"
max_allowed_packet = "1G"
# mysql_bin = "C:\\Program Files\\MySQL\\MySQL Server 8.0\\bin\\mysql.exe"
# mysqldump_bin = "C:\\Program Files\\MySQL\\MySQL Server 8.0\\bin\\mysqldump.exe"

[import]
dir = "./dumps"
workers = 8
database = "my_database"
tune_server = true
buffer_pool_size = "4G"
io_capacity = 2500
log_dir = "./logs"
resume = true

[export]
dir = "./export-dumps"
compress = true
workers = 8
database = "my_database"
routines = true
events = true
triggers = true
net_buffer_length = "1M"
log_dir = "./export-dumps/logs"
resume = true
```

---

## Complete CLI Reference

### Root & Import Command Options
```text
Usage: zephyr [OPTIONS] [COMMAND]

Commands:
  import  Ingest and restore MySQL dump files in parallel (default command)
  export  Export MySQL database tables in parallel at maximum throughput
  help    Print this message or the help of the given subcommand(s)

Options:
      --config <FILE>                 Explicit path to a TOML configuration file
      --no-config                     Bypass loading any TOML configuration file
  -d, --dir <DIR>                     Directory containing .sql or .sql.gz dump files [default: .]
  -H, --host <HOST>                   MySQL server host [default: 127.0.0.1]
  -P, --port <PORT>                   MySQL server port [default: 3306]
  -u, --user <USER>                   MySQL username [default: root]
  -p, --password <PASSWORD>           MySQL password (or set MYSQL_PWD env var)
  -W, --ask-password                  Always prompt interactively for password
  -w, --workers <COUNT>               Concurrent workers [default: min(CPU cores, 8)]
      --mysql-bin <PATH>              Explicit path to mysql / mysql.exe binary
      --database <NAME>               Force all SQL files into this database
      --default-database <NAME>       Fallback database if header detection fails
      --max-allowed-packet <SIZE>     MySQL max_allowed_packet [default: 1G]
      --charset <CHARSET>             Connection character set [default: utf8mb4]
      --no-disable-foreign-keys       Do not disable foreign key checks during import
      --dry-run                       Display scan results and plan without importing
      --log-dir <DIR>                 Directory for error logs [default: logs]
      --manifest <PATH>               Explicit path to manifest.json (auto-detected if omitted)
      --resume                        Resume previous run, skipping completed files
      --retry-file <FILE>             Only import files listed in failure file
      --filter <PATTERN>              Filter files matching substring (e.g. 'users')
      --scan-depth <LINES>            Max lines to scan in SQL header [default: 100]
      --tune-server                   Optimize InnoDB server variables during import
      --buffer-pool-size <SIZE>       Target InnoDB buffer pool size (e.g. 4G, 8G)
      --io-capacity <IOPS>            Target InnoDB I/O capacity [default with --tune-server: 2500]
      --no-progress                   Disable progress animations (plain text logging)
  -h, --help                          Print help
  -V, --version                       Print version
```

### Export Command Options
```text
Usage: zephyr export [OPTIONS]

Options:
      --config <FILE>                 Explicit path to a TOML configuration file
      --no-config                     Bypass loading any TOML configuration file
  -d, --dir <DIR>                     Directory to store exported dump files [default: export-dumps]
  -z, --compress                      Compress exported dump files with gzip (.sql.gz)
  -B, --database <NAME>               Target single database to export
      --databases <NAMES>             Comma-separated list of databases to export (e.g. "db1,db2")
      --all-databases                 Export all non-system databases
      --tables <TABLES>               Comma-separated list of specific tables to export
      --exclude-tables <TABLES>       Comma-separated list of tables to exclude
  -w, --workers <COUNT>               Concurrent export worker threads [default: min(CPU cores, 8)]
  -H, --host <HOST>                   MySQL server host [default: 127.0.0.1]
  -P, --port <PORT>                   MySQL server port [default: 3306]
  -u, --user <USER>                   MySQL username [default: root]
  -p, --password <PASSWORD>           MySQL password (or set MYSQL_PWD env var)
  -W, --ask-password                  Prompt interactively for password
      --mysqldump-bin <PATH>          Explicit path to mysqldump binary
      --mysql-bin <PATH>              Explicit path to mysql binary
      --resume                        Resume previous export job, skipping completed tables
      --manifest <PATH>               Explicit path to export manifest file
      --dry-run                       Scan table schemas and estimate sizes without dumping
      --no-data                       Dump table structure (schema) only, without data
      --no-create-info                Dump table data only, without CREATE TABLE statements
      --routines                      Include stored procedures and functions [default: true]
      --events                        Include events in export [default: true]
      --triggers                      Include triggers in export [default: true]
      --max-allowed-packet <SIZE>     Network packet size [default: 1G]
      --net-buffer-length <SIZE>      Network buffer length [default: 1M]
      --log-dir <DIR>                 Directory for error logs and manifest [default: <output-dir>/logs]
      --no-progress                   Disable progress bar (plain text output)
  -h, --help                          Print help
```

---

## Development & Code Quality Guidelines

This project adheres to strict Rust development and quality standards:

### 1. Code Formatting
Format all source files according to `.rustfmt.toml`:
```bash
cargo fmt --all
# Verify formatting in CI:
cargo fmt --all -- --check
```

### 2. Linting with Clippy
Enforce zero-warning lint checks:
```bash
cargo clippy --all-targets --all-features -- -D warnings
```

### 3. Automated Unit & Integration Tests
Run comprehensive test suite (including mock MySQL client, resume checks, and export-import roundtrips):
```bash
cargo test --all-targets --verbose
```

---

## License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.
