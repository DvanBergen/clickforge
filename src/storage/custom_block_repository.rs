use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::models::{CustomBlockDefinition, MAX_CUSTOM_BLOCKS, validate_custom_block_definition};
use crate::storage::{
    StorageError, read_json_bounded, resolve_default_config_dir, write_json_atomically,
};

/// File name for persisted reusable custom tiles under `~/.config/clickforge/`.
pub const CUSTOM_BLOCKS_FILE_NAME: &str = "custom_blocks.json";

/// Maximum custom tiles JSON file size accepted when loading from disk (1 MiB).
pub const MAX_CUSTOM_BLOCKS_FILE_BYTES: u64 = 1_048_576;

/// Atomic JSON repository for ClickForge reusable custom tiles (`custom_blocks.json`).
#[derive(Debug, Clone)]
pub struct CustomBlockRepository {
    file_path: PathBuf,
}

impl CustomBlockRepository {
    /// Opens the default custom tiles file at `~/.config/clickforge/custom_blocks.json`.
    pub fn new_default() -> Result<Self, StorageError> {
        let default_directory = resolve_default_config_dir()?;
        Ok(Self::new_with_base_dir(default_directory))
    }

    /// Opens the custom tiles file rooted inside `base_directory`.
    pub fn new_with_base_dir(base_directory: PathBuf) -> Self {
        Self {
            file_path: base_directory.join(CUSTOM_BLOCKS_FILE_NAME),
        }
    }

    /// Returns the full path to `custom_blocks.json`.
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Loads all persisted custom tile definitions, returning an empty list if absent.
    pub fn load_all(&self) -> Result<Vec<CustomBlockDefinition>, StorageError> {
        if !self.file_path.exists() {
            return Ok(Vec::new());
        }
        let mut loaded_definitions: Vec<CustomBlockDefinition> =
            read_json_bounded(&self.file_path, MAX_CUSTOM_BLOCKS_FILE_BYTES)?;
        if loaded_definitions.len() > MAX_CUSTOM_BLOCKS {
            loaded_definitions.truncate(MAX_CUSTOM_BLOCKS);
        }
        Ok(loaded_definitions)
    }

    /// Validates all definitions and atomically writes the full custom tile library to disk.
    pub fn save_all(&self, definitions: &[CustomBlockDefinition]) -> Result<(), StorageError> {
        if definitions.len() > MAX_CUSTOM_BLOCKS {
            return Err(StorageError::InvalidCustomBlock {
                reason: format!(
                    "Custom tile count {} exceeds limit of {MAX_CUSTOM_BLOCKS}.",
                    definitions.len()
                ),
            });
        }
        let mut seen_ids = HashSet::with_capacity(definitions.len());
        for definition in definitions {
            if !seen_ids.insert(definition.id.as_str()) {
                return Err(StorageError::InvalidCustomBlock {
                    reason: format!("Duplicate custom tile identifier '{}'.", definition.id),
                });
            }
            validate_custom_block_definition(definition, definitions)
                .map_err(|reason| StorageError::InvalidCustomBlock { reason })?;
        }
        write_json_atomically(&self.file_path, &definitions)
    }

    /// Inserts or updates a single custom tile, persists atomically, and returns the updated list.
    pub fn upsert(
        &self,
        definition: &CustomBlockDefinition,
    ) -> Result<Vec<CustomBlockDefinition>, StorageError> {
        let mut updated_definitions = self.load_all()?;
        if let Some(existing_entry) = updated_definitions
            .iter_mut()
            .find(|entry| entry.id == definition.id)
        {
            *existing_entry = definition.clone();
        } else {
            if updated_definitions.len() >= MAX_CUSTOM_BLOCKS {
                return Err(StorageError::InvalidCustomBlock {
                    reason: format!(
                        "Cannot add custom tile '{}': library limit of {MAX_CUSTOM_BLOCKS} reached.",
                        definition.name
                    ),
                });
            }
            updated_definitions.push(definition.clone());
        }

        validate_custom_block_definition(definition, &updated_definitions)
            .map_err(|reason| StorageError::InvalidCustomBlock { reason })?;
        write_json_atomically(&self.file_path, &updated_definitions)?;
        Ok(updated_definitions)
    }

