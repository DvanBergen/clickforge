use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::models::{ScriptDocument, validate_script_for_persistence_and_execution};
use crate::storage::{
    StorageError, read_json_bounded, resolve_default_config_dir, write_json_atomically,
};

/// Default configuration folder name under XDG config home (`~/.config/clickforge`).
pub const DEFAULT_CONFIG_DIR_NAME: &str = "clickforge";

/// Subdirectory holding active script JSON documents.
pub const SCRIPTS_SUBDIR_NAME: &str = "scripts";

/// Subdirectory holding rotated backup copies of overwritten or deleted scripts.
pub const BACKUPS_SUBDIR_NAME: &str = "backups";

/// Maximum rotated backup files kept per script identifier.
pub const MAX_SCRIPT_BACKUPS: usize = 10;

/// Maximum script JSON file size accepted when loading from disk (1 MiB).
pub const MAX_SCRIPT_FILE_BYTES: u64 = 1_048_576;

const MAX_SCRIPT_ID_LENGTH: usize = 128;

static BACKUP_SEQUENCE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Atomic filesystem repository for ClickForge scripts with mandatory validation and backup rotation.
#[derive(Debug, Clone)]
pub struct ScriptRepository {
    base_directory: PathBuf,
}

impl ScriptRepository {
    /// Opens the repository at the default user configuration directory (`~/.config/clickforge`).
    pub fn new_default() -> Result<Self, StorageError> {
        let default_directory = resolve_default_config_dir()?;
        Self::new_with_base_dir(default_directory)
    }

    /// Opens or initializes a script repository rooted at `base_directory`.
    pub fn new_with_base_dir(base_directory: PathBuf) -> Result<Self, StorageError> {
        let repository = Self { base_directory };
        repository.ensure_directories_exist()?;
        Ok(repository)
    }

    /// Returns the root configuration directory path.
    pub fn base_directory(&self) -> &Path {
        &self.base_directory
    }

    /// Returns the directory containing persisted script JSON files.
    pub fn scripts_directory(&self) -> PathBuf {
        self.base_directory.join(SCRIPTS_SUBDIR_NAME)
    }

    /// Returns the directory containing rotated script backup files.
    pub fn backups_directory(&self) -> PathBuf {
        self.base_directory.join(BACKUPS_SUBDIR_NAME)
    }

    /// Validates, backs up any prior copy, and atomically writes a script document to disk.
    pub fn save_script(&self, script: &ScriptDocument) -> Result<PathBuf, StorageError> {
        validate_script_for_persistence_and_execution(script)?;
        validate_script_id(&script.id)?;
        self.ensure_directories_exist()?;

        let target_path = self.script_file_path(&script.id);
        if target_path.exists() {
            self.backup_existing_script(&script.id, &target_path)?;
        }

        write_json_atomically(&target_path, script)?;
        Ok(target_path)
    }

    /// Loads a single script document by identifier.
    pub fn load_script(&self, script_id: &str) -> Result<ScriptDocument, StorageError> {
        validate_script_id(script_id)?;
        let script_path = self.script_file_path(script_id);
        read_json_bounded(&script_path, MAX_SCRIPT_FILE_BYTES)
    }

    /// Loads all persisted script documents sorted deterministically by name and identifier.
    pub fn load_all_scripts(&self) -> Result<Vec<ScriptDocument>, StorageError> {
        let scripts_directory = self.scripts_directory();
        if !scripts_directory.exists() {
            return Ok(Vec::new());
        }

        let directory_entries =
            fs::read_dir(&scripts_directory).map_err(|source| StorageError::IoFailed {
                path: scripts_directory.clone(),
                source,
            })?;

        let mut loaded_scripts = Vec::new();
        for entry_result in directory_entries {
            let directory_entry = entry_result.map_err(|source| StorageError::IoFailed {
                path: scripts_directory.clone(),
                source,
            })?;
            let entry_path = directory_entry.path();
            if !is_script_json_file(&entry_path) {
                continue;
            }
            let script_document: ScriptDocument =
                read_json_bounded(&entry_path, MAX_SCRIPT_FILE_BYTES)?;
            loaded_scripts.push(script_document);
        }

        loaded_scripts.sort_by(|left_script, right_script| {
            left_script
                .name
                .to_ascii_lowercase()
                .cmp(&right_script.name.to_ascii_lowercase())
                .then_with(|| left_script.id.cmp(&right_script.id))
        });
        Ok(loaded_scripts)
    }

