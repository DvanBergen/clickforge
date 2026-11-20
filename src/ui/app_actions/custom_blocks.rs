use crate::models::{
    CoordinatorCommand, CustomBlockDefinition, StatementBlock, StatementKind,
    inspect_script_validation_with_custom_blocks, lookup_custom_block,
};
use crate::ui::app::ClickForgeWorkbenchApp;
use crate::ui::custom_block_modal::CustomBlockModalAction;

impl ClickForgeWorkbenchApp {
    pub(crate) fn apply_custom_block_modal_actions(
        &mut self,
        actions: Vec<CustomBlockModalAction>,
    ) {
        for action in actions {
            match action {
                CustomBlockModalAction::SaveCustomBlock(definition) => {
                    self.save_custom_block_definition(definition);
                }
            }
        }
    }

    pub(crate) fn open_custom_block_modal_for_new(&mut self) {
        self.cancel_active_key_capture_for_modal();
        self.custom_block_modal_state
            .open_new_blank(self.custom_blocks.len());
    }

    pub(crate) fn open_custom_block_modal_for_edit(&mut self, custom_block_id: &str) {
        let Some(definition) = lookup_custom_block(&self.custom_blocks, custom_block_id).cloned()
        else {
            self.show_toast("Custom tile no longer exists.", true);
            return;
        };
        self.cancel_active_key_capture_for_modal();
        self.custom_block_modal_state.open_edit(&definition);
    }

    pub(crate) fn open_custom_block_modal_for_statements(
        &mut self,
        suggested_name: String,
        statements: Vec<StatementBlock>,
    ) {
        if statements.is_empty() {
            self.show_toast(
                "Add at least one block before saving as a custom tile.",
                true,
            );
            return;
        }
        self.cancel_active_key_capture_for_modal();
        self.custom_block_modal_state
            .open_from_statements(suggested_name, statements);
    }

    fn cancel_active_key_capture_for_modal(&mut self) {
        if self.active_key_capture.take().is_some() {
            self.coordinator.cancel_key_capture();
        }
    }

    pub(crate) fn save_custom_block_definition(&mut self, definition: CustomBlockDefinition) {
        let tile_id = definition.id.clone();
        let tile_name = definition.name.clone();
        match self.custom_block_repository.upsert(&definition) {
            Ok(updated_library) => {
                self.custom_blocks = updated_library;
                self.refresh_cached_custom_block_names_in_scripts(&tile_id, &tile_name);
                let _ = self
                    .coordinator
                    .send_command(CoordinatorCommand::SyncCustomBlocks(
                        self.custom_blocks.clone(),
                    ));
                self.show_toast(format!("Saved custom tile '{tile_name}'."), false);
            }
            Err(storage_error) => {
                self.show_toast(format!("Save tile failed: {storage_error}"), true);
            }
        }
    }

    pub(crate) fn delete_custom_block_by_id(&mut self, custom_block_id: &str) {
        let tile_name = lookup_custom_block(&self.custom_blocks, custom_block_id)
            .map(|definition| definition.name.clone())
            .unwrap_or_else(|| "Custom Tile".to_string());

        match self.custom_block_repository.delete(custom_block_id) {
            Ok(updated_library) => {
                self.custom_blocks = updated_library;
                let _ = self
                    .coordinator
                    .send_command(CoordinatorCommand::SyncCustomBlocks(
                        self.custom_blocks.clone(),
                    ));
                let affected_script_count = self
                    .scripts
                    .iter()
                    .filter(|script| {
                        script
                            .visual_program
                            .referenced_custom_block_ids()
                            .iter()
                            .any(|referenced_id| referenced_id == custom_block_id)
                    })
                    .count();
                if affected_script_count > 0 {
                    let script_word = if affected_script_count == 1 {
                        "script"
                    } else {
                        "scripts"
                    };
                    self.show_toast(
                        format!(
                            "Deleted tile '{tile_name}' ({affected_script_count} {script_word} now reference a missing tile)."
                        ),
                        true,
                    );
                } else {
                    self.show_toast(format!("Deleted custom tile '{tile_name}'."), false);
                }
            }
            Err(storage_error) => {
                self.show_toast(format!("Delete tile failed: {storage_error}"), true);
            }
        }
    }

