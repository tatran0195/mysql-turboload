use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "mysql-turboload",
    author = "Enterprise Database Systems",
    version = "1.0.0",
    about = "MySQL TurboLoad Enterprise: High-performance concurrent bulk data ingestion & export engine",
    long_about = "MySQL TurboLoad Enterprise\n\
                  A resilient, enterprise-grade CLI application designed to restore and export MySQL database\n\
                  tables concurrently. Features dynamic MPMC work-stealing, live server-side InnoDB performance tuning,\n\
                  atomic stateful resume manifests, and high-throughput zero-copy streaming."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub import: ImportArgs,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Ingest and restore MySQL dump files in parallel (default command)
    Import(ImportArgs),
    /// Export MySQL database tables in parallel at maximum throughput
    Export(ExportArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ImportArgs {
    /// Directory containing .sql dump files (or positional argument)
    #[arg(short = 'd', long = "dir", value_name = "DIR", default_value = ".")]
    pub dir: PathBuf,

    /// MySQL server host
    #[arg(
        short = 'H',
        long = "host",
        value_name = "HOST",
        default_value = "127.0.0.1"
    )]
    pub host: String,

    /// MySQL server port
    #[arg(
        short = 'P',
        long = "port",
        value_name = "PORT",
        default_value = "3306"
    )]
    pub port: u16,

    /// MySQL username
    #[arg(
        short = 'u',
        long = "user",
        value_name = "USER",
        default_value = "root"
    )]
    pub user: String,

    /// MySQL password (read from env MYSQL_PWD if not provided, or prompted securely)
    #[arg(
        short = 'p',
        long = "password",
        value_name = "PASSWORD",
        env = "MYSQL_PWD"
    )]
    pub password: Option<String>,

    /// Always prompt interactively for MySQL password (masks typed characters)
    #[arg(short = 'W', long = "ask-password")]
    pub ask_password: bool,

    /// Number of concurrent worker threads [default: min(logical CPUs, 8)]
    #[arg(short = 'w', long = "workers", value_name = "COUNT")]
    pub workers: Option<usize>,

    /// Explicit path to mysql / mysql.exe executable (auto-detected if omitted)
    #[arg(long = "mysql-bin", value_name = "PATH")]
    pub mysql_bin: Option<PathBuf>,

    /// Force all SQL files to import into this database (overrides header detection)
    #[arg(long = "database", value_name = "NAME")]
    pub database: Option<String>,

    /// Fallback database name if a file's header does not specify one
    #[arg(long = "default-database", value_name = "NAME")]
    pub default_database: Option<String>,

    /// MySQL max_allowed_packet size
    #[arg(long = "max-allowed-packet", value_name = "SIZE", default_value = "1G")]
    pub max_allowed_packet: String,

    /// Default character set for client connection
    #[arg(long = "charset", value_name = "CHARSET", default_value = "utf8mb4")]
    pub charset: String,

    /// Do NOT disable foreign key checks and unique checks during import
    #[arg(long = "no-disable-foreign-keys")]
    pub no_disable_foreign_keys: bool,

    /// Scan and display plan without executing imports
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Directory to write error logs and diagnostics
    #[arg(long = "log-dir", value_name = "DIR", default_value = "logs")]
    pub log_dir: PathBuf,

    /// Explicit path to manifest.json file (auto-detected in --log-dir, source --dir, and CWD if omitted)
    #[arg(long = "manifest", value_name = "PATH")]
    pub manifest: Option<PathBuf>,

    /// Resume an interrupted import, skipping already completed files
    #[arg(long = "resume")]
    pub resume: bool,

    /// Only import files listed in a previous failure file (e.g. logs/failed_files.txt)
    #[arg(long = "retry-file", value_name = "FILE")]
    pub retry_file: Option<PathBuf>,

    /// Filter SQL files matching this substring or pattern (case-insensitive)
    #[arg(long = "filter", value_name = "PATTERN")]
    pub filter: Option<String>,

    /// Maximum lines to scan in each file header for database detection
    #[arg(long = "scan-depth", value_name = "LINES", default_value = "100")]
    pub scan_depth: usize,

    /// Automatically optimize MySQL server variables during import (innodb_flush_log_at_trx_commit=2, sync_binlog=0)
    /// and restore them upon completion for 5x-10x speedup (requires SUPER/SYSTEM_VARIABLES_ADMIN privilege)
    #[arg(long = "tune-server")]
    pub tune_server: bool,

    /// Target InnoDB buffer pool size during import (e.g. 4G, 8G, 2048M) [default with --tune-server: 4G]
    #[arg(long = "buffer-pool-size", value_name = "SIZE")]
    pub buffer_pool_size: Option<String>,

    /// Target InnoDB I/O capacity (IOPS) during import [default with --tune-server: 2500]
    #[arg(long = "io-capacity", value_name = "IOPS")]
    pub io_capacity: Option<u32>,

    /// Disable dynamic progress bar animations (plain text output)
    #[arg(long = "no-progress")]
    pub no_progress: bool,
}

