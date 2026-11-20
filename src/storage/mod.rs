pub mod script_repository;
pub mod settings_store;

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::models::ValidationError;

pub use script_repository::{
    BACKUPS_SUBDIR_NAME, DEFAULT_CONFIG_DIR_NAME, MAX_SCRIPT_BACKUPS, MAX_SCRIPT_FILE_BYTES,
    SCRIPTS_SUBDIR_NAME, ScriptRepository,
};
pub use settings_store::{AppSettings, SETTINGS_FILE_NAME, SettingsStore};

static ATOMIC_TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Errors emitted when validating, reading, or atomically persisting ClickForge files.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Unable to resolve XDG user configuration directory")]
    ConfigDirectoryUnavailable,

    #[error(transparent)]
    ValidationRejected(#[from] ValidationError),

    #[error(
        "Invalid script identifier '{script_id}': must be 1..=128 ASCII alphanumeric, '-' or '_' characters"
    )]
    InvalidScriptId { script_id: String },

    #[error("File '{path}' is {byte_count} bytes, exceeding limit of {max_bytes} bytes")]
    FileTooLarge {
        path: PathBuf,
        byte_count: u64,
        max_bytes: u64,
    },

    #[error("Filesystem I/O error at '{path}': {source}")]
    IoFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON serialization or parsing failed for '{path}': {source}")]
    JsonFailed {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

pub(crate) fn resolve_default_config_dir() -> Result<PathBuf, StorageError> {
    let config_root = dirs::config_dir().ok_or(StorageError::ConfigDirectoryUnavailable)?;
    Ok(config_root.join(DEFAULT_CONFIG_DIR_NAME))
}

pub(crate) fn write_json_atomically<ValueType: Serialize>(
    target_path: &Path,
    value: &ValueType,
) -> Result<(), StorageError> {
    if let Some(parent_directory) = target_path.parent() {
        fs::create_dir_all(parent_directory).map_err(|source| StorageError::IoFailed {
            path: parent_directory.to_path_buf(),
            source,
        })?;
    }

    let serialized_bytes =
        serde_json::to_vec_pretty(value).map_err(|source| StorageError::JsonFailed {
            path: target_path.to_path_buf(),
            source,
        })?;

    let temp_path = build_temp_sibling_path(target_path);
    let write_outcome = write_and_sync_file(&temp_path, &serialized_bytes);
    if let Err(error) = write_outcome {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    if let Err(source) = fs::rename(&temp_path, target_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(StorageError::IoFailed {
            path: target_path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

pub(crate) fn read_json_bounded<ValueType: DeserializeOwned>(
    source_path: &Path,
    max_bytes: u64,
) -> Result<ValueType, StorageError> {
    let metadata = fs::metadata(source_path).map_err(|source| StorageError::IoFailed {
        path: source_path.to_path_buf(),
        source,
    })?;
    let byte_count = metadata.len();
    if byte_count > max_bytes {
        return Err(StorageError::FileTooLarge {
            path: source_path.to_path_buf(),
            byte_count,
            max_bytes,
        });
    }
    let raw_bytes = fs::read(source_path).map_err(|source| StorageError::IoFailed {
        path: source_path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&raw_bytes).map_err(|source| StorageError::JsonFailed {
        path: source_path.to_path_buf(),
        source,
    })
}

fn build_temp_sibling_path(target_path: &Path) -> PathBuf {
    let file_stem = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("clickforge");
    let sequence_id = ATOMIC_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let process_id = std::process::id();
    let temp_file_name = format!(".{file_stem}.{process_id}.{sequence_id}.tmp");
    match target_path.parent() {
        Some(parent_directory) => parent_directory.join(temp_file_name),
        None => PathBuf::from(temp_file_name),
    }
}

fn write_and_sync_file(file_path: &Path, payload_bytes: &[u8]) -> Result<(), StorageError> {
    let mut temp_file = File::create(file_path).map_err(|source| StorageError::IoFailed {
        path: file_path.to_path_buf(),
        source,
    })?;
    temp_file
        .write_all(payload_bytes)
        .map_err(|source| StorageError::IoFailed {
            path: file_path.to_path_buf(),
            source,
        })?;
    temp_file
        .sync_all()
        .map_err(|source| StorageError::IoFailed {
            path: file_path.to_path_buf(),
            source,
        })?;
    Ok(())
}
