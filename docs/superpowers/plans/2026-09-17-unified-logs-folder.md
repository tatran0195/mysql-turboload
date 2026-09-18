# Unified `logs` Folder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Standardize the logs directory name from `import-logs` to `logs` across both export and import operations in `mysql-turboload`.

**Architecture:** Update CLI argument defaults in `cli.rs`, exporter path resolution in `exporter.rs`, candidate auto-detection in `manifest.rs`, and adjust test assertions and documentation accordingly.

**Tech Stack:** Rust (clap, serde_json, std::fs, std::path).

## Global Constraints

- Change folder name to `logs` strictly.
- Completely remove obsolete references to `import-logs`.
- Ensure all existing unit tests and integration tests compile and pass.

---

### Task 1: Update CLI Arguments and Helpers in `src/cli.rs`

**Files:**
- Modify: `src/cli.rs:108-125, 305-330`

**Interfaces:**
- `ImportArgs::log_dir`: Default value becomes `"logs"`.
- `ExportArgs::log_dir`: Optional `Option<PathBuf>`.
- `ExportArgs::resolved_log_dir(&self) -> PathBuf`: returns `log_dir` or `output_dir.join("logs")`.

- [ ] **Step 1: Edit `src/cli.rs` to update `ImportArgs` and `ExportArgs`**

In `ImportArgs`:
```rust
    /// Directory to write error logs and diagnostics
    #[arg(long = "log-dir", value_name = "DIR", default_value = "logs")]
    pub log_dir: PathBuf,
...
    /// Only import files listed in a previous failure file (e.g. logs/failed_files.txt)
    #[arg(long = "retry-file", value_name = "FILE")]
    pub retry_file: Option<PathBuf>,
```

In `ExportArgs`:
```rust
    /// Directory to write error logs and manifest [default: <output-dir>/logs]
    #[arg(long = "log-dir", value_name = "DIR")]
    pub log_dir: Option<PathBuf>,
```

In `impl ExportArgs`:
```rust
    /// Resolves log directory: user-specified, or output_dir/logs.
    pub fn resolved_log_dir(&self) -> PathBuf {
        self.log_dir
            .clone()
            .unwrap_or_else(|| self.output_dir.join("logs"))
    }
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check`
Expected: Passes without errors.

---

### Task 2: Update Manifest Manager Candidate Paths in `src/manifest.rs`

**Files:**
- Modify: `src/manifest.rs:25-36, 80-100, 320-340`

**Interfaces:**
- `ManifestManager::discover_and_load`: Searches `source_dir.join("logs").join("manifest.json")` instead of `import-logs`.

- [ ] **Step 1: Update candidate search paths and unit tests in `src/manifest.rs`**

Replace `source_dir.join("import-logs").join("manifest.json")` with:
```rust
        let candidates = vec![
            log_dir.join("manifest.json"),
            source_dir.join("logs").join("manifest.json"),
            source_dir.join("manifest.json"),
            source_dir.join(".import_manifest.json"),
        ];
```

In `test_manifest_fallback_to_source_dir`:
```rust
        let dump_logs = source_dir.join("logs");
```

- [ ] **Step 2: Run manifest unit tests**

Run: `cargo test manifest`
Expected: `test manifest::tests::test_manifest_lifecycle_and_atomic_write ... ok`, `test manifest::tests::test_manifest_fallback_to_source_dir ... ok`.

---

### Task 3: Update Exporter Engine in `src/exporter.rs`

**Files:**
- Modify: `src/exporter.rs:85-100, 465-510, 560-585, 645-660`

**Interfaces:**
- Uses `args.resolved_log_dir()` to create and populate log directory.

- [ ] **Step 1: Replace hardcoded `import-logs` with `resolved_log_dir()` in `src/exporter.rs`**

At line 88:
```rust
    let log_dir = args.resolved_log_dir();
```

Clone `log_dir` for use in worker threads:
```rust
    let log_dir_clone = log_dir.clone();
```

In worker failure handlers (replacing `args_clone.output_dir.join("import-logs")`):
```rust
    let log_path = log_dir_clone.join(format!(
        "{}__{}.export.err.log",
        task.database, task.table_name
    ));
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check`
Expected: Passes with no errors or warnings.

---

### Task 4: Update Documentation and Integration Tests

**Files:**
- Modify: `README.md:55-70, 180-230`
- Modify: `tests/e2e_test.rs:205-260`

- [ ] **Step 1: Update `README.md`**

Replace occurrences of `import-logs` with `logs`.

- [ ] **Step 2: Update integration tests in `tests/e2e_test.rs`**

Add assertions in `test_end_to_end_parallel_export_and_roundtrip`:
```rust
    let manifest_file = export_dir.join("logs").join("manifest.json");
    assert!(manifest_file.is_file(), "Export manifest should be generated in export_dir/logs");
```

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: All tests pass.
