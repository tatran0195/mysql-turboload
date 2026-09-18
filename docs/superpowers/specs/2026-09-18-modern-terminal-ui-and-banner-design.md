# Technical Design: Modern Terminal UI and Banner

**Date**: 2026-09-18  
**Topic**: Modernization of Zephyr Terminal Output UI, Startup Banners, Real-Time Progress, and Completion Cards  
**Status**: Approved by User  

---

## 1. Problem Statement & Motivation

The terminal output of `zephyr` previously utilized basic, retro-style ASCII separators (`============================================================`) and 1990s-style `#>-` progress indicators. The layout lacked visual hierarchy, structured spacing, and modern CLI aesthetics found in leading developer tools (such as Vite, Docker CLI, Cargo, and Supabase).

### Goals
1. **Modern Brand Identity**: Replace dated banner lines with a sleek Unicode rounded box card displaying the `ZEPHYR` name, version badge (`v1.0.0`), and descriptive engine tagline.
2. **Cohesive Key-Value Hierarchy**: Format configuration parameters (host, port, user, binary, folders, workers, workload) using subtle bullet indicators and aligned columns.
3. **Smooth Progress Visuals**: Upgrade `indicatif` progress bars from `#>-` characters to smooth Unicode blocks (`████████░░░░░░░░░░`), retaining real-time EWMA throughput, multi-worker Braille spinners, and ETA.
4. **Framed Execution Summary**: Replace raw text completion reports with framed status cards indicating success (`✔`), error (`✖`), or cancellation (`⚠`), highlighting volume processed, elapsed time, and manifest stats.
5. **Unified Experience**: Apply identical design language to both `zephyr import` and `zephyr export` pipelines.

---

## 2. Visual Design System

### 2.1 Character Set & Box Elements
- **Rounded Box Borders**:
  - Top-left: `╭` (`\u{256D}`)
  - Top-right: `╮` (`\u{256E}`)
  - Bottom-left: `╰` (`\u{2570}`)
  - Bottom-right: `╯` (`\u{256F}`)
  - Horizontal: `─` (`\u{2500}`)
  - Vertical: `│` (`\u{2502}`)
  - Mid-divider tee-left: `├` (`\u{251C}`)
  - Mid-divider tee-right: `┤` (`\u{2524}`)
- **Dividers & Bullets**:
  - Bullet item: `•` (`\u{2022}`) in dimmed styling
  - Section pointer: `▸` (`\u{25B8}`) in cyan
  - Success icon: `✔` (`\u{2714}`) in green bold
  - Failure icon: `✖` (`\u{2716}`) in red bold
  - Warning icon: `⚠` (`\u{26A0}`) in yellow bold
- **Progress Bar Chars**:
  - Smooth block characters: `"█▉▊▋▌▍▎▏ "` for sub-char precision, with dimmed background track `░`.

### 2.2 Color Palette
- **Brand / Structure**: Vivid Cyan (`.cyan()`) for frames and headers.
- **Success / Completed**: Green (`.green()`) for badges, checkmarks, and success cards.
- **Accent / Rates**: Bright Blue / Light Cyan (`.cyan().bold()`) for throughput rates and ETAs.
- **Labels & Dividers**: Dimmed (`.dimmed()`) for subtle field labels, dotted leaders, and background progress tracks.
- **Warning / Resume**: Yellow (`.yellow()`) for skipped tables, retry notifications, and warnings.
- **Error**: Red (`.red()`) for failed tasks and failure cards.

---

## 3. Component Architecture & UI Specifications

### 3.1 Startup Banner & Configuration Card (`src/main.rs` & `src/exporter.rs`)

Both `run_import` and `run_export` will render a 63-column rounded card:

```text
╭─────────────────────────────────────────────────────────────╮
│  ZEPHYR v1.0.0                                              │
│  High-Performance Concurrent MySQL Data Engine              │
├─────────────────────────────────────────────────────────────┤
│  • Target Server : 127.0.0.1:3306                           │
│  • MySQL User    : root                                     │
│  • MySQL Client  : C:\Program Files\MySQL\...\mysql.exe     │
│  • Source Folder : 9837987                                  │
│  • Manifest File : logs\manifest.json                       │
│  • Workers       : 6 threads                                │
│  • Workload      : 106 files (1.81 GB)                      │
╰─────────────────────────────────────────────────────────────╯
```

*(For `export`, labels display `MySQL Dump Bin`, `Output Folder`, and compression mode e.g. `Gzip (Level 6)`).*