impl ImportArgs {
    /// Determines effective worker count: user-specified, or min(num_cpus, 8), minimum 1.
    pub fn resolved_workers(&self) -> usize {
        if let Some(w) = self.workers {
            return w.max(1);
        }
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        cpus.clamp(1, 8)
    }

    /// Resolves password: asks user interactively if requested or not supplied.
    pub fn resolve_password(&mut self) -> Result<(), anyhow::Error> {
        if self.ask_password || self.password.is_none() {
            eprint!(
                "Enter MySQL password for user '{}' (leave blank for none): ",
                self.user
            );
            let input = rpassword::read_password()?;
            self.password = if input.is_empty() { None } else { Some(input) };
        }
        Ok(())
    }
}

#[derive(Args, Debug, Clone)]
pub struct ExportArgs {
    /// Output directory to store exported .sql files
    #[arg(
        short = 'o',
        short_alias = 'd',
        long = "output-dir",
        alias = "dir",
        value_name = "DIR",
        default_value = "export-dumps"
    )]
    pub output_dir: PathBuf,

    /// Compress exported dump files with gzip (.sql.gz)
    #[arg(short = 'z', long = "compress")]
    pub compress: bool,

    /// MySQL server host
    #[arg(
        short = 'H',
        long = "host",
        value_name = "HOST",
        default_value = "127.0.0.1"
    )]
    pub host: String,

    /// MySQL server port
    #[arg(
        short = 'P',
        long = "port",
        value_name = "PORT",
        default_value = "3306"
    )]
    pub port: u16,

    /// MySQL username
    #[arg(
        short = 'u',
        long = "user",
        value_name = "USER",
        default_value = "root"
    )]
    pub user: String,

    /// MySQL password (read from env MYSQL_PWD if not provided, or prompted securely)
    #[arg(
        short = 'p',
        long = "password",
        value_name = "PASSWORD",
        env = "MYSQL_PWD"
    )]
    pub password: Option<String>,

    /// Always prompt interactively for MySQL password (masks typed characters)
    #[arg(short = 'W', long = "ask-password")]
    pub ask_password: bool,

    /// Number of concurrent export worker threads [default: min(logical CPUs, 8)]
    #[arg(short = 'w', long = "workers", value_name = "COUNT")]
    pub workers: Option<usize>,

    /// Target single database to export
    #[arg(short = 'B', long = "database", value_name = "NAME")]
    pub database: Option<String>,

    /// Comma-separated list of databases to export (e.g. "db1,db2")
    #[arg(long = "databases", value_name = "NAMES")]
    pub databases: Option<String>,

    /// Export all non-system databases
    #[arg(long = "all-databases")]
    pub all_databases: bool,

    /// Comma-separated list of specific tables to export (e.g. "users,orders")
    #[arg(long = "tables", value_name = "TABLES")]
    pub tables: Option<String>,

    /// Comma-separated list of tables to exclude from export
    #[arg(long = "exclude-tables", value_name = "TABLES")]
    pub exclude_tables: Option<String>,

    /// Explicit path to mysqldump / mysqldump.exe executable (auto-detected if omitted)
    #[arg(long = "mysqldump-bin", value_name = "PATH")]
    pub mysqldump_bin: Option<PathBuf>,

    /// Explicit path to mysql / mysql.exe executable (auto-detected if omitted)
    #[arg(long = "mysql-bin", value_name = "PATH")]
    pub mysql_bin: Option<PathBuf>,

    /// Resume an interrupted export job, skipping already exported tables
    #[arg(long = "resume")]
    pub resume: bool,

    /// Explicit path to manifest.json file
    #[arg(long = "manifest", value_name = "PATH")]
    pub manifest: Option<PathBuf>,

    /// Scan table schemas and estimate sizes without executing dumps
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Dump table structure (schema) only, without data
    #[arg(long = "no-data")]
    pub no_data: bool,

    /// Dump table data only, without CREATE TABLE statements
    #[arg(long = "no-create-info")]
    pub no_create_info: bool,

    /// Include stored procedures and functions in export
    #[arg(long = "routines", default_value_t = true)]
    pub routines: bool,

    /// Include events in export
    #[arg(long = "events", default_value_t = true)]
    pub events: bool,

    /// Include triggers in export
    #[arg(long = "triggers", default_value_t = true)]
    pub triggers: bool,

    /// Maximum packet size for network buffer
    #[arg(long = "max-allowed-packet", value_name = "SIZE", default_value = "1G")]
    pub max_allowed_packet: String,

    /// Network buffer length for streaming
    #[arg(long = "net-buffer-length", value_name = "SIZE", default_value = "1M")]
    pub net_buffer_length: String,

    /// Disable dynamic progress bar animations (plain text output)
    #[arg(long = "no-progress")]
    pub no_progress: bool,

    /// Directory to write error logs and manifest [default: <output-dir>/logs]
    #[arg(long = "log-dir", value_name = "DIR")]
    pub log_dir: Option<PathBuf>,
}

