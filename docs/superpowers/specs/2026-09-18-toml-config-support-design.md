# Technical Design: Persistent TOML Configuration Support & Unified Directory Option

**Date**: 2026-09-18  
**Topic**: Persistent TOML Configuration Loading, CLI Merging Hierarchy, and Unified Directory Option for MySQL Zephyr  
**Status**: Approved by User

---

## 1. Problem Statement & Motivation

Currently, `zephyr` requires operators to pass all database connection parameters, performance tuning options, worker concurrency counts, and directory paths via CLI flags or environment variables for every execution. In enterprise database operations:

1. **Repetitive CLI Flag Burden**: Long command-line invocations (e.g. `--host 192.168.1.50 --user root --port 3306 --charset utf8mb4 --max-allowed-packet 1G --tune-server --buffer-pool-size 8G --io-capacity 3000`) are error-prone and tedious to manage across ad-hoc executions, cron schedules, and maintenance playbooks.
2. **Lack of Configuration Persistence**: There is no standard mechanism to store environment-specific configurations (such as staging vs. production database credentials or worker profiles) in a version-controlled or localized configuration file.
3. **CLI Option Asymmetry**: Import uses `--dir` / `-d` while Export historically used `--output-dir` / `-o` (with aliases). Unifying this to a clean, symmetrical `--dir` / `-d` without legacy baggage simplifies UX across both commands and in configuration.

---

## 2. Architecture & Design

### 2.1 Overview

The solution introduces:

1. **Strongly Typed TOML Configuration Module (`src/config.rs`)**:
    - Uses `serde` with `toml` (v0.8) to parse structured configuration.
    - Enforces strict schema validation with `#[serde(deny_unknown_fields)]` to catch typos early.
2. **Config Discovery & Loading Engine**:
    - CLI flags: `--config <PATH>` for explicit specification, and `--no-config` to bypass all config loading.
    - Automatic hierarchical candidate search:
        1. `./zephyr.toml` (Current Working Directory)
        2. `./zephyr.toml` (Current Working Directory)
        3. User platform configuration directory (`%APPDATA%\zephyr\config.toml` on Windows, `~/.config/zephyr/config.toml` on Linux/macOS).
3. **Precedence & Merge Engine**:
    - **Precedence Order**: Explicit CLI Arguments > Environment Variables (`MYSQL_PWD`, etc.) > TOML Configuration > Built-in Defaults.
    - Merges shared `[connection]` settings into both `ImportArgs` and `ExportArgs`.
    - Merges command-specific `[import]` and `[export]` settings into their respective arg structs.
4. **Unified `--dir` Option**:
    - Both `import` and `export` use `--dir` / `-d` as the canonical flag for input/output directories.
    - `ExportArgs` defines `pub dir: PathBuf` (default: `"export-dumps"`).
    - TOML sections both use `dir = "..."`.

---

## 3. Configuration Schema Specification

### 3.1 TOML Structure (`zephyr.toml`)

```toml
[connection]
host = "127.0.0.1"
port = 3306
user = "root"
# password = "secret"    # Optional; CLI or env variable preferred for security
charset = "utf8mb4"
max_allowed_packet = "1G"
# mysql_bin = "C:\\Program Files\\MySQL\\MySQL Server 8.0\\bin\\mysql.exe"
# mysqldump_bin = "C:\\Program Files\\MySQL\\MySQL Server 8.0\\bin\\mysqldump.exe"

[import]
dir = "./dumps"
workers = 8
database = "target_db"
# default_database = "fallback_db"
no_disable_foreign_keys = false
tune_server = true
buffer_pool_size = "4G"
io_capacity = 2500
scan_depth = 100
log_dir = "./logs"
resume = false
# filter = "audit_"
# retry_file = "./logs/failed_files.txt"
# manifest = "./logs/manifest.json"
dry_run = false
no_progress = false

[export]
dir = "./export-dumps"
compress = true
workers = 8
database = "source_db"
# databases = "db1,db2"
# all_databases = false
# tables = "users,orders"
# exclude_tables = "audit_log"
routines = true
events = true
triggers = true
net_buffer_length = "1M"
log_dir = "./export-dumps/logs"
resume = false
dry_run = false
no_data = false
no_create_info = false
no_progress = false
```

### 3.2 Rust Structs (`src/config.rs`)

