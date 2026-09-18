use anyhow::Result;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::Builder;

pub struct MySqlOptionFile {
    path: PathBuf,
}

impl MySqlOptionFile {
    /// Creates a secure temporary MySQL option file with RAII cleanup on drop.
    pub fn create(host: &str, port: u16, user: &str, password: Option<&str>) -> Result<Self> {
        let temp_file = Builder::new()
            .prefix("mysql-import-")
            .suffix(".cnf")
            .tempfile()?;

        let path = temp_file.path().to_path_buf();
        let mut file = File::create(&path)?;

        writeln!(file, "[client]")?;
        writeln!(file, "user={}", user)?;
        writeln!(file, "host={}", host)?;
        writeln!(file, "port={}", port)?;
        writeln!(file, "protocol=tcp")?;

        if let Some(pwd) = password {
            // Escape double quotes and backslashes for MySQL option file format
            let escaped = pwd.replace('\\', "\\\\").replace('"', "\\\"");
            writeln!(file, "password=\"{}\"", escaped)?;
        }

        file.flush()?;

        // Keep file on disk while struct is alive; deleted on drop
        temp_file.keep()?;

        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for MySqlOptionFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