impl ExportArgs {
    /// Resolves log directory: user-specified, or output_dir/logs.
    pub fn resolved_log_dir(&self) -> PathBuf {
        self.log_dir
            .clone()
            .unwrap_or_else(|| self.output_dir.join("logs"))
    }
    /// Determines effective worker count: user-specified, or min(num_cpus, 8), minimum 1.
    pub fn resolved_workers(&self) -> usize {
        if let Some(w) = self.workers {
            return w.max(1);
        }
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        cpus.clamp(1, 8)
    }

    /// Resolves password: asks user interactively if requested or not supplied.
    pub fn resolve_password(&mut self) -> Result<(), anyhow::Error> {
        if self.ask_password || self.password.is_none() {
            eprint!(
                "Enter MySQL password for user '{}' (leave blank for none): ",
                self.user
            );
            let input = rpassword::read_password()?;
            self.password = if input.is_empty() { None } else { Some(input) };
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_args_default_log_dir() {
        let cli = Cli::try_parse_from(["mysql-turboload"]).unwrap();
        assert_eq!(cli.import.log_dir, PathBuf::from("logs"));
    }

    #[test]
    fn test_export_args_default_and_custom_log_dir() {
        let cli = Cli::try_parse_from(["mysql-turboload", "export", "-o", "./my-dumps"]).unwrap();
        if let Some(Commands::Export(export_args)) = cli.command {
            assert_eq!(
                export_args.resolved_log_dir(),
                PathBuf::from("./my-dumps").join("logs")
            );
        } else {
            panic!("Expected export command");
        }

        let cli2 = Cli::try_parse_from([
            "mysql-turboload",
            "export",
            "-o",
            "./my-dumps",
            "--log-dir",
            "./custom-logs",
        ])
        .unwrap();
        if let Some(Commands::Export(export_args)) = cli2.command {
            assert_eq!(
                export_args.resolved_log_dir(),
                PathBuf::from("./custom-logs")
            );
        } else {
            panic!("Expected export command");
        }
    }

    #[test]
    fn test_export_args_dir_aliases() {
        let cli_short =
            Cli::try_parse_from(["mysql-turboload", "export", "-d", "./my-target"]).unwrap();
        if let Some(Commands::Export(export_args)) = cli_short.command {
            assert_eq!(export_args.output_dir, PathBuf::from("./my-target"));
        } else {
            panic!("Expected export command");
        }

        let cli_long =
            Cli::try_parse_from(["mysql-turboload", "export", "--dir", "./my-target"]).unwrap();
        if let Some(Commands::Export(export_args)) = cli_long.command {
            assert_eq!(export_args.output_dir, PathBuf::from("./my-target"));
        } else {
            panic!("Expected export command");
        }
    }

    #[test]
    fn test_export_args_compress_flags() {
        let cli_flag = Cli::try_parse_from(["mysql-turboload", "export", "--compress"]).unwrap();
        if let Some(Commands::Export(export_args)) = cli_flag.command {
            assert!(export_args.compress);
        } else {
            panic!("Expected export command");
        }

        let cli_short = Cli::try_parse_from(["mysql-turboload", "export", "-z"]).unwrap();
        if let Some(Commands::Export(export_args)) = cli_short.command {
            assert!(export_args.compress);
        } else {
            panic!("Expected export command");
        }
    }

    #[test]
    fn test_root_dir_inheritance_logic() {
        let cli = Cli::try_parse_from(["mysql-turboload", "-d", "./root-dumps", "export"]).unwrap();
        assert_eq!(cli.import.dir, PathBuf::from("./root-dumps"));
        if let Some(Commands::Export(mut export_args)) = cli.command {
            if export_args.output_dir == std::path::Path::new("export-dumps")
                && cli.import.dir != std::path::Path::new(".")
            {
                export_args.output_dir = cli.import.dir;
            }
            assert_eq!(export_args.output_dir, PathBuf::from("./root-dumps"));
        } else {
            panic!("Expected export command");
        }
    }
}