```rust
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct zephyrConfig {
    #[serde(default)]
    pub connection: ConnectionConfig,
    #[serde(default)]
    pub import: ImportConfig,
    #[serde(default)]
    pub export: ExportConfig,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct ConnectionConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub mysql_bin: Option<PathBuf>,
    pub mysqldump_bin: Option<PathBuf>,
    pub max_allowed_packet: Option<String>,
    pub charset: Option<String>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct ImportConfig {
    pub dir: Option<PathBuf>,
    pub workers: Option<usize>,
    pub database: Option<String>,
    pub default_database: Option<String>,
    pub no_disable_foreign_keys: Option<bool>,
    pub dry_run: Option<bool>,
    pub log_dir: Option<PathBuf>,
    pub manifest: Option<PathBuf>,
    pub resume: Option<bool>,
    pub retry_file: Option<PathBuf>,
    pub filter: Option<String>,
    pub scan_depth: Option<usize>,
    pub tune_server: Option<bool>,
    pub buffer_pool_size: Option<String>,
    pub io_capacity: Option<u32>,
    pub no_progress: Option<bool>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct ExportConfig {
    pub dir: Option<PathBuf>,
    pub compress: Option<bool>,
    pub workers: Option<usize>,
    pub database: Option<String>,
    pub databases: Option<String>,
    pub all_databases: Option<bool>,
    pub tables: Option<String>,
    pub exclude_tables: Option<String>,
    pub resume: Option<bool>,
    pub manifest: Option<PathBuf>,
    pub dry_run: Option<bool>,
    pub no_data: Option<bool>,
    pub no_create_info: Option<bool>,
    pub routines: Option<bool>,
    pub events: Option<bool>,
    pub triggers: Option<bool>,
    pub net_buffer_length: Option<String>,
    pub no_progress: Option<bool>,
    pub log_dir: Option<PathBuf>,
}
```

---

## 4. Discovery & Merging Mechanism

### 4.1 CLI Interface Updates

In `Cli`:

```rust
#[derive(Parser, Debug, Clone)]
pub struct Cli {
    /// Explicit path to a TOML configuration file
    #[arg(long = "config", value_name = "FILE", global = true)]
    pub config: Option<PathBuf>,

    /// Bypass loading any TOML configuration file
    #[arg(long = "no-config", global = true)]
    pub no_config: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub import: ImportArgs,
}
```

### 4.2 Candidate Discovery Flow

```rust
pub fn resolve_config_path(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p.to_path_buf());
    }
    let candidates = [
        PathBuf::from("zephyr.toml"),
        PathBuf::from("zephyr.toml"),
    ];
    for c in &candidates {
        if c.is_file() {
            return Some(c.clone());
        }
    }
    if let Some(config_dir) = dirs_next_or_custom() {
        let user_cfg = config_dir.join("zephyr").join("config.toml");
        if user_cfg.is_file() {
            return Some(user_cfg);
        }
    }
    None
}
```

### 4.3 Precedence Checking via Clap `ValueSource`

To distinguish between an explicit user CLI flag and a built-in Clap default, `clap::Command::try_get_matches_from` / `matches.value_source(...)` is utilized:

- If `value_source(id) == Some(ValueSource::CommandLine)`, the CLI value is retained.
- If `value_source(id) == Some(ValueSource::EnvVariable)`, the env value is retained (unless CLI passed).
- If `value_source(id) == Some(ValueSource::DefaultValue)` or argument was omitted:
    - Check the loaded TOML config struct for a present value.
    - If present in TOML, overwrite the field with the TOML value.
    - Otherwise, leave the default intact.

When a configuration file is successfully discovered and parsed, `zephyr` prints a status message:

```text
[CONFIG] Loaded settings from: ./zephyr.toml
```

---

## 5. Unified Directory Symmetrical Redesign

- In `ExportArgs`:
    - Rename field `pub output_dir: PathBuf` $\rightarrow$ `pub dir: PathBuf`.
    - Replace `#[arg(short = 'o', short_alias = 'd', long = "output-dir", alias = "dir", default_value = "export-dumps")]` with:
      `#[arg(short = 'd', long = "dir", value_name = "DIR", default_value = "export-dumps")]`.
    - Remove legacy `-o` / `--output-dir` (user confirmed no backward compatibility needed).
    - Method `resolved_log_dir` updates from `self.output_dir.join("logs")` $\rightarrow$ `self.dir.join("logs")`.
- In `main.rs`:
    - Root `-d` inheritance simplifies:
      `if export_args.dir == std::path::Path::new("export-dumps") && cli.import.dir != std::path::Path::new(".") { export_args.dir = cli.import.dir; }`.
- In `exporter.rs`:
    - Replace `export_args.output_dir` with `export_args.dir`.

---

## 6. Verification & Testing Strategy

1. **Schema & Parsing Tests (`tests/config_tests.rs`)**:
    - Valid full TOML file parsing with all fields populated.
    - Minimal TOML file parsing with missing optional sections/fields.
    - Unknown field rejection verification (`#[serde(deny_unknown_fields)]`).
2. **Precedence & Merge Tests**:
    - CLI overrides TOML values (`--host 10.0.0.1` beats `host = "127.0.0.1"`).
    - TOML values override default values (`workers = 16` beats default `min(num_cpus, 8)`).
    - `--no-config` prevents loading even if `zephyr.toml` exists in the filesystem.
    - Explicit `--config missing.toml` produces a clear descriptive error.
3. **Unified `--dir` Verification**:
    - `zephyr export -d ./custom_dumps` properly sets `dir`.
    - CLI help output for `zephyr export --help` reflects `-d, --dir <DIR>`.
4. **End-to-End Dry-Run Tests**:
    - Run with `--dry-run` and a sample `zephyr.toml` to verify printed banner and runtime settings reflect TOML values.
