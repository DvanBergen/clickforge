use clickforge_sdk::KeyCode;
use rand::RngExt;
use serde::{Deserialize, Serialize};

use crate::models::block_tree;
use crate::models::blocks::{BlockId, StatementBlock, StatementKind};
use crate::models::script::ScriptDocument;
use crate::models::validation::{
    MAX_BLOCK_NESTING_DEPTH, ValidationReport, validate_statement_slice_bounds,
};

/// Maximum number of reusable custom tiles stored in the user's library.
pub const MAX_CUSTOM_BLOCKS: usize = 64;

/// Maximum number of statement blocks permitted inside a single custom tile definition.
pub const MAX_CUSTOM_BLOCK_STEPS: usize = 64;

/// Maximum nested call depth permitted when custom tiles invoke other custom tiles.
pub const MAX_CUSTOM_BLOCK_CALL_DEPTH: usize = 8;

/// Maximum display name length in Unicode scalar values for a custom tile.
pub const MAX_CUSTOM_BLOCK_NAME_CHARS: usize = 48;

const MAX_SLUG_CHARS: usize = 24;

fn default_next_custom_block_id() -> u64 {
    1
}

/// Visual accent palette selectable per reusable custom tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CustomBlockColor {
    #[default]
    Coral,
    Blue,
    Teal,
    Gold,
    Amber,
}

impl CustomBlockColor {
    /// Returns the human-readable UI label for this custom tile color.
    pub fn display_label(self) -> &'static str {
        match self {
            Self::Coral => "Coral",
            Self::Blue => "Blue",
            Self::Teal => "Teal",
            Self::Gold => "Gold",
            Self::Amber => "Amber",
        }
    }

    /// Returns all supported custom tile colors in UI display order.
    pub fn all_colors() -> [CustomBlockColor; 5] {
        [Self::Coral, Self::Blue, Self::Teal, Self::Gold, Self::Amber]
    }
}

/// Reusable user-defined tile containing a sequence of Visual Block statements.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomBlockDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub color: CustomBlockColor,
    pub statements: Vec<StatementBlock>,
    #[serde(default = "default_next_custom_block_id")]
    pub next_block_id: u64,
    pub updated_at_epoch_ms: u64,
}

impl CustomBlockDefinition {
    /// Generates a unique slug-based identifier from `name`, wall-clock time, and random hex.
    pub fn generate_unique_id(name: &str) -> String {
        let slug = sanitize_id_slug(name);
        let timestamp_ms = ScriptDocument::current_unix_timestamp_ms();
        let random_suffix: u32 = rand::rng().random();
        format!("custom-{slug}-{timestamp_ms}-{random_suffix:08x}")
    }

    /// Creates an empty custom tile definition with a freshly generated identifier.
    pub fn new_empty(
        name: impl Into<String>,
        description: impl Into<String>,
        color: CustomBlockColor,
    ) -> Self {
        let resolved_name = name.into();
        let updated_at_epoch_ms = ScriptDocument::current_unix_timestamp_ms();
        Self {
            id: Self::generate_unique_id(&resolved_name),
            name: resolved_name,
            description: description.into(),
            color,
            statements: Vec::new(),
            next_block_id: default_next_custom_block_id(),
            updated_at_epoch_ms,
        }
    }

    /// Creates a custom tile from existing statements, reallocating fresh unique `BlockId`s.
    pub fn from_statements(
        name: impl Into<String>,
        description: impl Into<String>,
        color: CustomBlockColor,
        statements: Vec<StatementBlock>,
    ) -> Self {
        let mut definition = Self::new_empty(name, description, color);
        let mut next_block_id = definition.next_block_id;
        definition.statements =
            block_tree::reallocate_block_ids_in_slice(statements, &mut next_block_id);
        definition.next_block_id = next_block_id;
        definition
    }

    /// Allocates the next monotonic `BlockId` within this custom tile without overflow.
    pub fn allocate_block_id(&mut self) -> BlockId {
        let current_id = self.next_block_id.max(1);
        self.next_block_id = current_id.saturating_add(1);
        BlockId(current_id)
    }

    /// Allocates a new `StatementBlock` with a unique `BlockId` owned by this custom tile.
    pub fn create_block(&mut self, kind: StatementKind) -> StatementBlock {
        let block_id = self.allocate_block_id();
        StatementBlock { id: block_id, kind }
    }

