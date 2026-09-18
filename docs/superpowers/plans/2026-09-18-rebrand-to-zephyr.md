# Rebrand Application to Zephyr Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebrand the application from `mysql-turboload` to `zephyr` ("Zephyr: High-Performance Concurrent MySQL Data Engine"), updating crate metadata, CLI definitions, configuration discovery, banners, build scripts, tests, and documentation.

**Architecture:** Update Cargo package metadata to emit `zephyr` / `zephyr.exe` binary. Adjust CLI attributes and unit tests in `cli.rs`. Rename configuration structs and update candidate discovery paths in `config.rs` with backwards compatibility for `turboload.toml`. Update runtime headers in `main.rs`, suggestion strings in `exporter.rs`/`progress.rs`, integration tests in `e2e_test.rs`, packaging scripts (`build.bat`, `build.ps1`), and complete `README.md` documentation.

**Tech Stack:** Rust 2021 edition, Clap 4.5, Serde, TOML, PowerShell/Batch build scripts.

## Global Constraints

- Crate and binary target name: `zephyr` (executable: `zephyr.exe` on Windows, `zephyr` on Unix)
- Brand title: `Zephyr: High-Performance Concurrent MySQL Data Engine`
- Primary config file: `zephyr.toml`
- Backwards compatibility: Existing `turboload.toml` and `mysql-turboload.toml` files must continue to be discovered as fallbacks if `zephyr.toml` is absent.
- CLI flags and subcommands (`import`, `export`, `-d`/`--dir`, etc.) must maintain exact flag signatures and behaviors.

---

### Task 1: Rebrand Cargo Package & Binary Target Metadata

**Files:**
- Modify: `Cargo.toml:1-10`

**Interfaces:**
- Consumes: Existing package configuration
- Produces: `zephyr` crate and binary target named `zephyr`

- [ ] **Step 1: Update Cargo.toml metadata**

Modify `Cargo.toml` lines 1–10:
```toml
[package]
name = "zephyr"
version = "1.0.0"
edition = "2021"
description = "Zephyr: High-performance concurrent MySQL bulk data engine"

[[bin]]
name = "zephyr"
path = "src/main.rs"
```

- [ ] **Step 2: Run cargo check to verify metadata parsing**