#### Resume Mode Parameter Display:
When `--resume` is active, the workload entry cleanly distinguishes between skipped and active items:
- `• Total In Dump : 106 files (1.81 GB)`
- `• Already Done  : 40 files (620.10 MB) [Skipped via manifest]` (in yellow)
- `• To Process    : 66 files (1.19 GB)` (in green bold)

### 3.2 Database Breakdown & Pre-Flight Checks

#### Target Databases Breakdown:
Replaces loose spaces with arrow pointers and leader dots:
```text
Target Databases:
  ▸ 9837987  ······································  106 files    1,851.58 MB
```

#### Pre-Flight Schema Verification:
```text
▸ Verifying database connection & schemas...
  ✔ [1/1] Database '9837987' verified successfully.
```

### 3.3 Execution Start Divider & Progress Tracker (`src/progress.rs`)

#### Execution Start Divider:
```text
─── Starting Parallel Import (6 Workers) ───────────────────────────
```

#### Main Progress Bar:
- Upgrades indicatif bar chars from `#>-` to `"█▉▊▋▌▍▎▏ "` with smooth elapsed time and live throughput metrics:
  ```text
  ⠋ [01:37:07] [████████████████████░░░░░░░░░░░░] 68/106 (64%) | 1.18/1.81 GB (19.4 MB/s) | 6 active | ETA: 00:32
  ```

#### Multi-Worker Sub-Bars:
- Displays active worker prefix `[W1]`, database, table, transferred MB vs total MB, instant transfer speed, and table ETA:
  ```text
    ⠋ [W1] 9837987/audit_log.sql.gz : 45.2/120.0 MB (38%) [12.4 MB/s | ETA: 00:06]
    ⠋ [W2] 9837987/transactions.sql.gz (85.4 MB - 100%) [Finalizing in MySQL... (00:03)]
  ```

### 3.4 Completion Summary Cards (`src/progress.rs` & `src/exporter.rs`)

#### Success Card:
```text
╭─────────────────────────────────────────────────────────────╮
│  ✔ IMPORT COMPLETED SUCCESSFULLY                            │
├─────────────────────────────────────────────────────────────┤
│  • Tables Imported : 106 / 106                              │
│  • Volume Processed: 1.81 GB (1,851.58 MB)                  │
│  • Average Speed   : 19.8 MB/s                              │
│  • Total Duration  : 01:37:07                               │
│  • State Manifest  : logs\manifest.json (106 records)       │
╰─────────────────────────────────────────────────────────────╯
```

#### Failure Card & Details:
```text
╭─────────────────────────────────────────────────────────────╮
│  ✖ IMPORT FINISHED WITH 2 ERRORS                            │
├─────────────────────────────────────────────────────────────┤
│  • Completed       : 104 / 106 tables                       │
│  • Failed          : 2 tables                               │
│  • Volume Processed: 1.72 GB / 1.81 GB                      │
│  • Total Duration  : 01:35:12                               │
╰─────────────────────────────────────────────────────────────╯

Failed Tables:
  ✖ [9837987] transactions.sql.gz (85.40 MB, Exit 1)
    Reason : Table 'transactions' already exists
    Log    : logs\9837987__transactions.sql.gz.err.log

To retry only failed tables:
  zephyr --retry-file logs\failed_files.txt
```

---

## 4. Helper Utilities

A new UI helper module (`src/ui.rs` or shared helper functions in `src/progress.rs`) will provide:
1. `format_bytes(bytes: u64) -> String`: Formats raw bytes into human-readable format (e.g. `1.81 GB (1,851.58 MB)` or `45.20 MB`).
2. Box drawing primitives: Formats framed cards with automatic left/right padding to guarantee box alignment across any terminal width.
3. Windows Console UTF-8 & ANSI check: Ensures colored strings and box characters output correctly in Windows Terminal and standard PowerShell/Command Prompt.

---

## 5. Verification Plan

### 5.1 Automated Unit Tests
- `cargo test --lib`:
  - Test `format_bytes` helper with 0 bytes, KB, MB, GB boundaries.
  - Test progress bar style initialization and ETA calculations.
  - Test box-drawing text formatting functions for uniform column widths.

### 5.2 Manual Verification & CLI Demos
- Run `cargo run -- --help` and dry-run commands:
  - `cargo run -- -d ./test_dumps --dry-run`
  - Verify rounded banner rendering, aligned bullet points, and database summary.
- Run a live sample import/export to inspect:
  - Spinner animation and smooth block progress bar.
  - Finished summary card alignment.