    fn refresh_cached_custom_block_names_in_scripts(
        &mut self,
        custom_block_id: &str,
        updated_name: &str,
    ) {
        let custom_blocks_snapshot = self.custom_blocks.clone();
        for script in &mut self.scripts {
            let did_rename = update_cached_custom_block_name_in_slice(
                &mut script.visual_program.statements,
                custom_block_id,
                updated_name,
            );
            if !did_rename {
                continue;
            }
            let is_clean_and_valid = !self.dirty_script_ids.contains(&script.id)
                && inspect_script_validation_with_custom_blocks(script, &custom_blocks_snapshot)
                    .is_valid();
            if is_clean_and_valid {
                script.touch_updated_timestamp();
                let _ = self
                    .repository
                    .save_script_with_custom_blocks(script, &custom_blocks_snapshot);
                let _ = self
                    .coordinator
                    .send_command(CoordinatorCommand::UpsertScript(script.clone()));
            }
        }
    }
}

fn update_cached_custom_block_name_in_slice(
    statements: &mut [StatementBlock],
    target_custom_block_id: &str,
    updated_name: &str,
) -> bool {
    let mut did_change = false;
    for block in statements {
        match &mut block.kind {
            StatementKind::RunCustomBlock {
                custom_block_id,
                cached_name,
            } if custom_block_id == target_custom_block_id && cached_name != updated_name => {
                *cached_name = updated_name.to_string();
                did_change = true;
            }
            StatementKind::IfThen { then_blocks, .. } => {
                did_change |= update_cached_custom_block_name_in_slice(
                    then_blocks,
                    target_custom_block_id,
                    updated_name,
                );
            }
            StatementKind::IfThenElse {
                then_blocks,
                else_blocks,
                ..
            } => {
                did_change |= update_cached_custom_block_name_in_slice(
                    then_blocks,
                    target_custom_block_id,
                    updated_name,
                );
                did_change |= update_cached_custom_block_name_in_slice(
                    else_blocks,
                    target_custom_block_id,
                    updated_name,
                );
            }
            StatementKind::WhileCondition { body_blocks, .. }
            | StatementKind::RepeatCount { body_blocks, .. } => {
                did_change |= update_cached_custom_block_name_in_slice(
                    body_blocks,
                    target_custom_block_id,
                    updated_name,
                );
            }
            _ => {}
        }
    }
    did_change
}

#[cfg(test)]
mod tests {
    use clickforge_sdk::{InputTrigger, MouseButton};

    use super::update_cached_custom_block_name_in_slice;
    use crate::models::{BlockId, ConditionExpr, StatementBlock, StatementKind};

    #[test]
    fn update_cached_custom_block_name_updates_nested_references_recursively() {
        let mut statements = vec![StatementBlock {
            id: BlockId(1),
            kind: StatementKind::IfThenElse {
                condition: ConditionExpr::IsInputPressed {
                    input: InputTrigger::Mouse(MouseButton::Left),
                },
                then_blocks: vec![StatementBlock {
                    id: BlockId(2),
                    kind: StatementKind::RunCustomBlock {
                        custom_block_id: "tile-1".to_string(),
                        cached_name: "Old Name".to_string(),
                    },
                }],
                else_blocks: vec![StatementBlock {
                    id: BlockId(3),
                    kind: StatementKind::RunCustomBlock {
                        custom_block_id: "tile-other".to_string(),
                        cached_name: "Unchanged".to_string(),
                    },
                }],
            },
        }];

        let did_change =
            update_cached_custom_block_name_in_slice(&mut statements, "tile-1", "New Name");
        assert!(did_change);

        if let StatementKind::IfThenElse {
            then_blocks,
            else_blocks,
            ..
        } = &statements[0].kind
        {
            assert!(matches!(
                &then_blocks[0].kind,
                StatementKind::RunCustomBlock { cached_name, .. } if cached_name == "New Name"
            ));
            assert!(matches!(
                &else_blocks[0].kind,
                StatementKind::RunCustomBlock { cached_name, .. } if cached_name == "Unchanged"
            ));
        } else {
            panic!("expected IfThenElse container");
        }
    }
}
