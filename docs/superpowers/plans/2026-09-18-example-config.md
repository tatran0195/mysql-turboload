# Example Config Template Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a comprehensive, production-ready `zephyr.example.toml` configuration template in the repository root and add automated tests verifying its schema correctness.

**Architecture:** A fully annotated `zephyr.example.toml` configuration file will be provided in the root directory following the approved design. A unit test in `src/config.rs` will read `zephyr.example.toml`, verify that its active defaults deserialize cleanly into `ZephyrConfig`, and verify that uncommented fields match the strict `deny_unknown_fields` schema.

**Tech Stack:** Rust (2021 edition), `toml` crate, `serde`, `clap`.

## Global Constraints

- File placement: `zephyr.example.toml` in repository root.
- Strict schema validation: must adhere to `#[serde(deny_unknown_fields)]` on `ZephyrConfig` and child structs in `src/config.rs`.
- Standard logging convention: commented `log_dir = "./logs"` and `manifest = "./logs/manifest.json"` across both `[import]` and `[export]`.
- No regressions: all existing unit tests in `src/config.rs` and the test suite must pass.

---

### Task 1: Add Unit Test for `zephyr.example.toml` Validity in `src/config.rs`

**Files:**
- Modify: `src/config.rs:526-534`

**Interfaces:**
- Consumes: `ZephyrConfig::from_toml_str(&str) -> Result<ZephyrConfig>`
- Produces: Test `test_zephyr_example_toml_parses_successfully`

- [ ] **Step 1: Write the failing test in `src/config.rs`**

Add the following test to `mod tests` in `src/config.rs`:

```rust
    #[test]
    fn test_zephyr_example_toml_parses_successfully() {
        let example_path = Path::new("zephyr.example.toml");
        assert!(example_path.exists(), "zephyr.example.toml should exist in repository root");
        let content = std::fs::read_to_string(example_path).expect("failed to read zephyr.example.toml");
        let cfg = ZephyrConfig::from_toml_str(&content).expect("zephyr.example.toml should parse cleanly");

        // Verify active default settings
        assert_eq!(cfg.connection.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(cfg.connection.port, Some(3306));
        assert_eq!(cfg.connection.user.as_deref(), Some("root"));
        assert_eq!(cfg.connection.charset.as_deref(), Some("utf8mb4"));
        assert_eq!(cfg.connection.max_allowed_packet.as_deref(), Some("1G"));

        assert_eq!(cfg.import.dir, Some(PathBuf::from("./dumps")));

        assert_eq!(cfg.export.dir, Some(PathBuf::from("./export-dumps")));
        assert_eq!(cfg.export.compress, Some(true));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_zephyr_example_toml_parses_successfully`
Expected: FAIL because `zephyr.example.toml` does not exist yet.

- [ ] **Step 3: Commit the test**

Run:
```bash
git add src/config.rs
git commit -m "test(config): add unit test verifying zephyr.example.toml exists and parses"
```

---

### Task 2: Create `zephyr.example.toml` in Repository Root

**Files:**
- Create: `zephyr.example.toml`

**Interfaces:**
- Consumes: None
- Produces: `zephyr.example.toml` configuration template

- [ ] **Step 1: Create `zephyr.example.toml` with approved configuration structure**

Write the complete configuration template to `zephyr.example.toml`:

```toml
# ==============================================================================
# Zephyr Configuration Template (zephyr.example.toml)
#
# Copy this file to `zephyr.toml` in your current working directory, or to the
# user-level global config directory:
#   - Windows:     %APPDATA%\zephyr\config.toml
#   - Linux/macOS: ~/.config/zephyr/config.toml
#
# Precedence Order:
#   1. Explicit CLI Flags (e.g., -H, -P, -u, -w)
#   2. Environment Variables (e.g., MYSQL_PWD)
#   3. Configuration File (zephyr.toml)
#   4. Built-in Engine Defaults
# ==============================================================================

# ------------------------------------------------------------------------------
# Database Connection Settings
# ------------------------------------------------------------------------------
[connection]
# Target MySQL host address (default: "127.0.0.1")
host = "127.0.0.1"

# Target MySQL port (default: 3306)
port = 3306

# Database user account (default: "root")
user = "root"

# Database user password.
# NOTE: Storing plain passwords in config files is discouraged for security.
# Leave commented out to use the secure CLI prompt or MYSQL_PWD environment variable.
# password = ""

# Explicit path to the `mysql` client binary if not in system PATH.
# Windows example: "C:\\Program Files\\MySQL\\MySQL Server 8.0\\bin\\mysql.exe"
# Linux/macOS example: "/usr/bin/mysql"
# mysql_bin = "mysql"

# Explicit path to the `mysqldump` binary if not in system PATH.
# Windows example: "C:\\Program Files\\MySQL\\MySQL Server 8.0\\bin\\mysqldump.exe"
# Linux/macOS example: "/usr/bin/mysqldump"
# mysqldump_bin = "mysqldump"

# Client character set encoding (default: "utf8mb4")
charset = "utf8mb4"

# Maximum packet size for network buffer (default: "1G")
max_allowed_packet = "1G"

# ------------------------------------------------------------------------------
# Import / Ingestion Settings
# ------------------------------------------------------------------------------
[import]
# Directory containing SQL dump files (.sql, .sql.gz, .sql.zst) to import (default: ".")
dir = "./dumps"

# Concurrency: number of parallel worker threads (default: min(CPU cores, 8))
# workers = 8

# Override database name: forces all dump files into this specific database
# database = "app_production"

# Fallback database: used when a dump file does not specify a 'USE database;' statement
# default_database = "app_production"

# Do not disable foreign key checks during import (default: false, FK checks disabled for speed)
# no_disable_foreign_keys = false

# Directory where per-table error logs and execution logs are stored (default: "./logs")
# log_dir = "./logs"

# Path to state manifest file tracking completed files (default: auto-detected inside log_dir)
# manifest = "./logs/manifest.json"

# Resume ingestion using an existing manifest (skips already imported files)
# resume = true

# Re-run only files listed in a failed files text list
# retry_file = "./logs/failed_files.txt"

# File filter pattern (e.g., "*.sql", "orders_*.sql.gz")
# filter = "*.sql"

# Maximum directory depth to scan recursively for dump files (default: 5)
# scan_depth = 5

# Perform a dry run without modifying the database (default: false)
# dry_run = false

# Disable interactive unicode progress bar and worker tickers (recommended for CI/CD pipelines)
# no_progress = false

# --- High-Performance Server Tuning (Requires MySQL administrative privileges) ---
# Automatically optimize server variables (buffer pool, log buffer, flush log) during import
# tune_server = false

# Target InnoDB buffer pool size when tune_server is enabled (e.g., "4G", "8G")
# buffer_pool_size = "4G"

# Target InnoDB I/O capacity when tune_server is enabled (e.g., 2000)
# io_capacity = 2500

# ------------------------------------------------------------------------------
# Export / Dump Settings
# ------------------------------------------------------------------------------
[export]
# Destination directory for exported SQL dumps (default: "export-dumps")
dir = "./export-dumps"

# Compress SQL dumps using gzip (.sql.gz) on the fly (default: false)
compress = true

# Concurrency: number of parallel worker threads for table dumps (default: min(CPU cores, 8))
# workers = 8

# Target single database to export
# database = "app_production"

# Multiple databases to export (comma-separated list, e.g. "db1,db2,db3")
# databases = "app_production,auth_service"

# Export all accessible databases on the MySQL server
# all_databases = false

# Restrict export to specific tables (comma-separated list)
# tables = "users,orders,products"

# Exclude specific tables from export (comma-separated list)
# exclude_tables = "audit_logs,sessions,temp_cache"

# Path to state manifest file tracking completed table exports (default: auto-saved in log_dir)
# manifest = "./logs/manifest.json"

# Resume an interrupted export session (skips tables already exported in manifest)
# resume = true

# Perform a dry run without running mysqldump or writing files
# dry_run = false

# Dump table structures (DDL) only, omitting row data
# no_data = false

# Dump table rows (DML) only, omitting CREATE TABLE statements
# no_create_info = false

# Include stored procedures and functions in export (default: true)
# routines = true

# Include MySQL scheduled events in export (default: true)
# events = true

# Include database triggers in export (default: true)
# triggers = true

# Network buffer size for mysqldump client communication (e.g., "1M", "16M")
# net_buffer_length = "1M"

# Directory where export error logs are stored (default: "<dir>/logs")
# log_dir = "./logs"

# Disable interactive unicode progress bar and worker tickers (recommended for CI/CD)
# no_progress = false
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test test_zephyr_example_toml_parses_successfully`
Expected: PASS

- [ ] **Step 3: Commit `zephyr.example.toml`**

Run:
```bash
git add zephyr.example.toml
git commit -m "feat(config): add comprehensive zephyr.example.toml configuration template"
```

---

### Task 3: Full Test Suite and Format Verification

**Files:**
- Verify: Entire workspace

- [ ] **Step 1: Run all tests in the workspace**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 2: Format code**

Run: `cargo fmt -- --check`
Expected: Code formatted cleanly.

- [ ] **Step 3: Build release binary**

Run: `cargo build --release`
Expected: Success with `target/release/zephyr.exe` created.
