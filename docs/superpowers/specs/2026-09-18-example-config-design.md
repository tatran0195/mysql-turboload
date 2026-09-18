# Design Spec: Example Configuration File (`zephyr.example.toml`)

- **Author**: Antigravity Assistant & Engineering Team
- **Date**: 2026-09-18
- **Status**: Approved
- **Target File**: `zephyr.example.toml`

---

## 1. Background & Motivation

Zephyr provides TOML-based configuration file discovery via `src/config.rs` (checking `./zephyr.toml`, `./mysql-zephyr.toml`, and platform config folders). To facilitate seamless onboarding for operators and provide an authoritative reference for all configurable options, the repository requires an example configuration template: `zephyr.example.toml`.

Operators can copy `zephyr.example.toml` to `zephyr.toml` in their workspace or user config directory and customize settings without needing to inspect the Rust source code or CLI help flags.

---

## 2. Requirements & Design Decisions

1. **File Placement**:
   - Location: Repository root (`zephyr.example.toml`).
   - Purpose: Version-controlled template file. Operators copy it to `zephyr.toml` (which is auto-discovered when executing commands).

2. **Structure & Precedence Documentation**:
   - Header comments documenting the resolution hierarchy:
     1. Command-line flags (`-H`, `-P`, `-u`, etc.)
     2. Environment variables (`MYSQL_PWD`)
     3. TOML configuration file (`zephyr.toml`)
     4. Engine defaults
   - Auto-discovery search locations:
     - Windows: `%APPDATA%\zephyr\config.toml`
     - Linux / macOS: `~/.config/zephyr/config.toml`

3. **Approach**:
   - **Annotated Active Defaults with Commented Overrides**: Safe baseline defaults (`127.0.0.1`, port `3306`, user `root`, `utf8mb4`, `dir = "./dumps"`, etc.) remain active and valid.
   - All optional, advanced, or potentially sensitive settings (`password`, `tune_server`, `manifest`, `tables`, `filter`, etc.) are commented out with `#` and clearly annotated.
   - If renamed to `zephyr.toml` without editing, it must parse without any unknown field errors (`#[serde(deny_unknown_fields)]`).

4. **Schema Alignment**:
   - Must strictly match `ZephyrConfig`, `ConnectionConfig`, `ImportConfig`, and `ExportConfig` definitions in `src/config.rs`.

---

## 3. Configuration Specification

### 3.1 `[connection]` Table
| Field | Type | Default in Example | Description |
|---|---|---|---|
| `host` | String | `"127.0.0.1"` | Target MySQL server host |
| `port` | Integer | `3306` | Target MySQL server TCP port |
| `user` | String | `"root"` | MySQL user account |
| `password` | String | *(commented)* | MySQL password (prompt or `MYSQL_PWD` recommended) |
| `mysql_bin` | String/Path | *(commented)* | Explicit path to `mysql` binary |
| `mysqldump_bin` | String/Path | *(commented)* | Explicit path to `mysqldump` binary |
| `charset` | String | `"utf8mb4"` | MySQL client character set |
| `max_allowed_packet` | String | `"1G"` | Network packet buffer size |

### 3.2 `[import]` Table
| Field | Type | Default in Example | Description |
|---|---|---|---|
| `dir` | String/Path | `"./dumps"` | Source directory for dump files |
| `workers` | Integer | *(commented: `8`)* | Concurrency threads (default: min(cores, 8)) |
| `database` | String | *(commented)* | Target database override |
| `default_database` | String | *(commented)* | Fallback database if dump omits `USE` |
| `no_disable_foreign_keys` | Boolean | *(commented: `false`)* | Keep foreign key checks enabled |
| `log_dir` | String/Path | *(commented: `"./logs"`)* | Log directory (default: `./logs` or `<dir>/logs`) |
| `manifest` | String/Path | *(commented: `"./logs/manifest.json"`)* | Custom path to manifest (auto-discovered in `log_dir` if omitted) |
| `resume` | Boolean | *(commented: `true`)* | Resume ingestion skipping completed files |
| `retry_file` | String/Path | *(commented)* | Re-run only files listed in failure log |
| `filter` | String | *(commented: `"*.sql"`)* | File match filter pattern |
| `scan_depth` | Integer | *(commented: `5`)* | Directory recursion scan depth |
| `dry_run` | Boolean | *(commented: `false`)* | Dry run simulation |
| `no_progress` | Boolean | *(commented: `false`)* | Suppress interactive progress bar |
| `tune_server` | Boolean | *(commented: `false`)* | Optimize InnoDB settings on server |
| `buffer_pool_size` | String | *(commented: `"4G"`)* | Buffer pool size when tuning active |
| `io_capacity` | Integer | *(commented: `2500`)* | I/O capacity when tuning active |

### 3.3 `[export]` Table
| Field | Type | Default in Example | Description |
|---|---|---|---|
| `dir` | String/Path | `"./export-dumps"` | Target directory for exported dump files |
| `compress` | Boolean | `true` | Enable gzip compression (`.sql.gz`) on the fly |
| `workers` | Integer | *(commented: `8`)* | Concurrency threads (default: min(cores, 8)) |
| `database` | String | *(commented)* | Single database to export |
| `databases` | String | *(commented)* | Comma-separated list of databases |
| `all_databases` | Boolean | *(commented: `false`)* | Dump all databases on server |
| `tables` | String | *(commented)* | Comma-separated table inclusion list |
| `manifest` | String/Path | *(commented: `"./logs/manifest.json"`)* | Custom path to export manifest (auto-saved inside `log_dir` by default) |
| `resume` | Boolean | *(commented: `true`)* | Resume interrupted export |
| `dry_run` | Boolean | *(commented: `false`)* | Dry run simulation |
| `no_data` | Boolean | *(commented: `false`)* | DDL schema only |
| `no_create_info` | Boolean | *(commented: `false`)* | DML data rows only |
| `routines` | Boolean | *(commented: `true`)* | Export procedures and functions |
| `events` | Boolean | *(commented: `true`)* | Export scheduled events |
| `triggers` | Boolean | *(commented: `true`)* | Export table triggers |
| `net_buffer_length` | String | *(commented: `"1M"`)* | Network packet buffer size for mysqldump |
| `log_dir` | String/Path | *(commented: `"./logs"`)* | Directory for export logs (default: `<dir>/logs`) |
| `no_progress` | Boolean | *(commented: `false`)* | Suppress interactive progress bar |

---

## 4. Verification & Testing

1. **Schema Validation**:
   - Parse `zephyr.example.toml` using `ZephyrConfig::from_toml_str` in an automated Rust test or verification script.
   - Verify that all active fields deserialize correctly and all optional fields deserialize as expected.
2. **Commented Fields Coverage**:
   - Ensure an uncommented version of all commented fields parses cleanly without `unknown field` errors.
3. **Repository Build & Formatting**:
   - Check `cargo test` and `cargo fmt`.
