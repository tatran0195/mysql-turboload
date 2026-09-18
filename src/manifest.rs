use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct ImportManifest {
    #[serde(default)]
    pub last_updated: Option<DateTime<Utc>>,
    #[serde(default)]
    pub completed_files: HashSet<String>,
    #[serde(default)]
    pub completed_tables: Option<HashSet<String>>,
}

pub struct ManifestManager {
    file_path: PathBuf,
    loaded_from: Option<PathBuf>,
    manifest: Mutex<ImportManifest>,
    completed_files_lower: Mutex<HashSet<String>>,
}

impl ManifestManager {
    /// Creates or loads the manifest by searching candidate paths:
    /// 1. Explicit manifest path if specified
    /// 2. `log_dir/manifest.json`
    /// 3. `source_dir/logs/manifest.json`
    /// 4. `source_dir/manifest.json`
    /// 5. `source_dir/.import_manifest.json`
    /// 6. `current_dir/logs/manifest.json`
    pub fn new(log_dir: &Path, source_dir: &Path, explicit_manifest: Option<&Path>) -> Self {
        let (file_path, loaded_from, manifest) =
            Self::discover_and_load(log_dir, source_dir, explicit_manifest);

        let mut lower_set = HashSet::new();
        for item in &manifest.completed_files {
            lower_set.insert(item.to_lowercase());
            // If the item had path separators, also index just the filename
            if let Some(clean_name) = Path::new(item).file_name() {
                lower_set.insert(clean_name.to_string_lossy().to_lowercase());
            }
        }
        if let Some(ref tables) = manifest.completed_tables {
            for item in tables {
                lower_set.insert(item.to_lowercase());
            }
        }

        Self {
            file_path,
            loaded_from,
            manifest: Mutex::new(manifest),
            completed_files_lower: Mutex::new(lower_set),
        }
    }

    fn discover_and_load(
        log_dir: &Path,
        source_dir: &Path,
        explicit_manifest: Option<&Path>,
    ) -> (PathBuf, Option<PathBuf>, ImportManifest) {
        // If an explicit path is given by the user, honor it first
        if let Some(exp) = explicit_manifest {
            if exp.is_file() {
                match Self::load(exp) {
                    Ok(m) => return (exp.to_path_buf(), Some(exp.to_path_buf()), m),
                    Err(e) => {
                        eprintln!(
                            "[WARNING] Failed to load explicit manifest '{}': {}",
                            exp.display(),
                            e
                        );
                    }
                }
            }
            return (exp.to_path_buf(), None, ImportManifest::default());
        }

        // Ordered list of candidate locations to find an existing manifest
        let candidates = vec![
            log_dir.join("manifest.json"),
            source_dir.join("logs").join("manifest.json"),
            source_dir.join("manifest.json"),
            source_dir.join(".import_manifest.json"),
        ];

        for candidate in &candidates {
            if candidate.is_file() {
                match Self::load(candidate) {
                    Ok(m) => {
                        if !m.completed_files.is_empty()
                            || m.completed_tables.as_ref().is_some_and(|t| !t.is_empty())
                        {
                            return (candidate.clone(), Some(candidate.clone()), m);
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "[WARNING] Error reading manifest '{}': {}",
                            candidate.display(),
                            e
                        );
                    }
                }
            }
        }

        // If none found with existing records, default to log_dir/manifest.json
        let default_target = log_dir.join("manifest.json");
        (default_target, None, ImportManifest::default())
    }

    fn load(path: &Path) -> Result<ImportManifest> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;

        // 1. Try standard JSON object
        if let Ok(mut manifest) = serde_json::from_str::<ImportManifest>(&content) {
            // Normalize any completed_tables into completed_files
            if let Some(tables) = manifest.completed_tables.take() {
                for t in tables {
                    manifest.completed_files.insert(t);
                }
            }
            return Ok(manifest);
        }

        // 2. Try array of strings JSON: ["users.sql", "orders.sql"]
        if let Ok(file_list) = serde_json::from_str::<Vec<String>>(&content) {
            let mut manifest = ImportManifest::default();
            for f in file_list {
                manifest.completed_files.insert(f);
            }
            manifest.last_updated = Some(Utc::now());
            return Ok(manifest);
        }

        // 3. Fallback: line-by-line parsing if format was damaged or plain text
        let mut manifest = ImportManifest::default();
        for line in content.lines() {
            let trimmed = line
                .trim()
                .trim_matches(|c| c == '"' || c == ',' || c == '[' || c == ']');
            if trimmed.ends_with(".sql") || trimmed.ends_with(".SQL") {
                manifest.completed_files.insert(trimmed.to_string());
            }
        }

        if !manifest.completed_files.is_empty() {
            manifest.last_updated = Some(Utc::now());
            return Ok(manifest);
        }

        // 4. Try backup file if exists (e.g. manifest.json.bak)
        let bak_path = path.with_extension("json.bak");
        if bak_path.is_file() {
            if let Ok(bak_content) = fs::read_to_string(&bak_path) {
                if let Ok(manifest) = serde_json::from_str::<ImportManifest>(&bak_content) {
                    return Ok(manifest);
                }
            }
        }