    /// Loads existing scripts, or persists and returns the Building Upgrade template when empty.
    pub fn load_or_seed_default_scripts(&self) -> Result<Vec<ScriptDocument>, StorageError> {
        let existing_scripts = self.load_all_scripts()?;
        if !existing_scripts.is_empty() {
            return Ok(existing_scripts);
        }
        let default_template = ScriptDocument::new_building_upgrade_template();
        self.save_script(&default_template)?;
        Ok(vec![default_template])
    }

    /// Backs up and deletes a script file from disk, returning true if the file existed.
    pub fn delete_script(&self, script_id: &str) -> Result<bool, StorageError> {
        validate_script_id(script_id)?;
        let script_path = self.script_file_path(script_id);
        if !script_path.exists() {
            return Ok(false);
        }
        self.backup_existing_script(script_id, &script_path)?;
        fs::remove_file(&script_path).map_err(|source| StorageError::IoFailed {
            path: script_path,
            source,
        })?;
        Ok(true)
    }

    /// Lists all retained backup files for `script_id`, sorted oldest to newest.
    pub fn list_backups_for_script(&self, script_id: &str) -> Result<Vec<PathBuf>, StorageError> {
        validate_script_id(script_id)?;
        let backups_directory = self.backups_directory();
        if !backups_directory.exists() {
            return Ok(Vec::new());
        }

        let backup_prefix = format!("{script_id}.");
        let directory_entries =
            fs::read_dir(&backups_directory).map_err(|source| StorageError::IoFailed {
                path: backups_directory.clone(),
                source,
            })?;

        let mut matching_paths = Vec::new();
        for entry_result in directory_entries {
            let directory_entry = entry_result.map_err(|source| StorageError::IoFailed {
                path: backups_directory.clone(),
                source,
            })?;
            let entry_path = directory_entry.path();
            let Some(file_name) = entry_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if file_name.starts_with(&backup_prefix) && file_name.ends_with(".json") {
                matching_paths.push(entry_path);
            }
        }

        matching_paths.sort();
        Ok(matching_paths)
    }

    fn ensure_directories_exist(&self) -> Result<(), StorageError> {
        let scripts_directory = self.scripts_directory();
        fs::create_dir_all(&scripts_directory).map_err(|source| StorageError::IoFailed {
            path: scripts_directory,
            source,
        })?;
        let backups_directory = self.backups_directory();
        fs::create_dir_all(&backups_directory).map_err(|source| StorageError::IoFailed {
            path: backups_directory,
            source,
        })?;
        Ok(())
    }

    fn script_file_path(&self, script_id: &str) -> PathBuf {
        self.scripts_directory().join(format!("{script_id}.json"))
    }

    fn backup_existing_script(
        &self,
        script_id: &str,
        existing_path: &Path,
    ) -> Result<(), StorageError> {
        let timestamp_ms = ScriptDocument::current_unix_timestamp_ms();
        let sequence_number = BACKUP_SEQUENCE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let backup_file_name = format!("{script_id}.{timestamp_ms:016}.{sequence_number:06}.json");
        let backup_path = self.backups_directory().join(backup_file_name);

        fs::copy(existing_path, &backup_path).map_err(|source| StorageError::IoFailed {
            path: backup_path,
            source,
        })?;
        self.evict_excess_backups(script_id)?;
        Ok(())
    }

    fn evict_excess_backups(&self, script_id: &str) -> Result<(), StorageError> {
        let backups = self.list_backups_for_script(script_id)?;
        if backups.len() <= MAX_SCRIPT_BACKUPS {
            return Ok(());
        }
        let excess_count = backups.len().saturating_sub(MAX_SCRIPT_BACKUPS);
        for obsolete_backup in backups.iter().take(excess_count) {
            fs::remove_file(obsolete_backup).map_err(|source| StorageError::IoFailed {
                path: obsolete_backup.clone(),
                source,
            })?;
        }
        Ok(())
    }
}