Run: `cargo check`
Expected: Passes with metadata updated to package `zephyr`.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "build: rebrand cargo package and binary target to zephyr"
```

---

### Task 2: Rebrand CLI Definitions and Unit Tests

**Files:**
- Modify: `src/cli.rs:4-15, 360-440`

**Interfaces:**
- Consumes: Clap derive macros
- Produces: `Cli` parser identifying as `zephyr`

- [ ] **Step 1: Update `Cli` struct clap attributes**

Modify `src/cli.rs` lines 4–14:
```rust
#[derive(Parser, Debug, Clone)]
#[command(
    name = "zephyr",
    author = "Enterprise Database Systems",
    version = "1.0.0",
    about = "Zephyr: High-performance concurrent bulk data engine for MySQL & MariaDB",
    long_about = "Zephyr\n\
                  A resilient, high-performance CLI engine designed to restore and export MySQL database\n\
                  tables concurrently. Features dynamic MPMC work-stealing, live server-side InnoDB performance tuning,\n\
                  atomic stateful resume manifests, and high-throughput zero-copy streaming."
)]
pub struct Cli {
```

- [ ] **Step 2: Update CLI unit tests in `src/cli.rs`**

Replace occurrences of `"mysql-turboload"` with `"zephyr"` in `src/cli.rs` test cases:
* Line 360: `Cli::try_parse_from(["zephyr"]).unwrap();`
* Line 366: `Cli::try_parse_from(["zephyr", "export", "-d", "./my-dumps"]).unwrap();`
* Line 377: `"zephyr",`
* Line 398: `Cli::try_parse_from(["zephyr", "export", "-d", "./my-target"]).unwrap();`
* Line 406: `Cli::try_parse_from(["zephyr", "export", "--dir", "./my-target"]).unwrap();`
* Line 416: `Cli::try_parse_from(["zephyr", "export", "--compress"]).unwrap();`
* Line 423: `Cli::try_parse_from(["zephyr", "export", "-z"]).unwrap();`
* Line 433: `Cli::try_parse_from(["zephyr", "-d", "./root-dumps", "export"]).unwrap();`

- [ ] **Step 3: Run CLI unit tests**

Run: `cargo test --lib cli::tests`
Expected: All CLI parsing tests PASS.

- [ ] **Step 4: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): rebrand cli parser name and unit tests to zephyr"
```

---

### Task 3: Rebrand Configuration Module & Discovery Paths

**Files:**
- Modify: `src/config.rs:1-130, 495-535`
- Modify: `src/main.rs:50-80`

**Interfaces:**
- Consumes: `src/config.rs`
- Produces: `ZephyrConfig` struct with prioritized `zephyr.toml` discovery and `turboload.toml` fallbacks

- [ ] **Step 1: Rename `TurboLoadConfig` to `ZephyrConfig` in `src/config.rs` and `src/main.rs`**

In `src/config.rs`:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ZephyrConfig {
    #[serde(default)]
    pub connection: Option<ConnectionConfig>,
    #[serde(default)]
    pub import: Option<ImportConfig>,
    #[serde(default)]
    pub export: Option<ExportConfig>,
}

impl ZephyrConfig {
    pub fn from_toml_str(toml_str: &str) -> Result<Self> {
        let config: Self = toml::from_str(toml_str)?;
        Ok(config)
    }
}

pub fn load_config(path: &Path) -> Result<ZephyrConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read configuration file '{}'", path.display()))?;
    ZephyrConfig::from_toml_str(&content)
        .with_context(|| format!("In configuration file '{}'", path.display()))
}
```
Update signature in `merge_import_config(&mut ImportArgs, &ZephyrConfig, ...)` and `merge_export_config(&mut ExportArgs, &ZephyrConfig, ...)`.

- [ ] **Step 2: Update candidate discovery paths in `find_config_file()`**

In `src/config.rs`:
```rust
    let candidates = [
        PathBuf::from("zephyr.toml"),
        PathBuf::from("mysql-zephyr.toml"),
        PathBuf::from("turboload.toml"),
        PathBuf::from("mysql-turboload.toml"),
    ];
    for c in &candidates {
        if c.is_file() {
            return Ok(Some(c.clone()));
        }
    }

    if let Some(appdata) = std::env::var_os("APPDATA") {
        let zephyr_p = PathBuf::from(&appdata)
            .join("zephyr")
            .join("config.toml");
        if zephyr_p.is_file() {
            return Ok(Some(zephyr_p));
        }
        let legacy_p = PathBuf::from(appdata)
            .join("mysql-turboload")
            .join("config.toml");
        if legacy_p.is_file() {
            return Ok(Some(legacy_p));
        }
    } else if let Some(home) = std::env::var_os("HOME") {
        let zephyr_p = PathBuf::from(&home)
            .join(".config")
            .join("zephyr")
            .join("config.toml");
        if zephyr_p.is_file() {
            return Ok(Some(zephyr_p));
        }
        let legacy_p = PathBuf::from(home)
            .join(".config")
            .join("mysql-turboload")
            .join("config.toml");
        if legacy_p.is_file() {
            return Ok(Some(legacy_p));
        }
    }
```

- [ ] **Step 3: Update `src/config.rs` unit tests**

Update mock parsing strings and test cases in `src/config.rs` from `"mysql-turboload"` to `"zephyr"`.
Add a test verifying `zephyr.toml` discovery.

- [ ] **Step 4: Run configuration unit tests**

Run: `cargo test --lib config::tests`
Expected: All config discovery, parsing, and precedence tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "feat(config): rebrand config struct to ZephyrConfig and prioritize zephyr.toml discovery"
```

---

### Task 4: Rebrand Console Banners, Prompts, and Suggestions

**Files:**
- Modify: `src/main.rs:213-224`
- Modify: `src/exporter.rs:880-888`
- Modify: `src/progress.rs:508-515`

**Interfaces:**
- Consumes: CLI output formatting
- Produces: Consistent `zephyr` terminal banners and retry/resume suggestions

- [ ] **Step 1: Update import banner in `src/main.rs`**

Modify lines 213–224:
```rust
    // Header Banner
    println!();
    println!(
        "{}",
        "==================================================".cyan()
    );
    println!("{}", " ZEPHYR v1.0.0".cyan().bold());
    println!("{}", " High-Performance Concurrent MySQL Data Engine".cyan());
    println!(
        "{}",
        "==================================================".cyan()
    );
```

- [ ] **Step 2: Update suggestions in `src/exporter.rs` and `src/progress.rs`**

In `src/exporter.rs`:
```rust
        println!(
            "{}",
            format!(
                "  zephyr export --resume --manifest {}",
                manifest_path.display()
            )
            .yellow()
        );
```

In `src/progress.rs`:
```rust
        println!("  zephyr --retry-file {}", retry_file.display());
```

- [ ] **Step 3: Run all library tests**

