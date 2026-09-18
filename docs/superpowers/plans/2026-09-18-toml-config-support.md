# Persistent TOML Configuration Support & Unified Directory Option Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement persistent TOML configuration file support (`turboload.toml`) with automated candidate discovery, CLI flag precedence, strict schema validation, and unified `--dir` option for import and export.

**Architecture:** A new strongly-typed `src/config.rs` module deserializes TOML configs via `toml` (v0.8) and `serde`. Configuration auto-discovery searches `./turboload.toml`, `./mysql-turboload.toml`, and platform `%APPDATA%` / `~/.config` dirs. A merge layer applies config values to `ImportArgs` and `ExportArgs` where CLI flags were not explicitly passed, respecting CLI > Env > TOML > Defaults. `ExportArgs` is unified with `ImportArgs` to use canonical `--dir` / `-d`.

**Tech Stack:** Rust 2021 edition, `clap` 4.5 (derive & value_source), `serde` 1.0 (derive), `toml` 0.8, `colored` 2.1, `anyhow` 1.0.

## Global Constraints
- Unified directory option: both import and export must use `-d, --dir <DIR>` (remove legacy `-o, --output-dir`).
- Precedence order: CLI Arguments > Environment Variables > TOML File > Built-in Defaults.
- Auto-discovery order: `./turboload.toml`, `./mysql-turboload.toml`, `%APPDATA%\mysql-turboload\config.toml` (or `~/.config/mysql-turboload/config.toml`).
- Schema validation: `#[serde(deny_unknown_fields)]` to prevent silent misconfigurations.
- CLI flags: `--config <PATH>` (explicit path) and `--no-config` (bypass all config loading).
- All tests must pass: `cargo test --bin mysql-turboload`.

---

### Task 1: Add `toml` Dependency, Unify `--dir` Flag, and Add Config Flags in `cli.rs`

**Files:**
- Modify: `Cargo.toml:23-29`
- Modify: `src/cli.rs:15-30,178-220,315-345,350-440`
- Modify: `src/main.rs:37-45`
- Modify: `src/exporter.rs:35-50,150-170`

**Interfaces:**
- Consumes: `clap::Parser`, `clap::Args`
- Produces:
  - `Cli::config: Option<PathBuf>`
  - `Cli::no_config: bool`
  - `ExportArgs::dir: PathBuf` (replaces `output_dir`)

- [ ] **Step 1: Update `Cargo.toml` with `toml` dependency**

In `Cargo.toml` under `[dependencies]`:
```toml
toml = "0.8"
```

- [ ] **Step 2: Update `src/cli.rs` with `--config`, `--no-config`, and unified `ExportArgs.dir`**

In `src/cli.rs`:
1. In `Cli`:
```rust
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
2. In `ExportArgs`:
```rust
    /// Output directory to store exported .sql files
    #[arg(
        short = 'd',
        long = "dir",
        value_name = "DIR",
        default_value = "export-dumps"
    )]
    pub dir: PathBuf,
```
Remove `-o` and `--output-dir`. Update `resolved_log_dir()`:
```rust
    pub fn resolved_log_dir(&self) -> PathBuf {
        self.log_dir
            .clone()
            .unwrap_or_else(|| self.dir.join("logs"))
    }
```
3. Update tests in `src/cli.rs` to test `-d` and `--dir` on export instead of `-o` / `--output-dir`.

- [ ] **Step 3: Update `src/main.rs` and `src/exporter.rs` references from `output_dir` to `dir`**

In `src/main.rs`:
```rust
        Some(Commands::Export(mut export_args)) => {
            if export_args.dir == std::path::Path::new("export-dumps")
                && cli.import.dir != std::path::Path::new(".")
            {
                export_args.dir = cli.import.dir;
            }
            exporter::run_export(export_args)
        }
```
In `src/exporter.rs`:
Replace `export_args.output_dir` with `export_args.dir`.

- [ ] **Step 4: Run unit tests to verify compilation and unified `--dir` behavior**

Run: `cargo test --bin mysql-turboload`
Expected: All tests pass (including updated `cli::tests`).

- [ ] **Step 5: Commit changes**

```bash
git add Cargo.toml Cargo.lock src/cli.rs src/main.rs src/exporter.rs
git commit -m "feat(cli): add --config / --no-config flags and unify export directory to --dir"
```

---

### Task 2: Implement TOML Configuration Models & Deserialization in `src/config.rs`

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs:1-12`

**Interfaces:**
- Consumes: `serde::Deserialize`, `toml::from_str`
- Produces:
  - `TurboLoadConfig`
  - `ConnectionConfig`
  - `ImportConfig`
  - `ExportConfig`
  - `TurboLoadConfig::from_toml_str(content: &str) -> Result<TurboLoadConfig, anyhow::Error>`

