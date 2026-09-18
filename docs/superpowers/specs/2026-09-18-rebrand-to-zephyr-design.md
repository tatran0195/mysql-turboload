# Technical Design: Rebranding to Zephyr

**Date**: 2026-09-18  
**Topic**: Comprehensive Rebrand from `mysql-turboload` to `zephyr` ("Zephyr: High-Performance Concurrent MySQL Data Engine")  
**Status**: Approved by User  

---

## 1. Problem Statement & Motivation

The project began under the name `mysql-turboload` as a specialized bulk ingestion engine for MySQL database dumps. Over recent releases, the tool has evolved into a complete, bidirectional high-throughput engine providing:
1. **Parallel Multi-Worker Extraction (`export`)**: Real-time zero-spool Gzip streaming compression (`--compress` / `-z`), recursive directory auto-creation, and LPT task queue balancing.
2. **Parallel Ingestion (`import`)**: Multi-worker bulk restoration, streaming decompression, dynamic MPMC work-stealing, and server-side InnoDB tuning.
3. **Enterprise Resilience**: Atomic JSON state manifests, non-blocking pre-flight schema discovery, and unified TOML configuration loading.

### Why Rebrand?
* **Misleading Scope**: The name "TurboLoad" implies an ingestion-only ("load") utility, neglecting its enterprise-grade export capabilities.
* **CLI Ergonomics**: Typing `mysql-turboload` repeatedly in terminal workflows and automation scripts is verbose and cumbersome.
* **Modern Identity**: Rebranding to **Zephyr** delivers a sleek, poetic, and premium brand identity—evoking featherlight friction, fluid streaming, and effortless movement of heavy database workloads at maximum velocity.

---

## 2. Brand Identity & CLI Specifications

* **Product Name**: **Zephyr**
* **Tagline**: *"High-Performance Concurrent MySQL Data Engine"*
* **Binary Name**: `zephyr` (Windows: `zephyr.exe`)
* **Primary Configuration File**: `zephyr.toml`
* **Command Syntax**:
  ```bash
  # Ingestion / Restoration
  zephyr import -d ./dumps --resume
  zephyr -d ./dumps  # shorthand import default

  # Extraction / Backup
  zephyr export --database production_db -d ./dumps --compress

  # Configuration
  zephyr --config production.toml export --database analytics_db
  ```

---

## 3. Scope of Technical Changes

### 3.1 Cargo Package & Metadata (`Cargo.toml`)
* Rename crate package from `mysql-turboload` to `zephyr`.
* Update description to `"Zephyr: High-performance concurrent MySQL bulk data engine"`.
* Update binary target `[[bin]]` from `name = "mysql-turboload"` to `name = "zephyr"`.

### 3.2 CLI Command Definitions (`src/cli.rs`)
* Update `#[command(...)]` annotations on `Cli`:
  * `name = "zephyr"`
  * `about = "Zephyr: High-performance concurrent bulk data engine for MySQL & MariaDB"`
  * `long_about = "Zephyr\nA resilient, high-performance CLI engine designed to restore and export MySQL database\ntables concurrently..."`
* Update mock invocation strings in CLI unit tests from `"mysql-turboload"` to `"zephyr"`.

### 3.3 Configuration System (`src/config.rs`)
* Rename struct `TurboLoadConfig` $\rightarrow$ `ZephyrConfig`.
* Update automatic config discovery candidate order in `find_config_file()`:
  1. Explicit `--config <FILE>` (highest precedence)
  2. `./zephyr.toml` (primary working directory config)
  3. `./mysql-zephyr.toml`
  4. `./turboload.toml` *(backwards compatibility fallback)*
  5. `./mysql-turboload.toml` *(backwards compatibility fallback)*
  6. OS AppData / Home config paths:
     * Windows: `%APPDATA%\zephyr\config.toml` (fallback: `%APPDATA%\mysql-turboload\config.toml`)
     * Linux / macOS: `~/.config/zephyr/config.toml` (fallback: `~/.config/mysql-turboload/config.toml`)
* Update unit tests to test `ZephyrConfig` and argument parsing with `"zephyr"`.

### 3.4 Terminal Header & User Prompts
* **`src/main.rs`**:
  * Update startup header banner in `run_import`:
    ```text
    ==================================================
     ZEPHYR v1.0.0
     High-Performance Concurrent MySQL Data Engine
    ==================================================
    ```
* **`src/exporter.rs`**:
  * Update resume suggestion string:
    `"  zephyr export --resume --manifest {}"`
* **`src/progress.rs`**:
  * Update retry suggestion string:
    `"  zephyr --retry-file {}"`

### 3.5 Build Scripts (`build.bat` & `build.ps1`)
* Update build script titles and output paths:
  * Destination binary: `bin\zephyr.exe`
  * Release source: `target\release\zephyr.exe`
  * Console banner: `"Building Zephyr (Release)"`

### 3.6 Integration Tests (`tests/e2e_test.rs`)
* Update `env!("CARGO_BIN_EXE_mysql-turboload")` references to `env!("CARGO_BIN_EXE_zephyr")`.
* Ensure mock CLI assertions and commands pass cleanly with the new binary name.

### 3.7 Documentation (`README.md`)
* Update document title to `# Zephyr (v1.0.0)`.
* Update architectural diagrams and comparison tables referencing Zephyr.
* Update all PowerShell and Bash command examples to use `zephyr` and `zephyr.exe`.
* Update configuration guides to reference `zephyr.toml` as the primary configuration file.

---

## 4. Backwards Compatibility & Safety Guarantees

1. **Config File Fallbacks**: Operators with existing `turboload.toml` or `mysql-turboload.toml` files will continue to have their configurations automatically loaded if `zephyr.toml` is not present.
2. **Manifest Schema Integrity**: Existing `manifest.json` files generated by previous runs remain fully compatible and will be discovered and parsed without modification.
3. **Zero Breaking Changes to Flags**: All CLI flags (`-d`, `--dir`, `-H`, `--host`, `-u`, `--user`, `--compress`, etc.) remain 100% identical.

---

## 5. Verification Plan

### 5.1 Automated Unit & Integration Tests
* `cargo test --lib`: Verify CLI argument parsing, configuration loading, and merge hierarchy under the `zephyr` command name.
* `cargo test --test e2e_test`: Verify full end-to-end import/export pipelines with the compiled `zephyr` binary.

### 5.2 Build Verification
* `cargo build --release`: Verify successful compilation and binary emission at `target/release/zephyr.exe`.
* `powershell -File .\build.ps1`: Verify packaging script copies binary to `bin/zephyr.exe`.

### 5.3 Manual CLI Smoke Test
* Run `target/release/zephyr.exe --help` and verify:
  * Binary name is `zephyr`.
  * Description reflects the Zephyr brand.
  * Subcommands `import` and `export` are clearly presented.
