mod custom_blocks;
mod history;

use std::time::{Duration, Instant};

use clickforge_sdk::KeyCode;
use egui::{Key, Modifiers};

use crate::engine::generate_rust_source_with_custom_blocks;
use crate::models::{
    CoordinatorCommand, GameTargetRule, ScriptDocument, ScriptMode, TargetScope,
    inspect_script_validation_with_custom_blocks,
};
use crate::ui::active_dashboard::ActiveDashboardAction;
use crate::ui::app::{ClickForgeWorkbenchApp, DeletedScriptUndoEntry, WorkbenchTab};
use crate::ui::block_canvas::BlockCanvasAction;
use crate::ui::block_palette::PaletteAction;
use crate::ui::rust_editor_panel::RustEditorAction;
use crate::ui::script_settings_bar::SettingsBarAction;
use crate::ui::sidebar::{NewScriptTemplateChoice, SidebarAction, script_mode_for_template};
use crate::ui::target_picker_modal::TargetPickerAction;

pub(crate) use history::VisualProgramHistory;

const DELETE_UNDO_TOAST_DURATION_MS: u64 = 5_000;

impl ClickForgeWorkbenchApp {
    pub(crate) fn process_keyboard_shortcuts(&mut self, egui_context: &egui::Context) {
        if self.active_key_capture.is_some() {
            return;
        }

        let did_press_quit =
            egui_context.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::Q));
        if did_press_quit {
            self.request_clean_quit(egui_context);
            return;
        }

        if self.target_picker_state.is_open
            || self.new_script_modal_state.is_open
            || self.custom_block_modal_state.is_open
        {
            return;
        }

        let did_press_save =
            egui_context.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::S));
        if did_press_save {
            self.save_active_script();
            return;
        }

        if egui_context.egui_wants_keyboard_input() {
            return;
        }

        if self.active_tab != WorkbenchTab::Editor {
            return;
        }

        let is_visual_mode = self
            .active_script_index()
            .is_some_and(|index| self.scripts[index].mode == ScriptMode::VisualBlocks);
        if !is_visual_mode {
            return;
        }

        let did_press_redo = egui_context.input_mut(|input| {
            input.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)
                || input.consume_key(Modifiers::COMMAND, Key::Y)
        });
        if did_press_redo {
            self.redo_active_visual_edit();
            return;
        }

        let did_press_undo =
            egui_context.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::Z));
        if did_press_undo {
            self.undo_active_visual_edit();
        }
    }

    pub(crate) fn apply_sidebar_actions(
        &mut self,
        egui_context: &egui::Context,
        actions: Vec<SidebarAction>,
    ) {
        for action in actions {
            match action {
                SidebarAction::SelectTab(target_tab) => {
                    self.switch_workbench_tab(target_tab);
                }
                SidebarAction::SelectScript(script_id) => {
                    self.selected_script_id = Some(script_id);
                    self.active_key_capture = None;
                    self.coordinator.cancel_key_capture();
                    self.persist_settings_quietly();
                }
                SidebarAction::OpenScriptInEditor(script_id) => {
                    self.selected_script_id = Some(script_id);
                    self.switch_workbench_tab(WorkbenchTab::Editor);
                    self.persist_settings_quietly();
                }
                SidebarAction::ToggleScriptEnabled {
                    script_id,
                    is_enabled,
                } => {
                    self.set_script_enabled_by_id(&script_id, is_enabled);
                }
                SidebarAction::DuplicateScript(script_id) => {
                    self.duplicate_script_by_id(&script_id);
                }
                SidebarAction::DeleteScript(script_id) => {
                    self.delete_script_by_id(&script_id);
                }
                SidebarAction::OpenNewScriptModal => {
                    self.new_script_modal_state
                        .open_with_defaults(self.scripts.len());
                }
                SidebarAction::CreateScript {
                    name,
                    is_selected_game_scope,
                    template_choice,
                    kill_switch_key,
                } => {
                    self.create_script_from_modal(
                        name,
                        is_selected_game_scope,
                        template_choice,
                        kill_switch_key,
                    );
                }
                SidebarAction::SetMinimizeToTray(is_minimizing) => {
                    self.settings.minimize_to_tray_on_close = is_minimizing;
                    self.persist_settings_quietly();
                }
                SidebarAction::QuitApplication => {
                    self.request_clean_quit(egui_context);
                }
            }
        }
    }

    pub(crate) fn apply_active_dashboard_actions(&mut self, actions: Vec<ActiveDashboardAction>) {
        for action in actions {
            match action {
                ActiveDashboardAction::SelectScript(script_id) => {
                    self.selected_script_id = Some(script_id);
                    self.persist_settings_quietly();
                }
                ActiveDashboardAction::EditScript(script_id) => {
                    self.selected_script_id = Some(script_id);
                    self.switch_workbench_tab(WorkbenchTab::Editor);
                    self.persist_settings_quietly();
                }
                ActiveDashboardAction::OpenEditorTab => {
                    self.switch_workbench_tab(WorkbenchTab::Editor);
                }
                ActiveDashboardAction::OpenNewScriptModal => {
                    self.new_script_modal_state
                        .open_with_defaults(self.scripts.len());
                }
                ActiveDashboardAction::OpenTargetPickerForScript(script_id) => {
                    self.selected_script_id = Some(script_id);
                    self.persist_settings_quietly();
                    self.open_target_picker_dialog();
                }
                ActiveDashboardAction::ToggleScriptEnabled {
                    script_id,
                    is_enabled,
                } => {
                    self.set_script_enabled_by_id(&script_id, is_enabled);
                }
                ActiveDashboardAction::EmergencyStopAll => {
                    let _ = self
                        .coordinator
                        .send_command(CoordinatorCommand::EmergencyStopAll {
                            reason: "UI STOP ALL button".to_string(),
                        });
                    self.show_toast("Emergency stop triggered.", true);
                }
                ActiveDashboardAction::ClearEmergencyStop => {
                    let _ = self
                        .coordinator
                        .send_command(CoordinatorCommand::ClearEmergencyStop);
                    self.show_toast("Emergency stop cleared.", false);
                }
                ActiveDashboardAction::DisableAllScripts => {
                    self.disable_all_scripts();
                }
                ActiveDashboardAction::RefreshFocusedWindow => {
                    self.window_watcher.refresh_now();
                    let _ = self
                        .coordinator
                        .send_command(CoordinatorCommand::RefreshWindowsAndProcesses);
                }
            }
        }
    }

    pub(crate) fn disable_all_scripts(&mut self) {
        let custom_blocks_snapshot = self.custom_blocks.clone();
        for script in &mut self.scripts {
            if !script.is_enabled {
                continue;
            }
            script.is_enabled = false;
            script.touch_updated_timestamp();
            if inspect_script_validation_with_custom_blocks(script, &custom_blocks_snapshot)
                .is_valid()
                && self
                    .repository
                    .save_script_with_custom_blocks(script, &custom_blocks_snapshot)
                    .is_ok()
            {
                self.dirty_script_ids.remove(&script.id);
            }
        }
        let _ = self
            .coordinator
            .send_command(CoordinatorCommand::DisableAllScripts);
        self.show_toast("All scripts disabled.", false);
    }

    fn switch_workbench_tab(&mut self, target_tab: WorkbenchTab) {
        self.active_tab = target_tab;
        self.active_key_capture = None;
        self.coordinator.cancel_key_capture();
    }

    fn open_target_picker_dialog(&mut self) {
        self.window_watcher.refresh_now();
        let _ = self
            .coordinator
            .send_command(CoordinatorCommand::RefreshWindowsAndProcesses);
        self.target_picker_state.open();
    }

    pub(crate) fn apply_settings_bar_actions(&mut self, actions: Vec<SettingsBarAction>) {
        for action in actions {
            match action {
                SettingsBarAction::OpenTargetPicker => {
                    self.open_target_picker_dialog();
                }
                SettingsBarAction::SetScopeGlobal => {
                    if let Some(active_index) = self.active_script_index() {
                        self.scripts[active_index].target_scope = TargetScope::Global;
                        let script_id = self.scripts[active_index].id.clone();
                        self.mark_script_dirty(&script_id);
                    }
                }
                SettingsBarAction::BeginKeyCapture(target) => {
                    self.active_key_capture = Some(target);
                    self.coordinator.begin_key_capture();
                }
                SettingsBarAction::CancelKeyCapture => {
                    self.active_key_capture = None;
                    self.coordinator.cancel_key_capture();
                }
                SettingsBarAction::ClearArmToggleKey => {
                    if let Some(active_index) = self.active_script_index() {
                        self.scripts[active_index].arm_toggle_key = None;
                        let script_id = self.scripts[active_index].id.clone();
                        self.mark_script_dirty(&script_id);
                    }
                }
                SettingsBarAction::SaveActiveScript => {
                    self.save_active_script();
                }
                SettingsBarAction::ToggleActiveScriptEnabled(is_enabled) => {
                    if let Some(selected_id) = self.selected_script_id.clone() {
                        self.set_script_enabled_by_id(&selected_id, is_enabled);
                    }
                }
                SettingsBarAction::OpenActiveDashboard => {
                    self.switch_workbench_tab(WorkbenchTab::Active);
                }
                SettingsBarAction::EmergencyStopAll => {
                    let _ = self
                        .coordinator
                        .send_command(CoordinatorCommand::EmergencyStopAll {
                            reason: "UI STOP ALL button".to_string(),
                        });
                    self.show_toast("Emergency stop triggered.", true);
                }
                SettingsBarAction::ClearEmergencyStop => {
                    let _ = self
                        .coordinator
                        .send_command(CoordinatorCommand::ClearEmergencyStop);
                    self.show_toast("Emergency stop cleared.", false);
                }
                SettingsBarAction::MarkActiveScriptDirty => {
                    if let Some(selected_id) = self.selected_script_id.clone() {
                        self.mark_script_dirty(&selected_id);
                    }
                }
                SettingsBarAction::ExportToRust => {
                    self.export_active_script_to_rust();
                }
            }
        }
    }

    pub(crate) fn apply_target_picker_actions(&mut self, actions: Vec<TargetPickerAction>) {
        for action in actions {
            match action {
                TargetPickerAction::RefreshTargets => {
                    self.window_watcher.refresh_now();
                    let _ = self
                        .coordinator
                        .send_command(CoordinatorCommand::RefreshWindowsAndProcesses);
                }
                TargetPickerAction::AttachTargetRule(target_rule) => {
                    if let Some(active_index) = self.active_script_index() {
                        let target_name = target_rule.display_name.clone();
                        self.scripts[active_index].target_scope =
                            TargetScope::SelectedGame(target_rule);
                        self.sync_target_change_for_active_script(
                            active_index,
                            format!("Attached target: {target_name}"),
                        );
                    }
                }
                TargetPickerAction::SetScopeGlobal => {
                    if let Some(active_index) = self.active_script_index() {
                        self.scripts[active_index].target_scope = TargetScope::Global;
                        self.sync_target_change_for_active_script(
                            active_index,
                            "Switched script scope to Global.".to_string(),
                        );
                    }
                }
            }
        }
    }

    pub(crate) fn apply_palette_actions(&mut self, actions: Vec<PaletteAction>) {
        if actions.is_empty() {
            return;
        }
        let active_index = self.active_script_index();
        let before_snapshot = active_index.map(|index| {
            (
                self.scripts[index].id.clone(),
                self.scripts[index].visual_program.clone(),
            )
        });
        let mut did_modify_active_script = false;

        for action in actions {
            match action {
                PaletteAction::ApplyTrigger(new_trigger) => {
                    if let Some(index) = active_index {
                        self.scripts[index].visual_program.trigger = new_trigger;
                        did_modify_active_script = true;
                    }
                }
                PaletteAction::AppendStatement(statement_kind) => {
                    if let Some(index) = active_index {
                        self.scripts[index]
                            .visual_program
                            .append_root_block(statement_kind);
                        did_modify_active_script = true;
                    }
                }
                PaletteAction::OpenNewCustomBlockModal => {
                    self.open_custom_block_modal_for_new();
                }
                PaletteAction::EditCustomBlock(custom_block_id) => {
                    self.open_custom_block_modal_for_edit(&custom_block_id);
                }
                PaletteAction::DeleteCustomBlock(custom_block_id) => {
                    self.delete_custom_block_by_id(&custom_block_id);
                }
            }
        }
        if did_modify_active_script
            && let Some(index) = active_index
            && let Some((script_id, before_program)) = before_snapshot
        {
            let current_block_count = self.scripts[index].visual_program.total_block_count();
            self.record_visual_program_undo_snapshot(
                &script_id,
                before_program,
                current_block_count,
                false,
            );
            self.mark_script_dirty(&script_id);
        }
    }

    pub(crate) fn apply_canvas_actions(&mut self, actions: Vec<BlockCanvasAction>) {
        let active_index = self.active_script_index();
        for action in actions {
            match action {
                BlockCanvasAction::MarkDirty => {
                    if let Some(index) = active_index {
                        let script_id = self.scripts[index].id.clone();
                        self.mark_script_dirty(&script_id);
                    }
                }
                BlockCanvasAction::ExportToRust => {
                    self.export_active_script_to_rust();
                }
                BlockCanvasAction::EditCustomBlock(custom_block_id) => {
                    self.open_custom_block_modal_for_edit(&custom_block_id);
                }
                BlockCanvasAction::UnpackCustomBlock {
                    tile_name,
                    unpacked_block_count,
                } => {
                    let block_word = if unpacked_block_count == 1 {
                        "block"
                    } else {
                        "blocks"
                    };
                    self.show_toast(
                        format!(
                            "Unpacked '{tile_name}' inline ({unpacked_block_count} {block_word})."
                        ),
                        false,
                    );
                }
                BlockCanvasAction::SaveBlockAsCustomTile {
                    suggested_name,
                    statements,
                } => {
                    self.open_custom_block_modal_for_statements(suggested_name, statements);
                }
            }
        }
    }

    pub(crate) fn apply_rust_editor_actions(&mut self, actions: Vec<RustEditorAction>) {
        let Some(active_index) = self.active_script_index() else {
            return;
        };
        for action in actions {
            match action {
                RustEditorAction::MarkDirty => {
                    let script_id = self.scripts[active_index].id.clone();
                    self.mark_script_dirty(&script_id);
                }
                RustEditorAction::CompileActiveScript => {
                    let script_snapshot = self.scripts[active_index].clone();
                    let validation_report = inspect_script_validation_with_custom_blocks(
                        &script_snapshot,
                        &self.custom_blocks,
                    );
                    if !validation_report.is_valid() {
                        self.show_toast(
                            "Compilation blocked until validation issues are resolved.",
                            true,
                        );
                        continue;
                    }
                    match self
                        .coordinator
                        .send_command(CoordinatorCommand::CompileRustScript(script_snapshot))
                    {
                        Ok(()) => self.show_toast("Started cargo build check...", false),
                        Err(send_error) => self.show_toast(send_error, true),
                    }
                }
            }
        }
    }

    pub(crate) fn save_active_script(&mut self) {
        let Some(active_index) = self.active_script_index() else {
            return;
        };
        self.scripts[active_index].touch_updated_timestamp();
        let script_snapshot = self.scripts[active_index].clone();

        match self
            .repository
            .save_script_with_custom_blocks(&script_snapshot, &self.custom_blocks)
        {
            Ok(_) => {
                self.dirty_script_ids.remove(&script_snapshot.id);
                let _ = self
                    .coordinator
                    .send_command(CoordinatorCommand::UpsertScript(script_snapshot.clone()));
                self.show_toast(format!("Saved '{}'.", script_snapshot.name), false);
            }
            Err(storage_error) => {
                self.show_toast(format!("Save blocked: {storage_error}"), true);
            }
        }
    }

    pub(crate) fn restore_last_deleted_script(&mut self) {
        let Some(deleted_entry) = self.last_deleted_script.take() else {
            return;
        };
        let restored_script = deleted_entry.script;
        let restored_id = restored_script.id.clone();
        let restored_name = restored_script.name.clone();
        let insert_index = deleted_entry.original_index.min(self.scripts.len());

        if self
            .repository
            .save_script_with_custom_blocks(&restored_script, &self.custom_blocks)
            .is_ok()
        {
            let _ = self
                .coordinator
                .send_command(CoordinatorCommand::UpsertScript(restored_script.clone()));
            self.dirty_script_ids.remove(&restored_id);
        } else {
            self.mark_script_dirty(&restored_id);
        }

        self.scripts.insert(insert_index, restored_script);
        self.selected_script_id = Some(restored_id);
        self.persist_settings_quietly();
        self.show_toast(format!("Restored '{restored_name}'."), false);
    }

    fn export_active_script_to_rust(&mut self) {
        let Some(active_index) = self.active_script_index() else {
            return;
        };
        let generated_rust = generate_rust_source_with_custom_blocks(
            &self.scripts[active_index],
            &self.custom_blocks,
        );
        self.scripts[active_index].rust_source = generated_rust;
        self.scripts[active_index].mode = ScriptMode::AdvancedRust;
        let script_id = self.scripts[active_index].id.clone();
        self.mark_script_dirty(&script_id);
        self.show_toast(
            "Exported visual blocks to Rust and switched to Advanced Rust mode.",
            false,
        );
    }

    fn set_script_enabled_by_id(&mut self, script_id: &str, is_enabled: bool) {
        let Some(script_index) = self.scripts.iter().position(|item| item.id == script_id) else {
            return;
        };
        if is_enabled {
            let validation_report = inspect_script_validation_with_custom_blocks(
                &self.scripts[script_index],
                &self.custom_blocks,
            );
            if !validation_report.is_valid() {
                let first_reason = validation_report
                    .issues
                    .first()
                    .map(|issue| issue.message.clone())
                    .unwrap_or_else(|| "Validation failed.".to_string());
                self.show_toast(format!("Enable blocked: {first_reason}"), true);
                return;
            }
        }

        self.scripts[script_index].is_enabled = is_enabled;
        self.scripts[script_index].touch_updated_timestamp();
        let updated_script = self.scripts[script_index].clone();

        if self
            .repository
            .save_script_with_custom_blocks(&updated_script, &self.custom_blocks)
            .is_ok()
        {
            self.dirty_script_ids.remove(&updated_script.id);
        }
        let _ = self
            .coordinator
            .send_command(CoordinatorCommand::UpsertScript(updated_script));
        let _ = self
            .coordinator
            .send_command(CoordinatorCommand::SetScriptEnabled {
                script_id: script_id.to_string(),
                is_enabled,
            });
    }

    fn create_script_from_modal(
        &mut self,
        name: String,
        is_selected_game_scope: bool,
        template_choice: NewScriptTemplateChoice,
        kill_switch_key: Option<KeyCode>,
    ) {
        let scope = if is_selected_game_scope {
            TargetScope::SelectedGame(GameTargetRule::default())
        } else {
            TargetScope::Global
        };

        let mut new_script = match template_choice {
            NewScriptTemplateChoice::BuildingUpgradeTemplate => {
                let mut template_script = ScriptDocument::new_building_upgrade_template();
                template_script.id = ScriptDocument::generate_unique_id();
                template_script.name = name;
                template_script.target_scope = scope;
                template_script.kill_switch_key = kill_switch_key;
                template_script.is_enabled = false;
                template_script.touch_updated_timestamp();
                template_script
            }
            NewScriptTemplateChoice::VisualBlocks | NewScriptTemplateChoice::AdvancedRust => {
                let mut blank_script = ScriptDocument::new_blank(
                    name,
                    scope,
                    script_mode_for_template(template_choice),
                );
                blank_script.kill_switch_key = kill_switch_key;
                blank_script
            }
        };

        let new_script_id = new_script.id.clone();
        let new_script_name = new_script.name.clone();
        let can_persist_now =
            inspect_script_validation_with_custom_blocks(&new_script, &self.custom_blocks)
                .is_valid();

        if can_persist_now
            && self
                .repository
                .save_script_with_custom_blocks(&new_script, &self.custom_blocks)
                .is_ok()
        {
            let _ = self
                .coordinator
                .send_command(CoordinatorCommand::UpsertScript(new_script.clone()));
            self.show_toast(format!("Created '{new_script_name}'."), false);
        } else {
            new_script.is_enabled = false;
            self.mark_script_dirty(&new_script_id);
        }

        self.scripts.push(new_script);
        self.selected_script_id = Some(new_script_id);
        self.switch_workbench_tab(WorkbenchTab::Editor);
        self.persist_settings_quietly();
    }

    fn sync_target_change_for_active_script(&mut self, active_index: usize, toast_message: String) {
        let script_id = self.scripts[active_index].id.clone();
        if self.active_tab == WorkbenchTab::Active
            && inspect_script_validation_with_custom_blocks(
                &self.scripts[active_index],
                &self.custom_blocks,
            )
            .is_valid()
        {
            self.scripts[active_index].touch_updated_timestamp();
            let updated_script = self.scripts[active_index].clone();
            if self
                .repository
                .save_script_with_custom_blocks(&updated_script, &self.custom_blocks)
                .is_ok()
            {
                self.dirty_script_ids.remove(&script_id);
            } else {
                self.mark_script_dirty(&script_id);
            }
            let _ = self
                .coordinator
                .send_command(CoordinatorCommand::UpsertScript(updated_script));
        } else {
            self.mark_script_dirty(&script_id);
        }
        self.show_toast(toast_message, false);
    }

    fn duplicate_script_by_id(&mut self, script_id: &str) {
        let Some(source_script) = self
            .scripts
            .iter()
            .find(|item| item.id == script_id)
            .cloned()
        else {
            return;
        };
        let mut duplicated_script = source_script;
        duplicated_script.id = ScriptDocument::generate_unique_id();
        duplicated_script.name = format!("{} (Copy)", duplicated_script.name);
        duplicated_script.is_enabled = false;
        duplicated_script.touch_updated_timestamp();
        let new_script_id = duplicated_script.id.clone();

        if self
            .repository
            .save_script_with_custom_blocks(&duplicated_script, &self.custom_blocks)
            .is_ok()
        {
            let _ = self
                .coordinator
                .send_command(CoordinatorCommand::UpsertScript(duplicated_script.clone()));
        } else {
            self.mark_script_dirty(&new_script_id);
        }

        self.scripts.push(duplicated_script);
        self.selected_script_id = Some(new_script_id);
        self.persist_settings_quietly();
    }

    fn delete_script_by_id(&mut self, script_id: &str) {
        let Some(position) = self.scripts.iter().position(|item| item.id == script_id) else {
            return;
        };
        let mut removed_script = self.scripts.remove(position);
        removed_script.is_enabled = false;
        self.dirty_script_ids.remove(script_id);
        let _ = self.repository.delete_script(script_id);
        let _ = self
            .coordinator
            .send_command(CoordinatorCommand::RemoveScript {
                script_id: script_id.to_string(),
            });

        if self.selected_script_id.as_deref() == Some(script_id) {
            self.selected_script_id = self.scripts.first().map(|script| script.id.clone());
        }
        self.persist_settings_quietly();

        self.status_toast = None;
        self.last_deleted_script = Some(DeletedScriptUndoEntry {
            script: removed_script,
            original_index: position,
            expires_at: Instant::now() + Duration::from_millis(DELETE_UNDO_TOAST_DURATION_MS),
        });
    }
}