- [ ] **Step 1: Write failing unit tests in `src/config.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_full_valid_toml() {
        let toml_str = r#"
            [connection]
            host = "192.168.1.100"
            port = 3307
            user = "dbadmin"
            password = "securepassword"
            charset = "utf8mb4"
            max_allowed_packet = "2G"

            [import]
            dir = "./test-dumps"
            workers = 12
            database = "test_db"
            tune_server = true
            buffer_pool_size = "8G"
            io_capacity = 3000
            resume = true

            [export]
            dir = "./export-target"
            compress = true
            workers = 16
            database = "source_db"
            routines = true
            events = true
            triggers = true
        "#;
        let cfg = TurboLoadConfig::from_toml_str(toml_str).unwrap();
        assert_eq!(cfg.connection.host.as_deref(), Some("192.168.1.100"));
        assert_eq!(cfg.connection.port, Some(3307));
        assert_eq!(cfg.import.dir, Some(PathBuf::from("./test-dumps")));
        assert_eq!(cfg.import.workers, Some(12));
        assert_eq!(cfg.export.dir, Some(PathBuf::from("./export-target")));
        assert_eq!(cfg.export.compress, Some(true));
    }

    #[test]
    fn test_parse_minimal_empty_toml() {
        let toml_str = "";
        let cfg = TurboLoadConfig::from_toml_str(toml_str).unwrap();
        assert!(cfg.connection.host.is_none());
        assert!(cfg.import.dir.is_none());
        assert!(cfg.export.dir.is_none());
    }

    #[test]
    fn test_reject_unknown_fields() {
        let toml_str = r#"
            [import]
            worker = 8  # typo: should be workers
        "#;
        let res = TurboLoadConfig::from_toml_str(toml_str);
        assert!(res.is_err(), "Expected error on unknown field 'worker'");
    }
}
```

- [ ] **Step 2: Run test to verify failure before implementation**

Run: `cargo test --bin mysql-turboload config::tests`
Expected: FAIL (module or functions not found).

- [ ] **Step 3: Implement `src/config.rs` structs and deserializer**

Create `src/config.rs`:
```rust
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct TurboLoadConfig {
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

impl TurboLoadConfig {
    pub fn from_toml_str(content: &str) -> Result<Self> {
        toml::from_str(content).context("Failed to parse TOML configuration")
    }
}
```
Register `mod config;` in `src/main.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin mysql-turboload config::tests`
Expected: PASS with 3 tests passing.

- [ ] **Step 5: Commit changes**

```bash
git add src/config.rs src/main.rs
git commit -m "feat(config): implement strongly-typed TOML deserialization with strict validation"
```

---

### Task 3: Implement Config File Discovery, Precedence Merging & Banner Notification

**Files:**
- Modify: `src/config.rs`
- Modify: `src/main.rs:25-50`

**Interfaces:**
- Consumes: `clap::Command::try_get_matches_from`, `clap::parser::ValueSource`, `std::env`
- Produces:
  - `find_config_file(explicit: Option<&Path>) -> Result<Option<PathBuf>>`
  - `load_config(path: &Path) -> Result<TurboLoadConfig>`
  - `merge_import_config(args: &mut ImportArgs, cfg: &TurboLoadConfig, matches: &clap::ArgMatches)`
  - `merge_export_config(args: &mut ExportArgs, cfg: &TurboLoadConfig, matches: &clap::ArgMatches)`

- [ ] **Step 1: Write failing tests for file discovery and merging logic in `src/config.rs`**

```rust
    #[test]
    fn test_merge_import_respects_cli_precedence() {
        let toml_str = r#"
            [connection]
            host = "10.0.0.1"
            port = 3307

            [import]
            dir = "./toml-dumps"
            workers = 16
        "#;
        let cfg = TurboLoadConfig::from_toml_str(toml_str).unwrap();

        // Simulate CLI where host was passed explicitly, but workers and dir were not
        let cli = Cli::try_parse_from(["mysql-turboload", "-H", "192.168.1.1"]).unwrap();
        let matches = Cli::command().get_matches_from(["mysql-turboload", "-H", "192.168.1.1"]);
        let mut import_args = cli.import;
        merge_import_config(&mut import_args, &cfg, &matches);

        assert_eq!(import_args.host, "192.168.1.1"); // CLI overrides TOML
        assert_eq!(import_args.port, 3307);          // TOML overrides default 3306
        assert_eq!(import_args.workers, Some(16));   // TOML overrides default None
        assert_eq!(import_args.dir, PathBuf::from("./toml-dumps")); // TOML overrides default "."
    }

    #[test]
    fn test_merge_export_respects_cli_precedence() {
        let toml_str = r#"
            [connection]
            host = "10.0.0.2"

            [export]
            dir = "./toml-exports"
            compress = true
        "#;
        let cfg = TurboLoadConfig::from_toml_str(toml_str).unwrap();

        let cli = Cli::try_parse_from(["mysql-turboload", "export", "-d", "./cli-exports"]).unwrap();
        let matches = Cli::command().get_matches_from(["mysql-turboload", "export", "-d", "./cli-exports"]);
        let sub_matches = matches.subcommand_matches("export").unwrap();
        if let Some(Commands::Export(mut export_args)) = cli.command {
            merge_export_config(&mut export_args, &cfg, sub_matches);
            assert_eq!(export_args.dir, PathBuf::from("./cli-exports")); // CLI overrides TOML
            assert_eq!(export_args.host, "10.0.0.2");                  // TOML overrides default 127.0.0.1
            assert!(export_args.compress);                              // TOML overrides default false
        } else {
            panic!("Expected export command");
        }
    }
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test --bin mysql-turboload config::tests`
Expected: FAIL (functions not yet defined).

