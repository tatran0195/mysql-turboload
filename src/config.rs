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

#[cfg(test)]
mod tests {
    use super::*;

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
