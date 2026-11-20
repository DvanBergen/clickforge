mod toasts;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use clickforge_sdk::KeyCode;
use egui::{
    CentralPanel, CornerRadius, Event, Frame, Margin, Panel, RichText, Stroke, Ui, ViewportCommand,
};
use tracing::warn;

use crate::engine::CoordinatorHandle;
use crate::linux::WindowWatcher;
use crate::models::{
    CompileResultSnapshot, CoordinatorCommand, CoordinatorSnapshot, CustomBlockDefinition,
    ScriptDocument, ScriptExecutionPhase, ScriptMode, TrayUiEvent,
    inspect_script_validation_with_custom_blocks,
};
use crate::storage::{AppSettings, CustomBlockRepository, ScriptRepository, SettingsStore};
use crate::tray::SystemTrayHandle;
use crate::ui::active_dashboard::{
    ActiveDashboardViewModel, render_active_dashboard, render_uinput_offline_banner,
};
use crate::ui::app_actions::VisualProgramHistory;
use crate::ui::block_canvas::{BlockCanvasAction, render_block_canvas};
use crate::ui::block_palette::{
    BlockDragState, PALETTE_DEFAULT_WIDTH_PX, PALETTE_MAX_WIDTH_PX, PALETTE_MIN_WIDTH_PX,
    render_block_palette,
};
use crate::ui::custom_block_modal::{CustomBlockModalState, render_custom_block_modal};
use crate::ui::rust_editor_panel::render_rust_editor_panel;
use crate::ui::script_settings_bar::{KeyCaptureTarget, render_script_settings_bar};
use crate::ui::sidebar::{
    NewScriptModalState, SIDEBAR_DEFAULT_WIDTH_PX, SIDEBAR_MAX_WIDTH_PX, SIDEBAR_MIN_WIDTH_PX,
    SidebarViewModel, render_new_script_modal, render_sidebar,
};
use crate::ui::target_picker_modal::{TargetPickerState, render_target_picker_modal};
use crate::ui::telemetry_drawer::{GaussianHistogramCache, render_telemetry_drawer};
use crate::ui::theme::{
    BG_CANVAS, BORDER_SUBTLE, CORNER_RADIUS_MD_PX, FONT_SIZE_BODY_PX, FONT_SIZE_HEADING_PX,
    SURFACE_ELEVATED, SURFACE_PANEL, TEXT_MUTED, TEXT_PRIMARY, build_panel_frame,
    map_egui_key_to_sdk_key, rich_bold,
};

pub(crate) use toasts::{DeletedScriptUndoEntry, StatusToast};

const ACTIVE_REPAINT_INTERVAL_MS: u64 = 50;
const IDLE_REPAINT_INTERVAL_MS: u64 = 250;
const HIDDEN_REPAINT_INTERVAL_MS: u64 = 250;
const WAYLAND_DISPLAY_ENV_VAR: &str = "WAYLAND_DISPLAY";

/// Startup dependency bundle passed from `main.rs` into `ClickForgeWorkbenchApp`.
pub struct WorkbenchLaunchBundle {
    pub scripts: Vec<ScriptDocument>,
    pub custom_blocks: Vec<CustomBlockDefinition>,
    pub repository: ScriptRepository,
    pub custom_block_repository: CustomBlockRepository,
    pub settings_store: SettingsStore,
    pub settings: AppSettings,
    pub coordinator: CoordinatorHandle,
    pub window_watcher: Arc<WindowWatcher>,
    pub tray_handle: Option<Arc<SystemTrayHandle>>,
    pub tray_event_receiver: Receiver<TrayUiEvent>,
    pub shutdown_flag: Arc<AtomicBool>,
}

/// Top-level workspace tab switching between the live control center and script editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkbenchTab {
    #[default]
    Active,
    Editor,
}