- [ ] **Step 3: Implement discovery, loading, and merging functions in `src/config.rs`**

Add candidate path resolution:
```rust
pub fn find_config_file(explicit: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(p) = explicit {
        if p.is_file() {
            return Ok(Some(p.to_path_buf()));
        }
        anyhow::bail!("Configuration file not found: {}", p.display());
    }

    let candidates = [
        PathBuf::from("turboload.toml"),
        PathBuf::from("mysql-turboload.toml"),
    ];
    for c in &candidates {
        if c.is_file() {
            return Ok(Some(c.clone()));
        }
    }

    if let Some(appdata) = std::env::var_os("APPDATA") {
        let p = PathBuf::from(appdata).join("mysql-turboload").join("config.toml");
        if p.is_file() {
            return Ok(Some(p));
        }
    } else if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home).join(".config").join("mysql-turboload").join("config.toml");
        if p.is_file() {
            return Ok(Some(p));
        }
    }

    Ok(None)
}

pub fn load_config(path: &Path) -> Result<TurboLoadConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read configuration file '{}'", path.display()))?;
    TurboLoadConfig::from_toml_str(&content)
        .with_context(|| format!("In configuration file '{}'", path.display()))
}
```

Implement `merge_import_config` and `merge_export_config` inspecting `matches.value_source(id) == Some(clap::parser::ValueSource::CommandLine)`.
If not from CommandLine:
- For `host`, `port`, `user`, `password`, `mysql_bin`, `charset`, `max_allowed_packet`: copy from `cfg.connection` if present, then from `cfg.import` if present.
- For command-specific fields: copy from `cfg.import` / `cfg.export`.

- [ ] **Step 4: Integrate config loading into `src/main.rs`**

In `main.rs::run()`:
```rust
    let cli = Cli::parse();

    // Check if configuration should be loaded
    let config = if cli.no_config {
        None
    } else {
        let config_path = config::find_config_file(cli.config.as_deref())?;
        if let Some(ref path) = config_path {
            println!("{}", format!("[CONFIG] Loaded configuration from: {}", path.display()).cyan());
            Some(config::load_config(path)?)
        } else {
            None
        }
    };
```
Merge config into `import_args` or `export_args` before invoking `run_import` or `exporter::run_export`.

- [ ] **Step 5: Run tests to verify discovery, precedence, and merging**

Run: `cargo test --bin mysql-turboload`
Expected: PASS (all unit tests pass).

- [ ] **Step 6: Commit changes**

```bash
git add src/config.rs src/main.rs
git commit -m "feat(config): integrate config discovery, CLI precedence merging, and banner"
```

---

### Task 4: Documentation Updates & End-to-End Verification

**Files:**
- Modify: `README.md`
- Create / Test: temporary scratch TOML file for verification

- [ ] **Step 1: Update `README.md`**

Add documentation for:
1. Unified `-d, --dir` option for both import and export commands.
2. The `turboload.toml` configuration file format with sample `[connection]`, `[import]`, and `[export]` blocks.
3. Discovery paths and precedence rules (CLI > Env > TOML > Defaults).
4. `--config <FILE>` and `--no-config` flag reference.

- [ ] **Step 2: Run dry-run verification with a sample `turboload.toml`**

Run:
1. Create temporary `turboload.toml` with `workers = 7`, `port = 3309`.
2. Execute: `cargo run -- --dry-run`
3. Verify output logs `[CONFIG] Loaded configuration from: turboload.toml` and displays `Target Server: 127.0.0.1:3309` and `Workers: 7`.
4. Execute with CLI override: `cargo run -- --dry-run -P 3310`
5. Verify `Target Server: 127.0.0.1:3310` (CLI flag overrides TOML).
6. Execute with bypass: `cargo run -- --dry-run --no-config`
7. Verify `[CONFIG]` is not logged and default port `3306` is used.
8. Clean up test TOML file.

- [ ] **Step 3: Run the complete test suite**

Run: `cargo test --bin mysql-turboload`
Expected: 100% PASS with no warnings.

- [ ] **Step 4: Commit changes**

```bash
git add README.md
git commit -m "docs: document TOML config support, discovery paths, and unified --dir option"
```
