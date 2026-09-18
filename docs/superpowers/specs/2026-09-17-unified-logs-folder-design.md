# Design Specification: Unified `logs` Folder Name

## 1. Context & Motivation

In `zephyr`, the export engine (`exporter.rs`) currently outputs manifests and error logs into a subdirectory named `import-logs` (e.g. `./dumps\import-logs\manifest.json`). This was a leftover naming convention from the original import tool. Additionally, the import CLI defaulted `--log-dir` to `import-logs`.

This specification formalizes the unification of the log directory name to **`logs`** across both export and import operations, cleanly replacing `import-logs`.

## 2. Changes & Specifications

### 2.1 CLI Configuration (`src/cli.rs`)

- **`ImportArgs`**:
    - Change default value of `--log-dir` from `"import-logs"` to `"logs"`.
    - Update doc comment and argument help text from `e.g. import-logs/failed_files.txt` to `e.g. logs/failed_files.txt`.
- **`ExportArgs`**:
    - Add optional `--log-dir` argument (`Option<PathBuf>`).
    - Add helper function `resolved_log_dir(&self) -> PathBuf` that defaults to `self.output_dir.join("logs")` if `--log-dir` is not explicitly supplied.

### 2.2 Export Engine (`src/exporter.rs`)

- Use `args.resolved_log_dir()` (defaults to `<output_dir>/logs`) instead of hardcoding `args.output_dir.join("import-logs")`.
- Ensure `fs::create_dir_all(&log_dir)` creates `<output_dir>/logs`.
- Pass `log_dir` to `ManifestManager::new(&log_dir, &args.output_dir, args.manifest.as_deref())`.
- Update all 4 worker error log paths (`.export.err.log`) to write directly into `log_dir` rather than `output_dir.join("import-logs")`.
- The banner and summary output will reflect `<output_dir>/logs/manifest.json`.

### 2.3 Manifest Manager (`src/manifest.rs`)

- In `ManifestManager::discover_and_load`:
    - Replace candidate path `source_dir.join("import-logs").join("manifest.json")` with `source_dir.join("logs").join("manifest.json")`.
    - The ordered candidate search list becomes:
        1. `explicit_manifest` (if provided)
        2. `log_dir.join("manifest.json")`
        3. `source_dir.join("logs").join("manifest.json")`
        4. `source_dir.join("manifest.json")`
        5. `source_dir(".import_manifest.json")`
    - Remove all remaining references to `import-logs`.
- Update doc comments and module unit tests (`test_manifest_fallback_to_source_dir`) to use `logs` instead of `import-logs`.

### 2.4 Documentation & Tests

- **`README.md`**:
    - Update all references to `import-logs` (e.g. `import-logs/*.err.log`, `--retry-file "import-logs/failed_files.txt"`, `--log-dir <DIR> [default: logs]`) to `logs/`.
- **`tests/e2e_test.rs`**:
    - In `test_end_to_end_parallel_export_and_roundtrip`, assert that `<export_dir>/logs/manifest.json` is generated upon export.
    - Verify that pointing `import` at `<export_dir>` automatically loads the manifest from `<export_dir>/logs/manifest.json`.

## 3. Verification Plan

1. **Compilation Check**: Run `cargo check` to ensure syntax, trait implementations, and argument structures are valid.
2. **Unit & Integration Tests**: Run `cargo test` to ensure all tests in `manifest.rs` and `e2e_test.rs` pass.
3. **Roundtrip Export & Import Verification**: Verify that running an export creates `logs/manifest.json` and running an import detects `logs/manifest.json`.