/// Top-level native `eframe` application hosting the ClickForge Desktop Workbench.
pub struct ClickForgeWorkbenchApp {
    pub(crate) active_tab: WorkbenchTab,
    pub(crate) scripts: Vec<ScriptDocument>,
    pub(crate) custom_blocks: Vec<CustomBlockDefinition>,
    pub(crate) selected_script_id: Option<String>,
    pub(crate) dirty_script_ids: HashSet<String>,
    pub(crate) repository: ScriptRepository,
    pub(crate) custom_block_repository: CustomBlockRepository,
    pub(crate) settings_store: SettingsStore,
    pub(crate) settings: AppSettings,
    pub(crate) coordinator: CoordinatorHandle,
    pub(crate) window_watcher: Arc<WindowWatcher>,
    tray_handle: Option<Arc<SystemTrayHandle>>,
    tray_event_receiver: Receiver<TrayUiEvent>,
    shutdown_flag: Arc<AtomicBool>,
    latest_snapshot: CoordinatorSnapshot,
    pub(crate) active_key_capture: Option<KeyCaptureTarget>,
    pub(crate) target_picker_state: TargetPickerState,
    pub(crate) new_script_modal_state: NewScriptModalState,
    pub(crate) custom_block_modal_state: CustomBlockModalState,
    block_drag_state: BlockDragState,
    histogram_cache: GaussianHistogramCache,
    pub(crate) visual_history_by_script_id: HashMap<String, VisualProgramHistory>,
    pub(crate) status_toast: Option<StatusToast>,
    pub(crate) last_deleted_script: Option<DeletedScriptUndoEntry>,
    is_window_hidden_to_tray: bool,
    should_allow_window_close: bool,
    did_apply_initial_visibility: bool,
}

impl ClickForgeWorkbenchApp {
    /// Constructs the workbench app state from initialized runtime handles and persisted scripts.
    pub fn new(bundle: WorkbenchLaunchBundle) -> Self {
        let initial_selected_id = resolve_initial_selected_script_id(
            &bundle.scripts,
            bundle.settings.last_selected_script_id.as_deref(),
        );
        let initial_snapshot = bundle.coordinator.snapshot();

        Self {
            active_tab: WorkbenchTab::Active,
            scripts: bundle.scripts,
            custom_blocks: bundle.custom_blocks,
            selected_script_id: initial_selected_id,
            dirty_script_ids: HashSet::new(),
            repository: bundle.repository,
            custom_block_repository: bundle.custom_block_repository,
            settings_store: bundle.settings_store,
            settings: bundle.settings,
            coordinator: bundle.coordinator,
            window_watcher: bundle.window_watcher,
            tray_handle: bundle.tray_handle,
            tray_event_receiver: bundle.tray_event_receiver,
            shutdown_flag: bundle.shutdown_flag,
            latest_snapshot: initial_snapshot,
            active_key_capture: None,
            target_picker_state: TargetPickerState::default(),
            new_script_modal_state: NewScriptModalState::default(),
            custom_block_modal_state: CustomBlockModalState::default(),
            block_drag_state: BlockDragState::default(),
            histogram_cache: GaussianHistogramCache::default(),
            visual_history_by_script_id: HashMap::new(),
            status_toast: None,
            last_deleted_script: None,
            is_window_hidden_to_tray: false,
            should_allow_window_close: false,
            did_apply_initial_visibility: false,
        }
    }

    pub(crate) fn active_script_index(&self) -> Option<usize> {
        let selected_id = self.selected_script_id.as_deref()?;
        self.scripts
            .iter()
            .position(|script| script.id == selected_id)
    }

    pub(crate) fn mark_script_dirty(&mut self, script_id: &str) {
        self.dirty_script_ids.insert(script_id.to_string());
    }

    pub(crate) fn persist_settings_quietly(&self) {
        let mut settings_snapshot = self.settings.clone();
        settings_snapshot.last_selected_script_id = self.selected_script_id.clone();
        if let Err(save_error) = self.settings_store.save(&settings_snapshot) {
            warn!("Failed to persist settings.json: {save_error}");
        }
    }

    pub(crate) fn request_clean_quit(&mut self, egui_context: &egui::Context) {
        self.should_allow_window_close = true;
        self.shutdown_flag.store(true, Ordering::SeqCst);
        if self.is_window_hidden_to_tray {
            self.is_window_hidden_to_tray = false;
            egui_context.send_viewport_cmd(ViewportCommand::Visible(true));
        }
        egui_context.send_viewport_cmd(ViewportCommand::Close);
    }