    /// Collects every distinct keyboard key directly emitted by statements inside this tile.
    pub fn collect_emitted_keys(&self) -> Vec<KeyCode> {
        let mut emitted_keys = Vec::new();
        block_tree::collect_emitted_keys_in_slice(&self.statements, &mut emitted_keys);
        emitted_keys
    }

    /// Collects every distinct keyboard key directly watched or checked inside this tile.
    pub fn collect_condition_keys(&self) -> Vec<KeyCode> {
        let mut condition_keys = Vec::new();
        block_tree::collect_condition_keys_in_slice(&self.statements, &mut condition_keys);
        condition_keys
    }

    /// Counts all statement blocks recursively inside this custom tile.
    pub fn total_block_count(&self) -> usize {
        block_tree::count_blocks_in_slice(&self.statements)
    }

    /// Collects every distinct custom tile identifier directly referenced inside this tile.
    pub fn referenced_custom_block_ids(&self) -> Vec<String> {
        let mut referenced_ids = Vec::new();
        block_tree::collect_custom_block_ids_in_slice(&self.statements, &mut referenced_ids);
        referenced_ids
    }

    /// Updates `updated_at_epoch_ms` to the current Unix timestamp in milliseconds.
    pub fn touch_updated_timestamp(&mut self) {
        self.updated_at_epoch_ms = ScriptDocument::current_unix_timestamp_ms();
    }
}

/// Looks up a custom tile definition by exact identifier inside `library`.
pub fn lookup_custom_block<'library>(
    library: &'library [CustomBlockDefinition],
    custom_block_id: &str,
) -> Option<&'library CustomBlockDefinition> {
    library
        .iter()
        .find(|definition| definition.id == custom_block_id)
}

/// Validates a custom tile definition against length, step count, timing, depth, and cycle rules.
pub fn validate_custom_block_definition(
    definition: &CustomBlockDefinition,
    library: &[CustomBlockDefinition],
) -> Result<(), String> {
    if definition.id.trim().is_empty() {
        return Err("Custom tile identifier cannot be empty.".to_string());
    }
    let trimmed_name = definition.name.trim();
    if trimmed_name.is_empty() {
        return Err("Custom tile name cannot be empty.".to_string());
    }
    let name_char_count = trimmed_name.chars().count();
    if name_char_count > MAX_CUSTOM_BLOCK_NAME_CHARS {
        return Err(format!(
            "Custom tile name cannot exceed {MAX_CUSTOM_BLOCK_NAME_CHARS} characters."
        ));
    }

    let step_count = definition.total_block_count();
    if definition.statements.is_empty() || step_count == 0 {
        return Err("Custom tile must contain at least one statement block.".to_string());
    }
    if step_count > MAX_CUSTOM_BLOCK_STEPS {
        return Err(format!(
            "Custom tile has {step_count} blocks, exceeding limit of {MAX_CUSTOM_BLOCK_STEPS}."
        ));
    }

    let nesting_depth = block_tree::max_depth_in_slice(&definition.statements, 0);
    if nesting_depth > MAX_BLOCK_NESTING_DEPTH {
        return Err(format!(
            "Custom tile nesting depth {nesting_depth} exceeds limit of {MAX_BLOCK_NESTING_DEPTH}."
        ));
    }

    let mut bounds_report = ValidationReport::default();
    validate_statement_slice_bounds(&definition.statements, &mut bounds_report);
    if let Some(first_issue) = bounds_report.issues.first() {
        return Err(first_issue.message.clone());
    }

    let mut active_call_stack: Vec<&str> = Vec::with_capacity(MAX_CUSTOM_BLOCK_CALL_DEPTH);
    active_call_stack.push(definition.id.as_str());
    validate_custom_call_graph(definition, library, &mut active_call_stack)
}

fn validate_custom_call_graph<'graph>(
    current_definition: &'graph CustomBlockDefinition,
    library: &'graph [CustomBlockDefinition],
    active_call_stack: &mut Vec<&'graph str>,
) -> Result<(), String> {
    let referenced_ids = current_definition.referenced_custom_block_ids();
    for referenced_id in referenced_ids {
        if active_call_stack.contains(&referenced_id.as_str()) {
            return Err(format!(
                "Cyclic custom tile recursion detected involving '{referenced_id}'."
            ));
        }
        if active_call_stack.len() >= MAX_CUSTOM_BLOCK_CALL_DEPTH {
            return Err(format!(
                "Custom tile call depth exceeds limit of {MAX_CUSTOM_BLOCK_CALL_DEPTH}."
            ));
        }
        let Some(target_definition) = lookup_custom_block(library, &referenced_id) else {
            return Err(format!(
                "Custom tile references missing custom tile '{referenced_id}'."
            ));
        };
        active_call_stack.push(target_definition.id.as_str());
        let nested_outcome =
            validate_custom_call_graph(target_definition, library, active_call_stack);
        active_call_stack.pop();
        nested_outcome?;
    }
    Ok(())
}