Run: `cargo test --lib`
Expected: All library unit tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs src/exporter.rs src/progress.rs
git commit -m "feat(ui): update console banners and CLI suggestions to Zephyr"
```

---

### Task 5: Update Integration Tests and Packaging Scripts

**Files:**
- Modify: `tests/e2e_test.rs:67, 206, 256, 317`
- Modify: `build.ps1:1-31`
- Modify: `build.bat:1-25`

**Interfaces:**
- Consumes: `cargo build` output
- Produces: `bin/zephyr.exe` binary and clean E2E test passes

- [ ] **Step 1: Update `tests/e2e_test.rs`**

Replace `env!("CARGO_BIN_EXE_mysql-turboload")` with `env!("CARGO_BIN_EXE_zephyr")` in `tests/e2e_test.rs` (lines 67, 206, 317).
Update comments referring to `mysql-turboload` to `zephyr`.

- [ ] **Step 2: Run E2E tests**

Run: `cargo test --test e2e_test`
Expected: All E2E tests PASS.

- [ ] **Step 3: Update `build.ps1` and `build.bat`**

Update `build.ps1`:
```powershell
# PowerShell build script for Zephyr
$ErrorActionPreference = "Stop"

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host " Building Zephyr (Release)" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Error "Build failed with exit code $LASTEXITCODE"
    exit $LASTEXITCODE
}

if (-not (Test-Path "bin")) {
    New-Item -ItemType Directory -Path "bin" | Out-Null
}

$source = "target\release\zephyr.exe"
$dest = "bin\zephyr.exe"

if (Test-Path $source) {
    Copy-Item -Path $source -Destination $dest -Force
    $fileInfo = Get-Item $dest
    $sizeMb = [math]::Round($fileInfo.Length / 1MB, 2)
    Write-Host ""
    Write-Host "[SUCCESS] Release binary built and copied to $dest ($sizeMb MB)" -ForegroundColor Green
} else {
    Write-Error "Release binary not found at $source"
    exit 1
}
```

Update `build.bat`:
```batch
@echo off
setlocal enabledelayedexpansion

echo ==========================================
echo  Building Zephyr (Release)
echo ==========================================

cargo build --release
if %ERRORLEVEL% neq 0 (
    echo [ERROR] Build failed with exit code %ERRORLEVEL%
    exit /b %ERRORLEVEL%
)

if not exist bin mkdir bin

set SOURCE=target\release\zephyr.exe
set DEST=bin\zephyr.exe

if exist "!SOURCE!" (
    copy /Y "!SOURCE!" "!DEST!" >nul
    echo.
    echo [SUCCESS] Release binary built and copied to !DEST!
) else (
    echo [ERROR] Release binary not found at !SOURCE!
    exit /b 1
)
```

- [ ] **Step 4: Execute `build.ps1` to verify packaging**

Run: `powershell -ExecutionPolicy Bypass -File .\build.ps1`
Expected: Successful release build, output copied to `bin\zephyr.exe`.

- [ ] **Step 5: Commit**

```bash
git add tests/e2e_test.rs build.ps1 build.bat
git commit -m "build: update integration test binary references and build scripts for zephyr"
```

---

### Task 6: Rebrand README and Documentation

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: Project features and specs
- Produces: Complete, updated `README.md` reflecting the Zephyr identity

- [ ] **Step 1: Update README.md header, badges, and overview**

Update title:
`# Zephyr (v1.0.0)`
Update overview text to introduce **Zephyr** as the high-performance concurrent MySQL data engine.
Update binary path references:
- Linux / macOS: `target/release/zephyr`
- Windows: `target/release/zephyr.exe`

- [ ] **Step 2: Update all Bash and PowerShell command snippets in README.md**

Replace all invocations:
* `mysql-turboload export ...` $\rightarrow$ `zephyr export ...`
* `.\target\release\mysql-turboload.exe export ...` $\rightarrow$ `.\target\release\zephyr.exe export ...`
* `mysql-turboload import ...` $\rightarrow$ `zephyr import ...`
* `.\target\release\mysql-turboload.exe import ...` $\rightarrow$ `.\target\release\zephyr.exe import ...`
* `mysql-turboload.toml` $\rightarrow$ `zephyr.toml`

- [ ] **Step 3: Update architecture diagrams and comparison table in README.md**

Replace `MySQL TurboLoad Enterprise` in diagram and table headings with `Zephyr`.

- [ ] **Step 4: Verify README.md has zero stale `mysql-turboload` command examples**

Search for `mysql-turboload` in `README.md` to confirm only historical/fallback references remain.

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "docs: rebrand README to Zephyr and update command examples"
```