    fn handle_tray_events(&mut self, egui_context: &egui::Context) {
        while let Ok(tray_event) = self.tray_event_receiver.try_recv() {
            match tray_event {
                TrayUiEvent::ShowMainWindow => {
                    self.is_window_hidden_to_tray = false;
                    egui_context.send_viewport_cmd(ViewportCommand::Visible(true));
                    egui_context.send_viewport_cmd(ViewportCommand::Focus);
                }
                TrayUiEvent::ToggleMainWindow => {
                    if is_wayland_display_session() {
                        self.is_window_hidden_to_tray = false;
                        egui_context.send_viewport_cmd(ViewportCommand::Visible(true));
                        egui_context.send_viewport_cmd(ViewportCommand::Focus);
                        continue;
                    }
                    self.is_window_hidden_to_tray = !self.is_window_hidden_to_tray;
                    egui_context.send_viewport_cmd(ViewportCommand::Visible(
                        !self.is_window_hidden_to_tray,
                    ));
                    if !self.is_window_hidden_to_tray {
                        egui_context.send_viewport_cmd(ViewportCommand::Focus);
                    }
                }
                TrayUiEvent::EmergencyStopAll => {
                    let _ = self
                        .coordinator
                        .send_command(CoordinatorCommand::EmergencyStopAll {
                            reason: "Tray Emergency Stop".to_string(),
                        });
                }
                TrayUiEvent::DisableAllScripts => {
                    self.disable_all_scripts();
                }
                TrayUiEvent::QuitApplication => {
                    self.request_clean_quit(egui_context);
                }
            }
        }
    }

    fn handle_close_request(&mut self, egui_context: &egui::Context) {
        let is_close_requested =
            egui_context.input(|input_state| input_state.viewport().close_requested());
        if !is_close_requested || self.should_allow_window_close {
            return;
        }
        if should_intercept_close_to_tray(
            is_wayland_display_session(),
            self.settings.minimize_to_tray_on_close,
            self.tray_handle.is_some(),
        ) {
            egui_context.send_viewport_cmd(ViewportCommand::CancelClose);
            egui_context.send_viewport_cmd(ViewportCommand::Visible(false));
            self.is_window_hidden_to_tray = true;
        }
    }

    fn schedule_repaint_pacing(&self, egui_context: &egui::Context) {
        let has_active_execution = self.latest_snapshot.script_statuses.iter().any(|status| {
            matches!(
                status.phase,
                ScriptExecutionPhase::Running | ScriptExecutionPhase::Compiling
            )
        });
        let repaint_interval_ms = if self.is_window_hidden_to_tray {
            HIDDEN_REPAINT_INTERVAL_MS
        } else if has_active_execution
            || self.active_key_capture.is_some()
            || self.block_drag_state.active_drag.is_some()
        {
            ACTIVE_REPAINT_INTERVAL_MS
        } else {
            IDLE_REPAINT_INTERVAL_MS
        };
        egui_context.request_repaint_after(Duration::from_millis(repaint_interval_ms));
    }

    fn process_active_key_capture(&mut self, egui_context: &egui::Context) {
        let Some(capture_target) = self.active_key_capture else {
            return;
        };
        let captured_from_evdev = self.coordinator.take_captured_key_code();
        let captured_from_egui = detect_egui_key_press(egui_context);
        let Some(captured_key) = captured_from_evdev.or(captured_from_egui) else {
            return;
        };

        self.active_key_capture = None;
        self.coordinator.cancel_key_capture();

        if captured_key == KeyCode::Escape {
            self.show_toast("Key capture cancelled.", false);
            return;
        }

        let Some(active_index) = self.active_script_index() else {
            return;
        };
        let script = &mut self.scripts[active_index];
        let script_id = script.id.clone();
        let toast_message = match capture_target {
            KeyCaptureTarget::KillSwitch => {
                script.kill_switch_key = Some(captured_key);
                format!(
                    "Hard Kill-Switch bound to {}.",
                    captured_key.display_label()
                )
            }
            KeyCaptureTarget::ArmToggle => {
                script.arm_toggle_key = Some(captured_key);
                format!("Arm Toggle bound to {}.", captured_key.display_label())
            }
        };
        self.mark_script_dirty(&script_id);
        self.show_toast(toast_message, false);
    }

    fn latest_compile_result_for_script(&self, script_id: &str) -> Option<CompileResultSnapshot> {
        if let Some(specific_result) = self.coordinator.last_compile_result(script_id) {
            return Some(specific_result);
        }
        self.latest_snapshot
            .last_compile_result
            .as_ref()
            .filter(|result| result.script_id == script_id)
            .cloned()
    }