fn sanitize_id_slug(raw_name: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_hyphen = false;

    for character in raw_name.chars() {
        if slug.len() >= MAX_SLUG_CHARS {
            break;
        }
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            previous_was_hyphen = false;
        } else if !slug.is_empty() && !previous_was_hyphen {
            slug.push('-');
            previous_was_hyphen = true;
        }
    }

    let trimmed_slug = slug.trim_matches('-');
    if trimmed_slug.is_empty() {
        "tile".to_string()
    } else {
        trimmed_slug.to_string()
    }
}

#[cfg(test)]
mod tests {
    use clickforge_sdk::{KeyCode, MouseButton};

    use super::*;

    #[test]
    fn from_statements_reallocates_unique_block_ids_across_nested_subtrees() {
        let duplicated_child = StatementBlock {
            id: BlockId(42),
            kind: StatementKind::KeyTap {
                key_code: KeyCode::E,
                hold_duration_ms: 10,
            },
        };
        let duplicated_parent = StatementBlock {
            id: BlockId(42),
            kind: StatementKind::RepeatCount {
                repeat_count: 2,
                body_blocks: vec![duplicated_child],
            },
        };

        let definition = CustomBlockDefinition::from_statements(
            "Double Tap E",
            "Presses E twice",
            CustomBlockColor::Teal,
            vec![duplicated_parent],
        );

        assert_eq!(definition.statements.len(), 1);
        assert_eq!(definition.statements[0].id, BlockId(1));
        if let StatementKind::RepeatCount { body_blocks, .. } = &definition.statements[0].kind {
            assert_eq!(body_blocks[0].id, BlockId(2));
        } else {
            panic!("expected RepeatCount root statement");
        }
        assert_eq!(definition.next_block_id, 3);
        assert_eq!(definition.collect_emitted_keys(), vec![KeyCode::E]);
    }

    #[test]
    fn validate_custom_block_definition_enforces_name_steps_timing_and_cycles() {
        let empty_tile = CustomBlockDefinition::new_empty("Empty", "", CustomBlockColor::Coral);
        assert!(validate_custom_block_definition(&empty_tile, &[]).is_err());

        let valid_click = StatementBlock {
            id: BlockId(1),
            kind: StatementKind::MouseClick {
                mouse_button: MouseButton::Left,
                hold_duration_ms: 12,
            },
        };
        let long_name = "x".repeat(MAX_CUSTOM_BLOCK_NAME_CHARS + 1);
        let too_long_tile = CustomBlockDefinition::from_statements(
            long_name,
            "",
            CustomBlockColor::Gold,
            vec![valid_click.clone()],
        );
        assert!(validate_custom_block_definition(&too_long_tile, &[]).is_err());

        let mut self_cycle_tile =
            CustomBlockDefinition::new_empty("Self Recursive", "", CustomBlockColor::Amber);
        let self_id = self_cycle_tile.id.clone();
        let recursive_step = self_cycle_tile.create_block(StatementKind::RunCustomBlock {
            custom_block_id: self_id,
            cached_name: "Self Recursive".to_string(),
        });
        self_cycle_tile.statements.push(recursive_step);
        assert!(validate_custom_block_definition(&self_cycle_tile, &[]).is_err());

        let bad_timing_tile = CustomBlockDefinition::from_statements(
            "Bad Wait",
            "",
            CustomBlockColor::Blue,
            vec![StatementBlock {
                id: BlockId(1),
                kind: StatementKind::WaitRandomUniform {
                    min_delay_ms: 200,
                    max_delay_ms: 50,
                },
            }],
        );
        assert!(validate_custom_block_definition(&bad_timing_tile, &[]).is_err());

        let valid_tile = CustomBlockDefinition::from_statements(
            "Quick Left Click",
            "Single left click",
            CustomBlockColor::Coral,
            vec![valid_click],
        );
        assert!(validate_custom_block_definition(&valid_tile, &[]).is_ok());
        assert_eq!(CustomBlockColor::all_colors().len(), 5);
        assert_eq!(CustomBlockColor::Coral.display_label(), "Coral");
    }
}