fn validate_script_id(script_id: &str) -> Result<(), StorageError> {
    if script_id.is_empty() || script_id.len() > MAX_SCRIPT_ID_LENGTH {
        return Err(StorageError::InvalidScriptId {
            script_id: script_id.to_string(),
        });
    }
    let all_safe_chars = script_id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_');
    if !all_safe_chars {
        return Err(StorageError::InvalidScriptId {
            script_id: script_id.to_string(),
        });
    }
    Ok(())
}

fn is_script_json_file(candidate_path: &Path) -> bool {
    if !candidate_path.is_file() {
        return false;
    }
    let Some(file_name) = candidate_path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    !file_name.starts_with('.') && file_name.ends_with(".json")
}

#[cfg(test)]
mod tests {
    use clickforge_sdk::KeyCode;

    use super::*;
    use crate::models::{ScriptMode, TargetScope};

    fn create_ephemeral_test_dir(test_label: &str) -> Result<PathBuf, std::io::Error> {
        let timestamp_ms = ScriptDocument::current_unix_timestamp_ms();
        let random_suffix: u32 = rand::random();
        let directory_path = std::env::temp_dir().join(format!(
            "clickforge-repo-test-{test_label}-{timestamp_ms}-{random_suffix:08x}"
        ));
        fs::create_dir_all(&directory_path)?;
        Ok(directory_path)
    }

    #[test]
    fn repository_refuses_to_save_script_without_kill_switch()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = create_ephemeral_test_dir("reject-no-kill-switch")?;
        let repository = ScriptRepository::new_with_base_dir(temp_dir.clone())?;

        let invalid_script = ScriptDocument::new_blank(
            "Unsafe Script",
            TargetScope::Global,
            ScriptMode::VisualBlocks,
        );
        let save_result = repository.save_script(&invalid_script);
        assert!(matches!(
            save_result,
            Err(StorageError::ValidationRejected(_))
        ));
        assert!(repository.load_all_scripts()?.is_empty());

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn repository_saves_atomically_rotates_backups_and_rejects_traversal()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = create_ephemeral_test_dir("atomic-and-backups")?;
        let repository = ScriptRepository::new_with_base_dir(temp_dir.clone())?;

        let mut script = ScriptDocument::new_building_upgrade_template();
        repository.save_script(&script)?;
        let loaded = repository.load_script(&script.id)?;
        assert_eq!(loaded.name, script.name);
        assert_eq!(loaded.kill_switch_key, Some(KeyCode::F8));

        for revision_index in 0..15 {
            script.description = format!("Revision {revision_index}");
            repository.save_script(&script)?;
        }

        let retained_backups = repository.list_backups_for_script(&script.id)?;
        assert_eq!(retained_backups.len(), MAX_SCRIPT_BACKUPS);

        // Verify zero leftover temp files in scripts directory.
        for entry_result in fs::read_dir(repository.scripts_directory())? {
            let file_name = entry_result?.file_name();
            let file_name_text = file_name.to_string_lossy();
            assert!(!file_name_text.ends_with(".tmp"));
        }

        // Reject path traversal identifiers.
        let mut traversal_script = ScriptDocument::new_building_upgrade_template();
        traversal_script.id = "../outside".to_string();
        assert!(matches!(
            repository.save_script(&traversal_script),
            Err(StorageError::InvalidScriptId { .. })
        ));

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn load_or_seed_creates_default_template_when_empty() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = create_ephemeral_test_dir("seed-default")?;
        let repository = ScriptRepository::new_with_base_dir(temp_dir.clone())?;

        let seeded = repository.load_or_seed_default_scripts()?;
        assert_eq!(seeded.len(), 1);
        assert_eq!(seeded[0].name, "Building Upgrade Rapid Clicker");

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }
}
