use anyhow::{bail, Result};
use std::env;
use std::path::{Path, PathBuf};

/// Finds the `mysql` or `mariadb` executable on the host system.
/// Priority:
/// 1. Explicit path given in CLI
/// 2. Current system PATH environment variable
/// 3. Well-known operating system installation directories
pub fn find_mysql(explicit_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit_path {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        bail!("Specified mysql executable not found: {}", path.display());
    }

    let exe_names = if cfg!(windows) {
        &["mysql.exe", "mariadb.exe"][..]
    } else {
        &["mysql", "mariadb"][..]
    };

    // 1. Check system PATH
    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            for &exe_name in exe_names {
                let candidate = dir.join(exe_name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }

    // 2. Platform-specific fallback paths
    if cfg!(windows) {
        let windows_candidates = [
            r"C:\Program Files\MySQL",
            r"C:\Program Files (x86)\MySQL",
            r"C:\Program Files\MariaDB",
        ];

        // Inspect MySQL/MariaDB server subdirectories
        for base in &windows_candidates {
            let base_path = Path::new(base);
            if let Ok(entries) = std::fs::read_dir(base_path) {
                for entry in entries.flatten() {
                    for &exe_name in exe_names {
                        let sub_bin = entry.path().join("bin").join(exe_name);
                        if sub_bin.is_file() {
                            return Ok(sub_bin);
                        }
                    }
                }
            }
        }

        // Specific common development stacks
        let standalone_paths = [
            r"C:\xampp\mysql\bin\mysql.exe",
            r"C:\tools\mysql\bin\mysql.exe",
        ];
        for path_str in &standalone_paths {
            let candidate = PathBuf::from(path_str);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }

        // Check Laragon pattern: C:\laragon\bin\mysql\mysql-*\bin\mysql.exe
        let laragon_base = Path::new(r"C:\laragon\bin\mysql");
        if let Ok(entries) = std::fs::read_dir(laragon_base) {
            for entry in entries.flatten() {
                for &exe_name in exe_names {
                    let candidate = entry.path().join("bin").join(exe_name);
                    if candidate.is_file() {
                        return Ok(candidate);
                    }
                }
            }
        }
    } else {
        // Unix / macOS common locations
        let unix_candidates = [
            "/usr/bin/mysql",
            "/usr/bin/mariadb",
            "/usr/local/bin/mysql",
            "/usr/local/bin/mariadb",
            "/opt/homebrew/bin/mysql",
            "/opt/homebrew/bin/mariadb",
            "/usr/local/mysql/bin/mysql",
            "/opt/local/bin/mysql",
        ];

        for path_str in &unix_candidates {
            let candidate = PathBuf::from(path_str);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    bail!(
        "Could not find 'mysql' or 'mariadb' in system PATH or standard installation paths.\n\
         Please provide its path using --mysql-bin <PATH> or ensure it is in your PATH."
    );
}

/// Finds the `mysqldump` or `mariadb-dump` executable on the host system.
pub fn find_mysqldump(explicit_path: Option<&Path>, mysql_bin: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit_path {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        bail!(
            "Specified mysqldump executable not found: {}",
            path.display()
        );
    }

    let dump_names = if cfg!(windows) {
        &["mysqldump.exe", "mariadb-dump.exe"][..]
    } else {
        &["mysqldump", "mariadb-dump"][..]
    };

    // 1. Look in the same directory as mysql binary if available
    if let Some(mysql) = mysql_bin {
        if let Some(parent) = mysql.parent() {
            for &dump_name in dump_names {
                let candidate = parent.join(dump_name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }

    // 2. Check system PATH
    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            for &dump_name in dump_names {
                let candidate = dir.join(dump_name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }

    // 3. Platform-specific fallback paths
    if cfg!(windows) {
        let windows_candidates = [
            r"C:\Program Files\MySQL",
            r"C:\Program Files (x86)\MySQL",
            r"C:\Program Files\MariaDB",
        ];

        for base in &windows_candidates {
            let base_path = Path::new(base);
            if let Ok(entries) = std::fs::read_dir(base_path) {
                for entry in entries.flatten() {
                    for &dump_name in dump_names {
                        let sub_bin = entry.path().join("bin").join(dump_name);
                        if sub_bin.is_file() {
                            return Ok(sub_bin);
                        }
                    }
                }
            }
        }
    } else {
        let unix_candidates = [
            "/usr/bin/mysqldump",
            "/usr/bin/mariadb-dump",
            "/usr/local/bin/mysqldump",
            "/usr/local/bin/mariadb-dump",
            "/opt/homebrew/bin/mysqldump",
            "/opt/homebrew/bin/mariadb-dump",
            "/usr/local/mysql/bin/mysqldump",
            "/opt/local/bin/mysqldump",
        ];

        for path_str in &unix_candidates {
            let candidate = PathBuf::from(path_str);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    bail!(
        "Could not find 'mysqldump' or 'mariadb-dump' in system PATH or standard installation paths.\n\
         Please provide its path using --mysqldump-bin <PATH> or ensure it is in your PATH."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_explicit_path_not_found() {
        let res = find_mysql(Some(Path::new("/non/existent/path/mysql")));
        assert!(res.is_err());
    }

    #[test]
    fn test_explicit_mysqldump_not_found() {
        let res = find_mysqldump(Some(Path::new("/non/existent/path/mysqldump")), None);
        assert!(res.is_err());
    }
}
