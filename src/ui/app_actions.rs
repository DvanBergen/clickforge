use crate::engine::generate_rust_source_from_script;
use crate::models::{
    CoordinatorCommand, GameTargetRule, ScriptDocument, ScriptMode, TargetScope,
    inspect_script_validation,
};
use crate::ui::app::ClickForgeWorkbenchApp;
use crate::ui::block_canvas::BlockCanvasAction;
use crate::ui::block_palette::PaletteAction;
use crate::ui::rust_editor_panel::RustEditorAction;
use crate::ui::script_settings_bar::SettingsBarAction;
use crate::ui::sidebar::SidebarAction;
use crate::ui::target_picker_modal::TargetPickerAction;

impl ClickForgeWorkbenchApp {
    pub(crate) fn apply_sidebar_actions(&mut self, actions: Vec<SidebarAction>) {
        for action in actions {
            match action {
                SidebarAction::SelectScript(script_id) => {
                    self.selected_script_id = Some(script_id);
                    self.active_key_capture = None;
                    self.coordinator.cancel_key_capture();
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
                SidebarAction::CreateBlankScript {
                    name,
                    is_selected_game_scope,
                    mode,
                } => {
                    self.create_blank_script(name, is_selected_game_scope, mode);
                }
                SidebarAction::CreateFromBuildingUpgradeTemplate => {
                    self.create_building_upgrade_template_copy();
                }
                SidebarAction::SetMinimizeToTray(is_minimizing) => {
                    self.settings.minimize_to_tray_on_close = is_minimizing;
                    self.persist_settings_quietly();
                }
                SidebarAction::ToggleTelemetryDrawer => {
                    self.settings.show_telemetry_drawer = !self.settings.show_telemetry_drawer;
                    self.persist_settings_quietly();
                }
            }
        }
    }

    pub(crate) fn apply_settings_bar_actions(&mut self, actions: Vec<SettingsBarAction>) {
        for action in actions {
            match action {
                SettingsBarAction::OpenTargetPicker => {
                    self.window_watcher.refresh_now();
                    let _ = self
                        .coordinator
                        .send_command(CoordinatorCommand::RefreshWindowsAndProcesses);
                    self.target_picker_state.open();
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
                        let script_id = self.scripts[active_index].id.clone();
                        self.mark_script_dirty(&script_id);
                        self.show_toast(format!("Attached target: {target_name}"), false);
                    }
                }
                TargetPickerAction::SetScopeGlobal => {
                    if let Some(active_index) = self.active_script_index() {
                        self.scripts[active_index].target_scope = TargetScope::Global;
                        let script_id = self.scripts[active_index].id.clone();
                        self.mark_script_dirty(&script_id);
                        self.show_toast("Switched script scope to Global.", false);
                    }
                }
            }
        }
    }

    pub(crate) fn apply_palette_actions(&mut self, actions: Vec<PaletteAction>) {
        let Some(active_index) = self.active_script_index() else {
            return;
        };
        let mut did_modify = false;
        for action in actions {
            match action {
                PaletteAction::ApplyTrigger(new_trigger) => {
                    self.scripts[active_index].visual_program.trigger = new_trigger;
                    did_modify = true;
                }
                PaletteAction::AppendStatement(statement_kind) => {
                    self.scripts[active_index]
                        .visual_program
                        .append_root_block(statement_kind);
                    did_modify = true;
                }
            }
        }
        if did_modify {
            let script_id = self.scripts[active_index].id.clone();
            self.mark_script_dirty(&script_id);
        }
    }

    pub(crate) fn apply_canvas_actions(&mut self, actions: Vec<BlockCanvasAction>) {
        let Some(active_index) = self.active_script_index() else {
            return;
        };
        for action in actions {
            match action {
                BlockCanvasAction::MarkDirty => {
                    let script_id = self.scripts[active_index].id.clone();
                    self.mark_script_dirty(&script_id);
                }
                BlockCanvasAction::ExportToRust => {
                    let generated_rust =
                        generate_rust_source_from_script(&self.scripts[active_index]);
                    self.scripts[active_index].rust_source = generated_rust;
                    self.scripts[active_index].mode = ScriptMode::AdvancedRust;
                    let script_id = self.scripts[active_index].id.clone();
                    self.mark_script_dirty(&script_id);
                    self.show_toast(
                        "Exported visual blocks to Rust and switched to Advanced Rust mode.",
                        false,
                    );
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
                    let validation_report = inspect_script_validation(&script_snapshot);
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

    fn save_active_script(&mut self) {
        let Some(active_index) = self.active_script_index() else {
            return;
        };
        self.scripts[active_index].touch_updated_timestamp();
        let script_snapshot = self.scripts[active_index].clone();

        match self.repository.save_script(&script_snapshot) {
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

    fn set_script_enabled_by_id(&mut self, script_id: &str, is_enabled: bool) {
        let Some(script_index) = self.scripts.iter().position(|item| item.id == script_id) else {
            return;
        };
        if is_enabled {
            let validation_report = inspect_script_validation(&self.scripts[script_index]);
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

        if self.repository.save_script(&updated_script).is_ok() {
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

    fn create_blank_script(
        &mut self,
        name: String,
        is_selected_game_scope: bool,
        mode: ScriptMode,
    ) {
        let scope = if is_selected_game_scope {
            TargetScope::SelectedGame(GameTargetRule::default())
        } else {
            TargetScope::Global
        };
        let new_script = ScriptDocument::new_blank(name, scope, mode);
        let new_script_id = new_script.id.clone();
        self.scripts.push(new_script);
        self.selected_script_id = Some(new_script_id.clone());
        self.mark_script_dirty(&new_script_id);
        self.persist_settings_quietly();

        if is_selected_game_scope {
            self.window_watcher.refresh_now();
            let _ = self
                .coordinator
                .send_command(CoordinatorCommand::RefreshWindowsAndProcesses);
            self.target_picker_state.open();
        }
    }

    fn create_building_upgrade_template_copy(&mut self) {
        let mut template_script = ScriptDocument::new_building_upgrade_template();
        template_script.id = ScriptDocument::generate_unique_id();
        let next_index = self.scripts.len().saturating_add(1);
        if self
            .scripts
            .iter()
            .any(|existing| existing.name == template_script.name)
        {
            template_script.name = format!("Building Upgrade Rapid Clicker ({next_index})");
        }
        let script_id = template_script.id.clone();

        if let Err(save_error) = self.repository.save_script(&template_script) {
            self.show_toast(format!("Failed to save template: {save_error}"), true);
            return;
        }
        let _ = self
            .coordinator
            .send_command(CoordinatorCommand::UpsertScript(template_script.clone()));
        self.scripts.push(template_script);
        self.selected_script_id = Some(script_id);
        self.persist_settings_quietly();
        self.show_toast("Created Building Upgrade Rapid Clicker script.", false);
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

        if self.repository.save_script(&duplicated_script).is_ok() {
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
        let removed_script = self.scripts.remove(position);
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
        self.show_toast(format!("Deleted '{}'.", removed_script.name), false);
    }
}