    /// Removes a custom tile by identifier, persists atomically, and returns the updated list.
    pub fn delete(
        &self,
        custom_block_id: &str,
    ) -> Result<Vec<CustomBlockDefinition>, StorageError> {
        let mut updated_definitions = self.load_all()?;
        updated_definitions.retain(|entry| entry.id != custom_block_id);
        write_json_atomically(&self.file_path, &updated_definitions)?;
        Ok(updated_definitions)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use clickforge_sdk::{KeyCode, MouseButton};

    use super::*;
    use crate::models::{BlockId, CustomBlockColor, ScriptDocument, StatementBlock, StatementKind};

    fn create_ephemeral_test_dir(test_label: &str) -> Result<PathBuf, std::io::Error> {
        let timestamp_ms = ScriptDocument::current_unix_timestamp_ms();
        let random_suffix: u32 = rand::random();
        let directory_path = std::env::temp_dir().join(format!(
            "clickforge-custom-blocks-test-{test_label}-{timestamp_ms}-{random_suffix:08x}"
        ));
        fs::create_dir_all(&directory_path)?;
        Ok(directory_path)
    }

    #[test]
    fn custom_block_repository_round_trips_save_upsert_and_delete_atomically()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = create_ephemeral_test_dir("roundtrip")?;
        let repository = CustomBlockRepository::new_with_base_dir(temp_dir.clone());

        assert!(repository.load_all()?.is_empty());

        let click_tile = CustomBlockDefinition::from_statements(
            "Rapid Click",
            "Left click 12ms",
            CustomBlockColor::Coral,
            vec![StatementBlock {
                id: BlockId(1),
                kind: StatementKind::MouseClick {
                    mouse_button: MouseButton::Left,
                    hold_duration_ms: 12,
                },
            }],
        );
        let tap_tile = CustomBlockDefinition::from_statements(
            "Tap E",
            "Press E once",
            CustomBlockColor::Teal,
            vec![StatementBlock {
                id: BlockId(1),
                kind: StatementKind::KeyTap {
                    key_code: KeyCode::E,
                    hold_duration_ms: 10,
                },
            }],
        );

        let after_first = repository.upsert(&click_tile)?;
        assert_eq!(after_first.len(), 1);
        let after_second = repository.upsert(&tap_tile)?;
        assert_eq!(after_second.len(), 2);

        let mut renamed_click = click_tile.clone();
        renamed_click.name = "Fast Click".to_string();
        let after_update = repository.upsert(&renamed_click)?;
        assert_eq!(after_update.len(), 2);
        assert_eq!(after_update[0].name, "Fast Click");

        let reloaded = repository.load_all()?;
        assert_eq!(reloaded, after_update);

        for entry_result in fs::read_dir(&temp_dir)? {
            let file_name = entry_result?.file_name();
            assert!(!file_name.to_string_lossy().ends_with(".tmp"));
        }

        let after_delete = repository.delete(&click_tile.id)?;
        assert_eq!(after_delete.len(), 1);
        assert_eq!(after_delete[0].id, tap_tile.id);
        assert_eq!(repository.load_all()?, after_delete);

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn custom_block_repository_rejects_invalid_and_cyclic_definitions()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = create_ephemeral_test_dir("reject-invalid")?;
        let repository = CustomBlockRepository::new_with_base_dir(temp_dir.clone());

        let empty_tile = CustomBlockDefinition::new_empty("Empty", "", CustomBlockColor::Gold);
        assert!(matches!(
            repository.upsert(&empty_tile),
            Err(StorageError::InvalidCustomBlock { .. })
        ));

        let mut tile_alpha = CustomBlockDefinition::from_statements(
            "Alpha",
            "",
            CustomBlockColor::Blue,
            vec![StatementBlock {
                id: BlockId(1),
                kind: StatementKind::RunCustomBlock {
                    custom_block_id: "tile-beta".to_string(),
                    cached_name: "Beta".to_string(),
                },
            }],
        );
        tile_alpha.id = "tile-alpha".to_string();

        let mut tile_beta = CustomBlockDefinition::from_statements(
            "Beta",
            "",
            CustomBlockColor::Amber,
            vec![StatementBlock {
                id: BlockId(1),
                kind: StatementKind::RunCustomBlock {
                    custom_block_id: "tile-alpha".to_string(),
                    cached_name: "Alpha".to_string(),
                },
            }],
        );
        tile_beta.id = "tile-beta".to_string();

        assert!(matches!(
            repository.save_all(&[tile_alpha, tile_beta]),
            Err(StorageError::InvalidCustomBlock { .. })
        ));

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }
}
