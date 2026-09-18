use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::cli::{ExportArgs, ImportArgs};

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct ZephyrConfig {
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

impl ZephyrConfig {
    pub fn from_toml_str(content: &str) -> Result<Self> {
        toml::from_str(content).context("Failed to parse TOML configuration")
    }
}

/// Locates a configuration file: either from the explicit path or by checking candidate paths.
pub fn find_config_file(explicit: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(p) = explicit {
        if p.is_file() {
            return Ok(Some(p.to_path_buf()));
        }
        anyhow::bail!("Configuration file not found: {}", p.display());
    }

    let candidates = [
        PathBuf::from("zephyr.toml"),
        PathBuf::from("mysql-zephyr.toml"),
    ];
    for c in &candidates {
        if c.is_file() {
            return Ok(Some(c.clone()));
        }
    }

    if let Some(appdata) = std::env::var_os("APPDATA") {
        let zephyr_p = PathBuf::from(&appdata).join("zephyr").join("config.toml");
        if zephyr_p.is_file() {
            return Ok(Some(zephyr_p));
        }
    } else if let Some(home) = std::env::var_os("HOME") {
        let zephyr_p = PathBuf::from(&home)
            .join(".config")
            .join("zephyr")
            .join("config.toml");
        if zephyr_p.is_file() {
            return Ok(Some(zephyr_p));
        }
    }

    Ok(None)
}

/// Loads and parses a TOML configuration file from disk.
pub fn load_config(path: &Path) -> Result<ZephyrConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read configuration file '{}'", path.display()))?;
    ZephyrConfig::from_toml_str(&content)
        .with_context(|| format!("In configuration file '{}'", path.display()))
}

fn is_cli_provided(matches: &clap::ArgMatches, id: &str) -> bool {
    matches.value_source(id) == Some(clap::parser::ValueSource::CommandLine)
}

fn is_cli_or_env_provided(matches: &clap::ArgMatches, id: &str) -> bool {
    matches.value_source(id) == Some(clap::parser::ValueSource::CommandLine)
        || matches.value_source(id) == Some(clap::parser::ValueSource::EnvVariable)
}