    fn render_active_workspace(&mut self, ui: &mut Ui) {
        let mut dashboard_actions = Vec::new();
        CentralPanel::default()
            .frame(build_panel_frame(BG_CANVAS, 12, 10))
            .show(ui, |ui| {
                let dashboard_view = ActiveDashboardViewModel {
                    scripts: &self.scripts,
                    custom_blocks: &self.custom_blocks,
                    selected_script_id: self.selected_script_id.as_deref(),
                    dirty_script_ids: &self.dirty_script_ids,
                    snapshot: &self.latest_snapshot,
                };
                render_active_dashboard(ui, &dashboard_view, &mut dashboard_actions);
            });
        self.apply_active_dashboard_actions(dashboard_actions);
    }

    fn render_editor_workspace(&mut self, ui: &mut Ui, egui_context: &egui::Context) {
        let mut settings_bar_actions = Vec::new();
        let active_index = self.active_script_index();
        let is_active_dirty = self
            .selected_script_id
            .as_ref()
            .is_some_and(|script_id| self.dirty_script_ids.contains(script_id));

        Panel::top("script_settings_top_bar")
            .resizable(false)
            .frame(build_panel_frame(SURFACE_PANEL, 14, 10))
            .show(ui, |ui| {
                let active_script_mut = active_index.map(|index| &mut self.scripts[index]);
                render_script_settings_bar(
                    ui,
                    active_script_mut,
                    &self.custom_blocks,
                    is_active_dirty,
                    self.active_key_capture,
                    &self.latest_snapshot,
                    &mut settings_bar_actions,
                );
            });
        self.apply_settings_bar_actions(settings_bar_actions);

        let active_index_after_chrome = self.active_script_index();
        let active_mode = active_index_after_chrome.map(|index| self.scripts[index].mode);

        if active_mode == Some(ScriptMode::VisualBlocks) {
            let mut palette_actions = Vec::new();
            Panel::left("block_palette_panel")
                .resizable(true)
                .default_size(PALETTE_DEFAULT_WIDTH_PX)
                .min_size(PALETTE_MIN_WIDTH_PX)
                .max_size(PALETTE_MAX_WIDTH_PX)
                .frame(build_panel_frame(SURFACE_PANEL, 10, 10))
                .show(ui, |ui| {
                    render_block_palette(
                        ui,
                        &self.custom_blocks,
                        &mut self.block_drag_state,
                        &mut palette_actions,
                    );
                });
            self.apply_palette_actions(palette_actions);
        }

        let mut canvas_actions = Vec::new();
        let mut rust_editor_actions = Vec::new();
        let active_compile_result = active_index_after_chrome
            .and_then(|index| self.latest_compile_result_for_script(&self.scripts[index].id));
        let pre_canvas_snapshot = active_index_after_chrome.and_then(|index| {
            (self.scripts[index].mode == ScriptMode::VisualBlocks).then(|| {
                (
                    self.scripts[index].id.clone(),
                    self.scripts[index].visual_program.clone(),
                )
            })
        });

        CentralPanel::default()
            .frame(build_panel_frame(BG_CANVAS, 12, 10))
            .show(ui, |ui| {
                let Some(script_index) = active_index_after_chrome else {
                    render_empty_workbench_center(ui);
                    return;
                };
                let script_id = self.scripts[script_index].id.clone();
                let runtime_status = self.latest_snapshot.status_for_script(&script_id);
                let active_block_id = runtime_status.and_then(|status| status.active_block_id);
                let runtime_phase = runtime_status
                    .map(|status| status.phase)
                    .unwrap_or(ScriptExecutionPhase::Disabled);
                let validation_report = inspect_script_validation_with_custom_blocks(
                    &self.scripts[script_index],
                    &self.custom_blocks,
                );

                match self.scripts[script_index].mode {
                    ScriptMode::VisualBlocks => render_block_canvas(
                        ui,
                        &mut self.scripts[script_index],
                        active_block_id,
                        &validation_report,
                        &self.custom_blocks,
                        &mut self.block_drag_state,
                        &mut canvas_actions,
                    ),
                    ScriptMode::AdvancedRust => render_rust_editor_panel(
                        ui,
                        &mut self.scripts[script_index],
                        &validation_report,
                        runtime_phase,
                        active_compile_result.as_ref(),
                        &mut rust_editor_actions,
                    ),
                }
            });

        let did_canvas_mutate = canvas_actions
            .iter()
            .any(|action| matches!(action, BlockCanvasAction::MarkDirty));
        if did_canvas_mutate
            && let Some((script_id, before_program)) = pre_canvas_snapshot
            && let Some(active_script_index) = self.active_script_index()
            && self.scripts[active_script_index].id == script_id
            && self.scripts[active_script_index].visual_program != before_program
        {
            let is_pointer_down = egui_context.input(|input| input.pointer.any_down());
            let current_block_count = self.scripts[active_script_index]
                .visual_program
                .total_block_count();
            self.record_visual_program_undo_snapshot(
                &script_id,
                before_program,
                current_block_count,
                is_pointer_down,
            );
        }

        self.apply_canvas_actions(canvas_actions);
        self.apply_rust_editor_actions(rust_editor_actions);
    }
}

