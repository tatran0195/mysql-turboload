# Zephyr (v1.0.0)

[![CI](https://github.com/enterprise/zephyr/actions/workflows/ci.yml/badge.svg)](https://github.com/enterprise/zephyr/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust 1.75+](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)

**Zephyr** is an ultra-fast, concurrent bulk data migration, restoration, and export engine for MySQL & MariaDB engineered in Rust. Built to replace single-threaded shell scripts and slow dump utilities, Zephyr maximizes hardware throughput with dynamic MPMC work-stealing, real-time in-memory Gzip compression, automated non-blocking InnoDB server tuning, and atomic stateful resume manifests.

---

## Table of Contents

- [Quick Start](#quick-start)
- [Why Zephyr? (Architectural Comparison)](#why-zephyr-architectural-comparison)
- [Core Architecture](#core-architecture)
- [Installation & Building](#installation--building)
- [Export Workflows (`zephyr export`)](#export-workflows-zephyr-export)
- [Import Workflows (`zephyr import`)](#import-workflows-zephyr-import)
- [Configuration File (`zephyr.toml`)](#configuration-file-zephyrtoml)
- [CLI Reference](#cli-reference)
- [Development & Quality Standards](#development--quality-standards)
- [License](#license)

---

## Quick Start

```bash
# 1. High-throughput parallel export with streaming Gzip compression
zephyr export --database production_db -d ./backup --compress --workers 8 -u root -p

# 2. Maximum-speed parallel restoration with automated InnoDB engine tuning
zephyr import -d ./backup --tune-server --buffer-pool-size 4G --workers 8 -u root -p

# 3. Interrupted job? Resume exactly where you left off
zephyr import -d ./backup --resume
```

> [!TIP]
> **Cross-Platform Syntax**: All commands work identically on Linux, macOS, and Windows PowerShell (e.g. `.\bin\zephyr.exe export ...`). Arguments use standard `-d` / `--dir` across both export and import.

---

## Why Zephyr? (Architectural Comparison)

| Capability                  | Traditional Tools (`mysqldump` / Scripts)                                          | Zephyr Engine                                                                                                                                    |
| :-------------------------- | :--------------------------------------------------------------------------------- | :----------------------------------------------------------------------------------------------------------------------------------------------- |
| **Concurrency**             | Single-threaded sequential dump/import; hours on large schemas.                    | **Parallel Multi-Worker Pipeline**. 4–16 native OS worker threads sorted by Longest Processing Time (LPT).                                       |
| **Gzip Compression**        | Spools raw files to disk or pipes external processes (2x disk overhead).           | **Zero-Spool In-Memory Gzip (`--compress` / `-z`)**. Streams direct to `.sql.gz` with zero disk temp files. Transparent decompression on import. |
| **Worker Scheduling**       | Static modulo partition (`task[i % workers]`); idle workers starve.                | **Dynamic MPMC Work Stealing** (`crossbeam-channel`). Central shared queue eliminates worker starvation.                                         |
| **Resource Footprint**      | Heavy interpreter processes (PowerShell / Python / Node) taking 150–300 MB/worker. | **Native Compiled Binary**. Sub-millisecond thread startup, < 15 MB total RAM footprint.                                                         |
| **Server Optimization**     | Manual or unmanaged; default `innodb_io_capacity=200` throttles writes.            | **Automated Server-Side Tuning (`--tune-server`)**. Non-blocking dynamic InnoDB buffer & flushing optimization with RAII restoration.            |
| **Resilience & Resumption** | Binary pass/fail; requires restarting multi-hour jobs from scratch.                | **Atomic State Manifest (`--resume`)**. Thread-safe JSON state manifest with atomic `.tmp` swap and `.bak` protection.                           |
| **Signal Handling**         | `Ctrl+C` leaves orphaned background MySQL child processes and table locks.         | **Process Group Termination**. Gracefully drains and terminates child process trees on `SIGINT` / `SIGTERM`.                                     |

---

## Core Architecture

```text
┌─────────────────────────────────────────────────────────────────┐
│                             Zephyr                              │
├─────────────────────────────────────────────────────────────────┤
│ 1. Schema Scanner & Pre-Flight Discovery                        │
│    • High-speed regex header parsing (USE, Database:, CREATE)    │
│    • information_schema table size & row count estimation       │
│    • Non-blocking pre-flight schema creation (utf8mb4)          │
├─────────────────────────────────────────────────────────────────┤
│ 2. Server Tuner (RAII Controlled - Import Mode)                 │
│    • Dynamic innodb_flush_log_at_trx_commit=2, sync_binlog=0    │
│    • Dynamic innodb_io_capacity=2500, change_buffer_max_size=50 │
│    • Guaranteed automatic restoration of original variables     │
├─────────────────────────────────────────────────────────────────┤
│ 3. Dynamic MPMC Work Distribution                               │
│    • LPT (Longest Processing Time) priority task queue          │
│    • Unbounded Crossbeam MPMC work-stealing channel             │
│    • Zero worker starvation during long-running table tasks     │
├─────────────────────────────────────────────────────────────────┤
│ 4. Worker Streaming Threads (Zero-Copy)                         │
│    • Async background stderr pipe draining (anti-deadlock)      │
│    • Optimized session variables (foreign_key_checks=0, etc.)   │
│    • Real-time atomic progress & state reporting                │
├─────────────────────────────────────────────────────────────────┤
│ 5. Atomic State Manifest Engine                                 │
│    • Auto-discovery across --manifest, log-dir, and target-dir  │
│    • Atomic rename persistence with .bak safety guarantees      │
│    • Isolated per-table error logs (logs/*.err.log)              │
└─────────────────────────────────────────────────────────────────┘
```

---

## Installation & Building

### Prerequisites

- [Rust toolchain](https://rustup.rs/) (1.75 or later)
- Standard MySQL or MariaDB client utilities (`mysql`, `mysqldump`) installed on the host system or system PATH.

### Build from Source

```bash
# Clone the repository
git clone https://github.com/enterprise/zephyr.git
cd zephyr

# Build optimized production release
cargo build --release
```

The release profile compiles with maximum optimizations (`opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, stripped symbols), producing a single self-contained ~3 MB binary:

- **Linux / macOS**: `target/release/zephyr`
- **Windows**: `target/release/zephyr.exe` (or use `./build.ps1` to place into `bin/zephyr.exe`)

To install into your user cargo binary PATH:

```bash
cargo install --path .
```

---

## Export Workflows (`zephyr export`)

Extract MySQL database tables in parallel with high-throughput streaming and optional Gzip compression.

### 1. Basic Parallel Export

Extract a database into Workbench-compatible `.sql` files across 8 parallel workers:

```bash
zephyr export --database production_db -d ./backup -w 8 -u root -p
```

### 2. Export with Streaming Gzip Compression (`--compress` / `-z`)

Streams output directly into compressed `.sql.gz` archives in memory with zero intermediate disk files:

```bash
zephyr export --database production_db -d ./backup --compress -u root -p
```

### 3. Export All Non-System Databases

Concurrently exports every database into organized target directories:

```bash
zephyr export --all-databases -d ./full_backup --compress -w 8 -u root -p
```

### 4. Selective Table Extraction / Exclusions

```bash
# Export only critical tables:
zephyr export --database production_db -d ./core_backup --tables "users,orders,payments"

# Export schema while excluding heavy log/audit tables:
zephyr export --database production_db -d ./clean_backup --exclude-tables "audit_logs,cache"
```

### 5. Structure-Only or Data-Only Dumps

```bash
# Schema DDL structure only (no row data):
zephyr export --database production_db -d ./schema_only --no-data

# Table row data only (no CREATE TABLE statements):
zephyr export --database production_db -d ./data_only --no-create-info --compress
```

### 6. Dry-Run & Resume

```bash
# Inspect table sizes and plan without exporting:
zephyr export --database production_db -d ./backup --dry-run

# Resume an interrupted export (skips completed tables via manifest.json):
zephyr export --database production_db -d ./backup --resume --compress
```

---

## Import Workflows (`zephyr import`)

Restore Workbench-compatible dump directories and `.sql.gz` archives at maximum wire speed.

### 1. High-Speed Bulk Restoration with Engine Tuning (`--tune-server`)

Temporarily adjusts InnoDB parameters to maximize disk throughput and memory buffering, restoring original settings safely on completion:

```bash
zephyr import -d ./backup --tune-server --buffer-pool-size 4G --io-capacity 2500 -w 8 -u root -p
```

> [!NOTE]
> **Transparent Decompression**: `.sql.gz` files are automatically decompressed in memory and streamed into MySQL stdin. No external archiving tools are required.

### 2. Default Command Shorthand

Running `zephyr -d <DIR>` without a subcommand automatically defaults to `import`:

```bash
zephyr -d ./backup -u root -p
```

### 3. Resuming Interrupted Imports

If an import job is interrupted, rerun with `--resume` to skip already-imported tables in milliseconds:

```bash
zephyr import -d ./backup --resume --tune-server
```

### 4. Retrying Failed Tables

If specific tables failed due to external locks or syntax issues, execute only the failed items:

```bash
zephyr import -d ./backup --retry-file "logs/failed_files.txt"
```

### 5. Dry-Run & Filtered Restoration

```bash
# Validate headers and display execution schedule without importing:
zephyr import -d ./backup --dry-run

# Restore only tables matching a pattern (e.g. 'order'):
zephyr import -d ./backup --filter "order"
```

---

## Configuration File (`zephyr.toml`)

Zephyr supports persistent configuration via TOML, removing the need to specify repetitive connection flags on every CLI invocation.

### Auto-Discovery Order

When `--config` is not explicitly passed, Zephyr automatically checks:

1. `./zephyr.toml` _(Current Working Directory - Primary)_
2. `./mysql-zephyr.toml` _(Current Working Directory)_
3. User global config directory:
    - **Windows**: `%APPDATA%\zephyr\config.toml`
    - **Linux / macOS**: `~/.config/zephyr/config.toml`

### Precedence Hierarchy

1. **Explicit CLI Flags** (`-H`, `-w`, `-d`, etc.)
2. **Environment Variables** (`MYSQL_PWD`)
3. **TOML Configuration File**
4. **Built-in Engine Defaults**

Pass `--no-config` to bypass configuration files (ideal for CI/CD), or `--config <FILE>` to specify an explicit path.

### Example `zephyr.toml`

```toml
[connection]
host = "127.0.0.1"
port = 3306
user = "root"
# password = "secret"    # Optional; CLI prompt or MYSQL_PWD env var recommended
charset = "utf8mb4"
max_allowed_packet = "1G"

[import]
dir = "./backup"
workers = 8
database = "production_db"
tune_server = true
buffer_pool_size = "4G"
io_capacity = 2500
log_dir = "./logs"
resume = true

[export]
dir = "./backup"
compress = true
workers = 8
database = "production_db"
routines = true
events = true
triggers = true
net_buffer_length = "1M"
log_dir = "./backup/logs"
resume = true
```

---

## CLI Reference

### Global Options

| Option            | Short | Description                                | Default         |
| :---------------- | :---- | :----------------------------------------- | :-------------- |
| `--config <FILE>` |       | Explicit path to a TOML configuration file | Auto-discovered |
| `--no-config`     |       | Bypass loading any TOML configuration file | `false`         |
| `--help`          | `-h`  | Print help information                     |                 |
| `--version`       | `-V`  | Print version                              |                 |

### Connection Options (Shared)

| Option                 | Short | Environment | Description                              | Default            |
| :--------------------- | :---- | :---------- | :--------------------------------------- | :----------------- |
| `--host <HOST>`        | `-H`  |             | MySQL server host                        | `127.0.0.1`        |
| `--port <PORT>`        | `-P`  |             | MySQL server port                        | `3306`             |
| `--user <USER>`        | `-u`  |             | MySQL username                           | `root`             |
| `--password <PWD>`     | `-p`  | `MYSQL_PWD` | MySQL password                           | Interactive prompt |
| `--ask-password`       | `-W`  |             | Always prompt interactively for password | `false`            |
| `--mysql-bin <PATH>`   |       |             | Path to `mysql` binary                   | Auto-detected      |
| `--charset <CHAR>`     |       |             | Client connection charset                | `utf8mb4`          |
| `--max-allowed-packet` |       |             | Maximum network packet size              | `1G`               |

### Import Command Options (`zephyr import` or root)

| Option                      | Short | Description                                        | Default           |
| :-------------------------- | :---- | :------------------------------------------------- | :---------------- |
| `--dir <DIR>`               | `-d`  | Source directory containing dump files             | `.`               |
| `--workers <COUNT>`         | `-w`  | Concurrent worker thread count                     | `min(CPUs, 8)`    |
| `--database <NAME>`         |       | Target database name (overrides SQL headers)       | Detected from SQL |
| `--default-database <NAME>` |       | Fallback database if header detection fails        | None              |
| `--tune-server`             |       | Auto-tune InnoDB server variables during import    | `false`           |
| `--buffer-pool-size <SIZE>` |       | Target InnoDB buffer pool size for tuning          | `4G`              |
| `--io-capacity <IOPS>`      |       | Target InnoDB I/O capacity (IOPS)                  | `2500`            |
| `--resume`                  |       | Resume interrupted import skipping finished tables | `false`           |
| `--retry-file <FILE>`       |       | Import only files listed in failure log            | None              |
| `--dry-run`                 |       | Display scan and plan without importing            | `false`           |
| `--filter <PATTERN>`        |       | Filter dump files by substring/pattern             | None              |
| `--log-dir <DIR>`           |       | Directory for error logs and diagnostics           | `logs`            |
| `--manifest <PATH>`         |       | Explicit path to `manifest.json`                   | Auto-detected     |
| `--no-progress`             |       | Disable dynamic animation (plain text output)      | `false`           |

### Export Command Options (`zephyr export`)

| Option                     | Short | Description                                 | Default        |
| :------------------------- | :---- | :------------------------------------------ | :------------- |
| `--dir <DIR>`              | `-d`  | Destination directory for export files      | `export-dumps` |
| `--compress`               | `-z`  | Stream Gzip compression (`.sql.gz`)         | `false`        |
| `--database <NAME>`        | `-B`  | Target single database to export            | None           |
| `--databases <NAMES>`      |       | Comma-separated list of databases to export | None           |
| `--all-databases`          |       | Export all non-system databases             | `false`        |
| `--tables <NAMES>`         |       | Comma-separated list of tables to include   | All tables     |
| `--exclude-tables <NAMES>` |       | Comma-separated list of tables to exclude   | None           |
| `--workers <COUNT>`        | `-w`  | Concurrent worker thread count              | `min(CPUs, 8)` |
| `--mysqldump-bin <PATH>`   |       | Path to `mysqldump` binary                  | Auto-detected  |
| `--no-data`                |       | Export table structure only (no rows)       | `false`        |
| `--no-create-info`         |       | Export table rows only (no DDL)             | `false`        |
| `--routines`               |       | Include stored procedures and functions     | `true`         |
| `--events`                 |       | Include events                              | `true`         |
| `--triggers`               |       | Include triggers                            | `true`         |
| `--net-buffer-length`      |       | Network buffer length                       | `1M`           |
| `--resume`                 |       | Resume interrupted export job               | `false`        |
| `--dry-run`                |       | Estimate sizes and schemas without dumping  | `false`        |
| `--log-dir <DIR>`          |       | Directory for logs and manifest             | `<dir>/logs`   |

---

## Development & Quality Standards

This project enforces strict Rust development, linting, and formatting standards:

```bash
# 1. Format code
cargo fmt --all

# 2. Enforce zero-warning Clippy linting
cargo clippy --all-targets --all-features -- -D warnings

# 3. Run full test suite
cargo test --all-targets --verbose
```

---

## License

Distributed under the MIT License. See [LICENSE](LICENSE) for full details.