/// Merges configuration settings into ImportArgs.
/// Settings explicitly passed on the CLI or via env take precedence over TOML settings.
pub fn merge_import_config(args: &mut ImportArgs, cfg: &ZephyrConfig, matches: &clap::ArgMatches) {
    // Connection settings
    if !is_cli_provided(matches, "host") {
        if let Some(ref val) = cfg.connection.host {
            args.host = val.clone();
        }
    }
    if !is_cli_provided(matches, "port") {
        if let Some(val) = cfg.connection.port {
            args.port = val;
        }
    }
    if !is_cli_provided(matches, "user") {
        if let Some(ref val) = cfg.connection.user {
            args.user = val.clone();
        }
    }
    if !is_cli_or_env_provided(matches, "password") {
        if let Some(ref val) = cfg.connection.password {
            args.password = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "mysql_bin") {
        if let Some(ref val) = cfg.connection.mysql_bin {
            args.mysql_bin = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "max_allowed_packet") {
        if let Some(ref val) = cfg.connection.max_allowed_packet {
            args.max_allowed_packet = val.clone();
        }
    }
    if !is_cli_provided(matches, "charset") {
        if let Some(ref val) = cfg.connection.charset {
            args.charset = val.clone();
        }
    }

    // Import settings
    if !is_cli_provided(matches, "dir") {
        if let Some(ref val) = cfg.import.dir {
            args.dir = val.clone();
        }
    }
    if !is_cli_provided(matches, "workers") {
        if let Some(val) = cfg.import.workers {
            args.workers = Some(val);
        }
    }
    if !is_cli_provided(matches, "database") {
        if let Some(ref val) = cfg.import.database {
            args.database = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "default_database") {
        if let Some(ref val) = cfg.import.default_database {
            args.default_database = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "no_disable_foreign_keys") {
        if let Some(val) = cfg.import.no_disable_foreign_keys {
            args.no_disable_foreign_keys = val;
        }
    }
    if !is_cli_provided(matches, "dry_run") {
        if let Some(val) = cfg.import.dry_run {
            args.dry_run = val;
        }
    }
    if !is_cli_provided(matches, "log_dir") {
        if let Some(ref val) = cfg.import.log_dir {
            args.log_dir = val.clone();
        }
    }
    if !is_cli_provided(matches, "manifest") {
        if let Some(ref val) = cfg.import.manifest {
            args.manifest = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "resume") {
        if let Some(val) = cfg.import.resume {
            args.resume = val;
        }
    }
    if !is_cli_provided(matches, "retry_file") {
        if let Some(ref val) = cfg.import.retry_file {
            args.retry_file = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "filter") {
        if let Some(ref val) = cfg.import.filter {
            args.filter = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "scan_depth") {
        if let Some(val) = cfg.import.scan_depth {
            args.scan_depth = val;
        }
    }
    if !is_cli_provided(matches, "tune_server") {
        if let Some(val) = cfg.import.tune_server {
            args.tune_server = val;
        }
    }
    if !is_cli_provided(matches, "buffer_pool_size") {
        if let Some(ref val) = cfg.import.buffer_pool_size {
            args.buffer_pool_size = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "io_capacity") {
        if let Some(val) = cfg.import.io_capacity {
            args.io_capacity = Some(val);
        }
    }
    if !is_cli_provided(matches, "no_progress") {
        if let Some(val) = cfg.import.no_progress {
            args.no_progress = val;
        }
    }
}

/// Merges configuration settings into ExportArgs.
/// Settings explicitly passed on the CLI or via env take precedence over TOML settings.
pub fn merge_export_config(args: &mut ExportArgs, cfg: &ZephyrConfig, matches: &clap::ArgMatches) {
    // Connection settings
    if !is_cli_provided(matches, "host") {
        if let Some(ref val) = cfg.connection.host {
            args.host = val.clone();
        }
    }
    if !is_cli_provided(matches, "port") {
        if let Some(val) = cfg.connection.port {
            args.port = val;
        }
    }
    if !is_cli_provided(matches, "user") {
        if let Some(ref val) = cfg.connection.user {
            args.user = val.clone();
        }
    }
    if !is_cli_or_env_provided(matches, "password") {
        if let Some(ref val) = cfg.connection.password {
            args.password = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "mysqldump_bin") {
        if let Some(ref val) = cfg.connection.mysqldump_bin {
            args.mysqldump_bin = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "mysql_bin") {
        if let Some(ref val) = cfg.connection.mysql_bin {
            args.mysql_bin = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "max_allowed_packet") {
        if let Some(ref val) = cfg.connection.max_allowed_packet {
            args.max_allowed_packet = val.clone();
        }
    }

    // Export settings
    if !is_cli_provided(matches, "dir") {
        if let Some(ref val) = cfg.export.dir {
            args.dir = val.clone();
        }
    }
    if !is_cli_provided(matches, "compress") {
        if let Some(val) = cfg.export.compress {
            args.compress = val;
        }
    }
    if !is_cli_provided(matches, "workers") {
        if let Some(val) = cfg.export.workers {
            args.workers = Some(val);
        }
    }
    if !is_cli_provided(matches, "database") {
        if let Some(ref val) = cfg.export.database {
            args.database = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "databases") {
        if let Some(ref val) = cfg.export.databases {
            args.databases = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "all_databases") {
        if let Some(val) = cfg.export.all_databases {
            args.all_databases = val;
        }
    }
    if !is_cli_provided(matches, "tables") {
        if let Some(ref val) = cfg.export.tables {
            args.tables = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "exclude_tables") {
        if let Some(ref val) = cfg.export.exclude_tables {
            args.exclude_tables = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "resume") {
        if let Some(val) = cfg.export.resume {
            args.resume = val;
        }
    }
    if !is_cli_provided(matches, "manifest") {
        if let Some(ref val) = cfg.export.manifest {
            args.manifest = Some(val.clone());
        }
    }
    if !is_cli_provided(matches, "dry_run") {
        if let Some(val) = cfg.export.dry_run {
            args.dry_run = val;
        }
    }
    if !is_cli_provided(matches, "no_data") {
        if let Some(val) = cfg.export.no_data {
            args.no_data = val;
        }
    }
    if !is_cli_provided(matches, "no_create_info") {
        if let Some(val) = cfg.export.no_create_info {
            args.no_create_info = val;
        }
    }
    if !is_cli_provided(matches, "routines") {
        if let Some(val) = cfg.export.routines {
            args.routines = val;
        }
    }
    if !is_cli_provided(matches, "events") {
        if let Some(val) = cfg.export.events {
            args.events = val;
        }
    }
    if !is_cli_provided(matches, "triggers") {
        if let Some(val) = cfg.export.triggers {
            args.triggers = val;
        }
    }
    if !is_cli_provided(matches, "net_buffer_length") {
        if let Some(ref val) = cfg.export.net_buffer_length {
            args.net_buffer_length = val.clone();
        }
    }
    if !is_cli_provided(matches, "no_progress") {
        if let Some(val) = cfg.export.no_progress {
            args.no_progress = val;
        }
    }
    if !is_cli_provided(matches, "log_dir") {
        if let Some(ref val) = cfg.export.log_dir {
            args.log_dir = Some(val.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Commands};
    use clap::{CommandFactory, Parser};

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
        let cfg = ZephyrConfig::from_toml_str(toml_str).unwrap();
        assert_eq!(cfg.connection.host.as_deref(), Some("192.168.1.100"));
        assert_eq!(cfg.connection.port, Some(3307));
        assert_eq!(cfg.connection.user.as_deref(), Some("dbadmin"));
        assert_eq!(cfg.connection.password.as_deref(), Some("securepassword"));
        assert_eq!(cfg.connection.charset.as_deref(), Some("utf8mb4"));
        assert_eq!(cfg.connection.max_allowed_packet.as_deref(), Some("2G"));

        assert_eq!(cfg.import.dir, Some(PathBuf::from("./test-dumps")));
        assert_eq!(cfg.import.workers, Some(12));
        assert_eq!(cfg.import.database.as_deref(), Some("test_db"));
        assert_eq!(cfg.import.tune_server, Some(true));
        assert_eq!(cfg.import.buffer_pool_size.as_deref(), Some("8G"));
        assert_eq!(cfg.import.io_capacity, Some(3000));
        assert_eq!(cfg.import.resume, Some(true));

        assert_eq!(cfg.export.dir, Some(PathBuf::from("./export-target")));
        assert_eq!(cfg.export.compress, Some(true));
        assert_eq!(cfg.export.workers, Some(16));
        assert_eq!(cfg.export.database.as_deref(), Some("source_db"));
        assert_eq!(cfg.export.routines, Some(true));
        assert_eq!(cfg.export.events, Some(true));
        assert_eq!(cfg.export.triggers, Some(true));
    }

    #[test]
    fn test_parse_minimal_empty_toml() {
        let toml_str = "";
        let cfg = ZephyrConfig::from_toml_str(toml_str).unwrap();
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
        let res = ZephyrConfig::from_toml_str(toml_str);
        assert!(res.is_err(), "Expected error on unknown field 'worker'");
    }

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
        let cfg = ZephyrConfig::from_toml_str(toml_str).unwrap();

        // Simulate CLI where host was passed explicitly, but workers, port, and dir were not
        let cli = Cli::try_parse_from(["zephyr", "-H", "192.168.1.1"]).unwrap();
        let matches = Cli::command().get_matches_from(["zephyr", "-H", "192.168.1.1"]);
        let mut import_args = cli.import;
        merge_import_config(&mut import_args, &cfg, &matches);

        assert_eq!(import_args.host, "192.168.1.1"); // CLI overrides TOML
        assert_eq!(import_args.port, 3307); // TOML overrides default 3306
        assert_eq!(import_args.workers, Some(16)); // TOML overrides default None
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
        let cfg = ZephyrConfig::from_toml_str(toml_str).unwrap();

        let cli = Cli::try_parse_from(["zephyr", "export", "-d", "./cli-exports"]).unwrap();
        let matches = Cli::command().get_matches_from(["zephyr", "export", "-d", "./cli-exports"]);
        let sub_matches = matches.subcommand_matches("export").unwrap();
        if let Some(Commands::Export(mut export_args)) = cli.command {
            merge_export_config(&mut export_args, &cfg, sub_matches);
            assert_eq!(export_args.dir, PathBuf::from("./cli-exports")); // CLI overrides TOML
            assert_eq!(export_args.host, "10.0.0.2"); // TOML overrides default 127.0.0.1
            assert!(export_args.compress); // TOML overrides default false
        } else {
            panic!("Expected export command");
        }
    }

    #[test]
    fn test_find_config_file_explicit_missing() {
        let missing = PathBuf::from("non_existent_config_file_xyz.toml");
        let res = find_config_file(Some(&missing));
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_zephyr_example_toml_parses_successfully() {
        let example_path = Path::new("zephyr.example.toml");
        assert!(
            example_path.exists(),
            "zephyr.example.toml should exist in repository root"
        );
        let content =
            std::fs::read_to_string(example_path).expect("failed to read zephyr.example.toml");
        let cfg = ZephyrConfig::from_toml_str(&content)
            .expect("zephyr.example.toml should parse cleanly");

        // Verify active default settings
        assert_eq!(cfg.connection.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(cfg.connection.port, Some(3306));
        assert_eq!(cfg.connection.user.as_deref(), Some("root"));
        assert_eq!(cfg.connection.charset.as_deref(), Some("utf8mb4"));
        assert_eq!(cfg.connection.max_allowed_packet.as_deref(), Some("1G"));

        assert_eq!(cfg.import.dir, Some(PathBuf::from("./dumps")));

        assert_eq!(cfg.export.dir, Some(PathBuf::from("./export-dumps")));
        assert_eq!(cfg.export.compress, Some(true));

        // Also verify that uncommenting every commented option in the example file produces a valid config
        let uncommented: String = content
            .lines()
            .map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("# ") && trimmed.contains(" = ") {
                    &trimmed[2..]
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let full_cfg = ZephyrConfig::from_toml_str(&uncommented)
            .expect("uncommented zephyr.example.toml should parse without unknown field errors");
        assert_eq!(full_cfg.import.workers, Some(8));
        assert_eq!(full_cfg.export.workers, Some(8));
        assert_eq!(full_cfg.import.tune_server, Some(false));
        assert_eq!(full_cfg.export.routines, Some(true));
    }
}