impl eframe::App for ClickForgeWorkbenchApp {
    fn logic(&mut self, egui_context: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.did_apply_initial_visibility {
            self.did_apply_initial_visibility = true;
            if !is_wayland_display_session()
                && self.settings.start_minimized_to_tray
                && self.tray_handle.is_some()
            {
                egui_context.send_viewport_cmd(ViewportCommand::Visible(false));
                self.is_window_hidden_to_tray = true;
            }
        }

        if self.shutdown_flag.load(Ordering::Relaxed) && !self.should_allow_window_close {
            self.request_clean_quit(egui_context);
        }

        self.handle_tray_events(egui_context);
        self.handle_close_request(egui_context);
        self.latest_snapshot = self.coordinator.snapshot();
        self.schedule_repaint_pacing(egui_context);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let egui_context = ui.ctx().clone();
        self.process_active_key_capture(&egui_context);
        self.process_keyboard_shortcuts(&egui_context);

        let mut sidebar_actions = Vec::new();
        Panel::left("scripts_sidebar_panel")
            .resizable(true)
            .default_size(SIDEBAR_DEFAULT_WIDTH_PX)
            .min_size(SIDEBAR_MIN_WIDTH_PX)
            .max_size(SIDEBAR_MAX_WIDTH_PX)
            .frame(build_panel_frame(SURFACE_PANEL, 10, 10))
            .show(ui, |ui| {
                let sidebar_view = SidebarViewModel {
                    active_tab: self.active_tab,
                    scripts: &self.scripts,
                    custom_blocks: &self.custom_blocks,
                    selected_script_id: self.selected_script_id.as_deref(),
                    dirty_script_ids: &self.dirty_script_ids,
                    snapshot: &self.latest_snapshot,
                    settings: &self.settings,
                    has_system_tray: self.tray_handle.is_some(),
                    is_wayland_session: is_wayland_display_session(),
                };
                render_sidebar(ui, &sidebar_view, &mut sidebar_actions);
            });
        self.apply_sidebar_actions(&egui_context, sidebar_actions);

        if !self.latest_snapshot.input_status.is_uinput_ready {
            Panel::top("uinput_offline_banner_panel")
                .resizable(false)
                .frame(build_panel_frame(SURFACE_PANEL, 14, 8))
                .show(ui, |ui| {
                    render_uinput_offline_banner(ui, &self.latest_snapshot.input_status);
                });
        }

        let active_index_for_drawer = self.active_script_index();
        let mut did_toggle_drawer = false;
        let drawer_response = Panel::bottom("telemetry_bottom_drawer")
            .resizable(false)
            .frame(build_panel_frame(SURFACE_PANEL, 12, 6))
            .show(ui, |ui| {
                let active_script_ref = active_index_for_drawer.map(|index| &self.scripts[index]);
                did_toggle_drawer = render_telemetry_drawer(
                    ui,
                    &mut self.settings.show_telemetry_drawer,
                    self.settings.telemetry_drawer_height_px,
                    active_script_ref,
                    &self.latest_snapshot,
                    &mut self.histogram_cache,
                );
            });
        let rendered_drawer_height_px = drawer_response.response.rect.height();
        if did_toggle_drawer {
            self.persist_settings_quietly();
        }

        match self.active_tab {
            WorkbenchTab::Active => self.render_active_workspace(ui),
            WorkbenchTab::Editor => self.render_editor_workspace(ui, &egui_context),
        }

        let mut picker_actions = Vec::new();
        let (open_windows, running_processes) = if self.target_picker_state.is_open {
            (
                self.window_watcher.snapshot_open_windows(),
                self.window_watcher.snapshot_running_processes(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        render_target_picker_modal(
            &egui_context,
            &mut self.target_picker_state,
            &open_windows,
            &running_processes,
            &mut picker_actions,
        );
        self.apply_target_picker_actions(picker_actions);

        let mut modal_sidebar_actions = Vec::new();
        render_new_script_modal(
            &egui_context,
            &mut self.new_script_modal_state,
            &mut modal_sidebar_actions,
        );
        self.apply_sidebar_actions(&egui_context, modal_sidebar_actions);

        let mut custom_block_modal_actions = Vec::new();
        render_custom_block_modal(
            &egui_context,
            &mut self.custom_block_modal_state,
            &self.custom_blocks,
            &mut custom_block_modal_actions,
        );
        self.apply_custom_block_modal_actions(custom_block_modal_actions);

        self.render_toasts(&egui_context, rendered_drawer_height_px);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist_settings_quietly();
        self.shutdown_flag.store(true, Ordering::SeqCst);
        self.coordinator.shutdown();
        if let Some(tray_handle) = &self.tray_handle {
            tray_handle.shutdown();
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        BG_CANVAS.to_normalized_gamma_f32()
    }
}

fn resolve_initial_selected_script_id(
    scripts: &[ScriptDocument],
    preferred_id: Option<&str>,
) -> Option<String> {
    if let Some(saved_id) = preferred_id
        && scripts.iter().any(|script| script.id == saved_id)
    {
        return Some(saved_id.to_string());
    }
    scripts.first().map(|script| script.id.clone())
}

fn detect_egui_key_press(egui_context: &egui::Context) -> Option<KeyCode> {
    egui_context.input(|input_state| {
        for event in &input_state.events {
            let Event::Key {
                key,
                physical_key,
                pressed: true,
                repeat: false,
                ..
            } = event
            else {
                continue;
            };
            let resolved_key = physical_key.unwrap_or(*key);
            if let Some(sdk_key) = map_egui_key_to_sdk_key(resolved_key) {
                return Some(sdk_key);
            }
        }
        None
    })
}

fn render_empty_workbench_center(ui: &mut Ui) {
    ui.centered_and_justified(|ui| {
        Frame::NONE
            .fill(SURFACE_ELEVATED)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
            .inner_margin(Margin::same(28))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(rich_bold(
                        "No Script Selected",
                        FONT_SIZE_HEADING_PX,
                        TEXT_PRIMARY,
                    ));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(
                            "Select an existing script or click + New Script in the left rail.",
                        )
                        .size(FONT_SIZE_BODY_PX)
                        .color(TEXT_MUTED),
                    );
                });
            });
    });
}

/// Returns `true` when the active session runs on a Wayland display server.
pub(crate) fn is_wayland_display_session() -> bool {
    std::env::var_os(WAYLAND_DISPLAY_ENV_VAR).is_some_and(|display_name| !display_name.is_empty())
}

fn should_intercept_close_to_tray(
    is_wayland_session: bool,
    minimize_to_tray_on_close: bool,
    has_system_tray: bool,
) -> bool {
    !is_wayland_session && minimize_to_tray_on_close && has_system_tray
}

#[cfg(test)]
mod tests {
    use super::should_intercept_close_to_tray;

    #[test]
    fn wayland_never_intercepts_window_close_even_when_minimize_setting_is_true() {
        assert!(!should_intercept_close_to_tray(true, true, true));
        assert!(!should_intercept_close_to_tray(true, false, true));
        assert!(!should_intercept_close_to_tray(true, true, false));
    }

    #[test]
    fn x11_intercepts_window_close_only_when_tray_and_setting_are_enabled() {
        assert!(should_intercept_close_to_tray(false, true, true));
        assert!(!should_intercept_close_to_tray(false, false, true));
        assert!(!should_intercept_close_to_tray(false, true, false));
    }
}
