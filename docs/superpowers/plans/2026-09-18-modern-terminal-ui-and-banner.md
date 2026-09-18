# Modern Terminal UI and Banner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Modernize the Zephyr CLI terminal output across both import and export pipelines with sleek Unicode rounded box cards, styled configuration parameters, smooth block progress indicators, and framed completion summary cards.

**Architecture:** Create a dedicated, reusable `src/ui.rs` module providing card rendering, character primitives, and byte formatting. Integrate this module into `src/main.rs`, `src/exporter.rs`, and `src/progress.rs` to replace legacy `====` banners and `#>-` progress bars with a unified visual system.

**Tech Stack:** Rust (2021 edition), `colored` crate (terminal colors & styles), `indicatif` (console progress bars & spinners), Unicode box-drawing characters (`\u{256D}`..`\u{2570}`).

## Global Constraints

- Preserve all existing CLI arguments, flags, configuration schema, and manifest formats.
- Box cards must maintain a standard width (63 characters) and adjust padding dynamically to prevent border breakage regardless of dynamic text length.
- Terminal styling must safely handle both Windows Terminal and ANSI-capable consoles without throwing encoding errors.

---

### Task 1: UI Formatting & Card Primitives (`src/ui.rs`)

**Files:**
- Create: `src/ui.rs`
- Modify: `src/main.rs:1-12` (add `mod ui;`)

**Interfaces:**
- Produces:
  - `pub fn format_bytes(bytes: u64) -> String`
  - `pub fn format_bytes_compact(bytes: u64) -> String`
  - `pub fn render_card(title: &str, version: &str, subtitle: &str, items: &[(&str, String)]) -> String`
  - `pub fn render_summary_card(status_title: &str, is_success: bool, is_cancelled: bool, metrics: &[(&str, String)]) -> String`
  - `pub fn render_section_divider(title: &str) -> String`

- [ ] **Step 1: Write the failing test in `src/ui.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes_representation() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GB (1,024.00 MB)");
    }

    #[test]
    fn test_render_card_structure() {
        let items = [("Target Server", "127.0.0.1:3306".to_string())];
        let card = render_card("ZEPHYR", "v1.0.0", "Concurrent Engine", &items);
        assert!(card.contains("╭"));
        assert!(card.contains("ZEPHYR"));
        assert!(card.contains("v1.0.0"));
        assert!(card.contains("Target Server"));
        assert!(card.contains("╰"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib ui::tests`
Expected: FAIL (cannot find module `ui`)

- [ ] **Step 3: Implement `src/ui.rs` and register in `src/main.rs`**

Create `src/ui.rs` with:
- `format_bytes(bytes: u64) -> String`: returns e.g. `"500 B"`, `"45.20 MB"`, `"1.81 GB (1,851.58 MB)"`.
- `format_bytes_compact(bytes: u64) -> String`: returns `"45.2 MB"` or `"1.81 GB"`.
- `render_card(...)`: generates the rounded 63-width card with cyan borders (`╭─╮`, `│`, `├─┤`, `╰─╯`).
- `render_summary_card(...)`: generates the result summary card in green (success), red (error), or yellow (cancelled).
- `render_section_divider(title: &str) -> String`: returns `─── Starting Parallel Import (6 Workers) ───────────────────────────`.

Add `pub mod ui;` in `src/main.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib ui::tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/ui.rs src/main.rs
git commit -m "feat(ui): add ui formatting and card drawing module"
```

---

### Task 2: Modern Startup Banner & Database Breakdown for Import (`src/main.rs`)

**Files:**
- Modify: `src/main.rs:213-330`

**Interfaces:**
- Consumes: `crate::ui::{format_bytes, render_card, render_section_divider}`

- [ ] **Step 1: Write integration smoke test for import banner rendering**

Add test in `tests/e2e_test.rs` or `src/main.rs` testing that `--dry-run` contains `"╭"` and `"ZEPHYR v1.0.0"`.

- [ ] **Step 2: Run test to verify it fails on old banner**

Run: `cargo test --test e2e_test test_dry_run_banner`
Expected: FAIL (banner contains old `==================================================`)

- [ ] **Step 3: Implement new banner and database list in `src/main.rs`**

In `src/main.rs`:
- Replace lines 213-256 with `ui::render_card("ZEPHYR", env!("CARGO_PKG_VERSION"), "High-Performance Concurrent MySQL Data Engine", &items)`.
- Replace database summary printout with:
  `"  ▸ {:<25} ······································ {:>4} files  {:>10.2} MB"`
- Replace pre-flight connection prints with:
  `println!("  {} Verifying database connection & schemas...", "▸".cyan());`
  `println!("  {} Database '{}' verified successfully.", "✔ [1/1]".green().bold(), db_name);`