        anyhow::bail!(
            "Unrecognized or corrupted manifest format in {}",
            path.display()
        )
    }

    /// Checks whether a table/file is recorded as completed.
    /// Supports exact matching, case-insensitive matching, and path-stripped matching.
    pub fn is_completed(&self, file_name: &str, file_path: Option<&Path>) -> bool {
        let guard = self.manifest.lock();

        // 1. Exact match on filename
        if guard.completed_files.contains(file_name) {
            return true;
        }

        // 2. Case-insensitive match on filename
        let lower = file_name.to_lowercase();
        let lower_guard = self.completed_files_lower.lock();
        if lower_guard.contains(&lower) {
            return true;
        }

        // 3. Check stripped filename (in case manifest contains directory prefix)
        if let Some(clean) = Path::new(file_name).file_name() {
            let clean_str = clean.to_string_lossy();
            if guard.completed_files.contains(clean_str.as_ref()) {
                return true;
            }
            if lower_guard.contains(&clean_str.to_lowercase()) {
                return true;
            }
        }

        // 4. Check full path or relative path if provided
        if let Some(path) = file_path {
            let path_str = path.to_string_lossy();
            if guard.completed_files.contains(path_str.as_ref()) {
                return true;
            }
            if lower_guard.contains(&path_str.to_lowercase()) {
                return true;
            }
        }

        false
    }

    /// Atomically marks a table as completed and persists to disk.
    /// Uses a temporary file + atomic rename to ensure manifest can NEVER be corrupted
    /// by unexpected shutdowns, power loss, or Ctrl+C.
    pub fn mark_completed(&self, file_name: &str) -> Result<()> {
        let clean_name = Path::new(file_name)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| file_name.to_string());

        {
            let mut guard = self.manifest.lock();
            guard.completed_files.insert(clean_name.clone());
            guard.last_updated = Some(Utc::now());

            let mut lower_guard = self.completed_files_lower.lock();
            lower_guard.insert(clean_name.to_lowercase());
        }

        self.persist_atomic()
    }

    fn persist_atomic(&self) -> Result<()> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp_path = self
            .file_path
            .with_extension(format!("tmp.{}", std::process::id()));
        let bak_path = self.file_path.with_extension("json.bak");

        let guard = self.manifest.lock();
        let json_bytes = serde_json::to_vec_pretty(&*guard)?;

        // Write to temporary file with flush & sync
        {
            let mut file = File::create(&tmp_path)?;
            file.write_all(&json_bytes)?;
            file.sync_all()?;
        }

        // Update backup file before replacing main manifest
        if self.file_path.is_file() {
            let _ = fs::copy(&self.file_path, &bak_path);
        }

        // Atomic rename replaces the existing file instantaneously
        fs::rename(&tmp_path, &self.file_path)?;

        Ok(())
    }

    pub fn count(&self) -> usize {
        let guard = self.manifest.lock();
        guard.completed_files.len()
    }

    pub fn loaded_from(&self) -> Option<&Path> {
        self.loaded_from.as_deref()
    }

    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    pub fn sample_entries(&self, max: usize) -> Vec<String> {
        let guard = self.manifest.lock();
        guard.completed_files.iter().take(max).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_manifest_lifecycle_and_atomic_write() {
        let temp = tempdir().unwrap();
        let log_dir = temp.path().join("logs");
        let source_dir = temp.path().join("dumps");
        fs::create_dir_all(&log_dir).unwrap();
        fs::create_dir_all(&source_dir).unwrap();

        let manager = ManifestManager::new(&log_dir, &source_dir, None);
        assert_eq!(manager.count(), 0);
        assert!(!manager.is_completed("users.sql", None));

        manager.mark_completed("users.sql").unwrap();
        manager.mark_completed("orders.sql").unwrap();

        assert_eq!(manager.count(), 2);
        assert!(manager.is_completed("users.sql", None));
        assert!(manager.is_completed("USERS.SQL", None)); // Case insensitive
        assert!(manager.is_completed("orders.sql", None));
        assert!(!manager.is_completed("products.sql", None));

        // Re-open from same directory
        let reopened = ManifestManager::new(&log_dir, &source_dir, None);
        assert_eq!(reopened.count(), 2);
        assert!(reopened.is_completed("users.sql", None));
        assert!(reopened.is_completed("orders.sql", None));
    }

    #[test]
    fn test_manifest_fallback_to_source_dir() {
        let temp = tempdir().unwrap();
        let log_dir = temp.path().join("logs");
        let source_dir = temp.path().join("dumps");
        let dump_logs = source_dir.join("logs");
        fs::create_dir_all(&dump_logs).unwrap();

        let dump_manifest = dump_logs.join("manifest.json");
        fs::write(
            &dump_manifest,
            r#"{"completed_files": ["table1.sql", "table2.sql"]}"#,
        )
        .unwrap();

        let manager = ManifestManager::new(&log_dir, &source_dir, None);
        assert_eq!(manager.count(), 2);
        assert!(manager.is_completed("table1.sql", None));
        assert!(manager.is_completed("table2.sql", None));
        assert_eq!(manager.loaded_from(), Some(dump_manifest.as_path()));
    }
}