- Replace line 321 `STARTING PARALLEL IMPORT` with:
  `println!("{}", ui::render_section_divider(&format!("Starting Parallel Import ({} Workers)", cli.resolved_workers())).green().bold());`

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test e2e_test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/main.rs tests/e2e_test.rs
git commit -m "feat(import): modernize startup banner, database list, and pre-flight ui"
```

---

### Task 3: Modern Startup Banner & Database Breakdown for Export (`src/exporter.rs`)

**Files:**
- Modify: `src/exporter.rs:180-275`

**Interfaces:**
- Consumes: `crate::ui::{format_bytes, render_card, render_section_divider}`

- [ ] **Step 1: Review export test coverage**

Inspect existing `tests/e2e_test.rs` export tests to ensure assertions are preserved.

- [ ] **Step 2: Implement modern banner and database list in `src/exporter.rs`**

In `src/exporter.rs`:
- Replace lines 180-230 with `ui::render_card("ZEPHYR", env!("CARGO_PKG_VERSION"), "High-Performance Concurrent MySQL Export Engine", &items)`.
- Format target databases with `▸` indicators and leader dots.
- Replace `STARTING PARALLEL EXPORT` with:
  `println!("{}", ui::render_section_divider(&format!("Starting Parallel Export ({} Workers)", args.resolved_workers())).green().bold());`

- [ ] **Step 3: Run export test to verify it passes**

Run: `cargo test --lib exporter::tests`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/exporter.rs
git commit -m "feat(export): modernize export startup banner and database list"
```

---

### Task 4: Smooth Block Progress Bar & Worker Live Tickers (`src/progress.rs`)

**Files:**
- Modify: `src/progress.rs:150-328`

**Interfaces:**
- Consumes: `indicatif::{MultiProgress, ProgressBar, ProgressStyle}`

- [ ] **Step 1: Update progress style unit tests in `src/progress.rs`**

Verify progress style setup compiles and sets correct characters.

- [ ] **Step 2: Update progress tracker implementation in `src/progress.rs`**

In `src/progress.rs`:
- Update `ProgressStyle::default_bar()`:
  - Template: `"{spinner:.cyan} [{elapsed_precise}] [{bar:32.cyan/blue}] {pos}/{len} ({percent}%) | {msg}"`
  - `.progress_chars("█▉▊▋▌▍▎▏ ")`
- Update the background ticker thread message formatting:
  `format!("{:.1}/{:.1} MB ({:.1} MB/s) | {} active{}{}", done_mb, total_mb, rate_mb, active_count, eta_str, failed_str)`
- Update worker sub-bar style:
  - Template: `"  {spinner:.cyan} [W{prefix}] {msg}"`
  - Worker ticker formatting:
    `format!("{}/{} : {:.1}/{:.1} MB ({:.0}%) {}", db.cyan(), file, cur_mb, tot_mb, pct, rate_str)`
- When finished:
  `pb.finish_with_message(format!("✔ Completed in {}", format_duration_compact(elapsed)));`

- [ ] **Step 3: Run unit tests in `src/progress.rs`**

Run: `cargo test --lib progress::tests`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/progress.rs
git commit -m "feat(progress): upgrade to smooth unicode block progress bar and styled worker tickers"
```

---

### Task 5: Modern Framed Completion Summary Cards (`src/progress.rs` & `src/exporter.rs`)

**Files:**
- Modify: `src/progress.rs:446-513`
- Modify: `src/exporter.rs:818-885`

**Interfaces:**
- Consumes: `crate::ui::render_summary_card`

- [ ] **Step 1: Write unit tests for summary card outputs**

Test `render_summary_card` handles success, error, and cancellation states with appropriate titles and colors.

- [ ] **Step 2: Implement new summary in `print_summary` (`src/progress.rs`)**

In `src/progress.rs`:
- Replace lines 447-490 with `ui::render_summary_card`:
  - Success title: `"✔ IMPORT COMPLETED SUCCESSFULLY"`
  - Error title: `"✖ IMPORT FINISHED WITH {} ERRORS"`
  - Cancelled title: `"⚠ IMPORT CANCELLED BY USER"`
- Format metric items:
  - `("Tables Imported", format!("{} / {}", summary.completed_count, summary.total_tasks))`
  - `("Volume Processed", format!("{:.2} MB ({:.1} MB/s)", completed_mb, mb_per_sec))`
  - `("Total Duration", format_duration_compact(summary.elapsed))`
  - `("State Manifest", format!("{} ({} recorded)", manifest_path.display(), summary.completed_count))`
- Format failed tables section with red `✖` bullet points and bold retry command.

- [ ] **Step 3: Implement matching export summary in `print_export_summary` (`src/exporter.rs`)**

In `src/exporter.rs`:
- Apply the same `ui::render_summary_card` for export completion.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --all-targets`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/progress.rs src/exporter.rs
git commit -m "feat(summary): modernize execution completion cards and failure reporting"
```

---

### Task 6: Full Verification & Visual Smoke Test

**Files:**
- Modify: `tests/e2e_test.rs` (if any assertions need alignment with new UI formatting)

- [ ] **Step 1: Run complete test suite**

Run: `cargo test --all-targets`
Expected: ALL PASS

- [ ] **Step 2: Smoke test dry-run in terminal**

Run: `cargo run -- --dry-run`
Verify the printed banner card, aligned bullets, and database summary.

- [ ] **Step 3: Commit any final test adjustments**

```bash
git commit -am "chore: verify modern ui and banner across full test suite"
```
