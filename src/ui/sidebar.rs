use std::collections::HashSet;

use egui::{
    Align, Button, Checkbox, Color32, CornerRadius, Frame, Id, Layout, Margin, Modal, RichText,
    ScrollArea, Sense, Stroke, TextEdit, Ui, vec2,
};

use crate::models::{
    CoordinatorSnapshot, ScriptDocument, ScriptExecutionPhase, ScriptMode, TargetScope,
    inspect_script_validation,
};
use crate::storage::AppSettings;
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BORDER_STRONG, BORDER_SUBTLE, CORNER_RADIUS_MD_PX,
    CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX, FONT_SIZE_HEADING_PX, FONT_SIZE_SMALL_PX,
    INK_ON_ACCENT, STATUS_DANGER, STATUS_DANGER_TINT, SURFACE_ELEVATED, SURFACE_HOVER,
    SURFACE_PANEL, TEXT_MUTED, TEXT_PRIMARY, paint_status_dot, phase_status_color,
    render_badge_pill, truncate_display_text,
};

/// Default sidebar width in logical pixels.
pub const SIDEBAR_DEFAULT_WIDTH_PX: f32 = 240.0;

/// Minimum sidebar width in logical pixels.
pub const SIDEBAR_MIN_WIDTH_PX: f32 = 204.0;

/// Maximum sidebar width in logical pixels.
pub const SIDEBAR_MAX_WIDTH_PX: f32 = 340.0;

const STATUS_DOT_RADIUS_PX: f32 = 4.5;
const MAX_SCRIPT_TITLE_CHARS: usize = 20;
const MAX_SUBTITLE_CHARS: usize = 26;
const NEW_SCRIPT_MODAL_WIDTH_PX: f32 = 440.0;

/// Transient form state for the New Script creation modal.
#[derive(Debug, Clone)]
pub struct NewScriptModalState {
    pub is_open: bool,
    pub script_name: String,
    pub is_selected_game_scope: bool,
    pub starting_mode: ScriptMode,
}

impl Default for NewScriptModalState {
    fn default() -> Self {
        Self {
            is_open: false,
            script_name: String::new(),
            is_selected_game_scope: true,
            starting_mode: ScriptMode::VisualBlocks,
        }
    }
}

impl NewScriptModalState {
    /// Resets form fields and opens the New Script modal dialog.
    pub fn open_with_defaults(&mut self, existing_script_count: usize) {
        let next_number = existing_script_count.saturating_add(1);
        self.script_name = format!("New Script {next_number}");
        self.is_selected_game_scope = true;
        self.starting_mode = ScriptMode::VisualBlocks;
        self.is_open = true;
    }
}

/// User actions emitted by the left script sidebar and New Script modal.
#[derive(Debug, Clone)]
pub enum SidebarAction {
    SelectScript(String),
    ToggleScriptEnabled {
        script_id: String,
        is_enabled: bool,
    },
    DuplicateScript(String),
    DeleteScript(String),
    OpenNewScriptModal,
    CreateBlankScript {
        name: String,
        is_selected_game_scope: bool,
        mode: ScriptMode,
    },
    CreateFromBuildingUpgradeTemplate,
    SetMinimizeToTray(bool),
    ToggleTelemetryDrawer,
}

/// Read-only projection passed into `render_sidebar`.
pub struct SidebarViewModel<'frame> {
    pub scripts: &'frame [ScriptDocument],
    pub selected_script_id: Option<&'frame str>,
    pub dirty_script_ids: &'frame HashSet<String>,
    pub snapshot: &'frame CoordinatorSnapshot,
    pub settings: &'frame AppSettings,
    pub has_system_tray: bool,
}

/// Renders the left script navigation sidebar grouped into Selected Game and Global sections.
pub fn render_sidebar(
    ui: &mut Ui,
    view_model: &SidebarViewModel<'_>,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    render_sidebar_header(ui, view_model.snapshot, emitted_actions);
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(6.0);

    let footer_reserved_height_px = 68.0;
    let scroll_height_px = (ui.available_height() - footer_reserved_height_px).max(120.0);

    ScrollArea::vertical()
        .id_salt("sidebar_scripts_scroll")
        .max_height(scroll_height_px)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let selected_game_scripts: Vec<&ScriptDocument> = view_model
                .scripts
                .iter()
                .filter(|script| matches!(script.target_scope, TargetScope::SelectedGame(_)))
                .collect();
            let global_scripts: Vec<&ScriptDocument> = view_model
                .scripts
                .iter()
                .filter(|script| matches!(script.target_scope, TargetScope::Global))
                .collect();

            render_script_group(
                ui,
                "Selected Game",
                &selected_game_scripts,
                view_model,
                emitted_actions,
            );

            ui.add_space(10.0);

            render_script_group(ui, "Global", &global_scripts, view_model, emitted_actions);
        });

    ui.separator();
    ui.add_space(4.0);
    render_sidebar_footer(
        ui,
        view_model.settings,
        view_model.has_system_tray,
        emitted_actions,
    );
}

fn render_sidebar_header(
    ui: &mut Ui,
    snapshot: &CoordinatorSnapshot,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("ClickForge")
                .size(FONT_SIZE_HEADING_PX)
                .strong()
                .color(TEXT_PRIMARY),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let overall_color = if snapshot.is_emergency_stopped {
                STATUS_DANGER
            } else if snapshot
                .script_statuses
                .iter()
                .any(|status| status.phase == ScriptExecutionPhase::Running)
            {
                phase_status_color(ScriptExecutionPhase::Running)
            } else if snapshot
                .script_statuses
                .iter()
                .any(|status| status.phase == ScriptExecutionPhase::Armed)
            {
                ACCENT_PRIMARY
            } else {
                TEXT_MUTED
            };
            paint_status_dot(ui, overall_color, STATUS_DOT_RADIUS_PX);
        });
    });

    ui.add_space(6.0);

    let new_button = Button::new(
        RichText::new("+ New Script")
            .strong()
            .color(INK_ON_ACCENT)
            .size(FONT_SIZE_BODY_PX),
    )
    .fill(ACCENT_PRIMARY)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
    .min_size(vec2(ui.available_width(), 30.0));

    if ui.add(new_button).clicked() {
        emitted_actions.push(SidebarAction::OpenNewScriptModal);
    }
}

fn render_script_group(
    ui: &mut Ui,
    group_title: &str,
    group_scripts: &[&ScriptDocument],
    view_model: &SidebarViewModel<'_>,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(group_title)
                .size(FONT_SIZE_SMALL_PX)
                .strong()
                .color(TEXT_MUTED),
        );
        ui.label(
            RichText::new(format!("({})", group_scripts.len()))
                .size(FONT_SIZE_SMALL_PX)
                .monospace()
                .color(TEXT_MUTED),
        );
    });
    ui.add_space(4.0);

    if group_scripts.is_empty() {
        ui.label(
            RichText::new("No scripts in this group")
                .size(FONT_SIZE_SMALL_PX)
                .color(TEXT_MUTED),
        );
        return;
    }

    for script in group_scripts {
        let is_selected = view_model.selected_script_id == Some(script.id.as_str());
        let is_dirty = view_model.dirty_script_ids.contains(&script.id);
        render_script_row(
            ui,
            script,
            is_selected,
            is_dirty,
            view_model.snapshot,
            emitted_actions,
        );
        ui.add_space(4.0);
    }
}

fn render_script_row(
    ui: &mut Ui,
    script: &ScriptDocument,
    is_selected: bool,
    is_dirty: bool,
    snapshot: &CoordinatorSnapshot,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    let validation_report = inspect_script_validation(script);
    let is_valid = validation_report.is_valid();
    let runtime_phase = snapshot
        .status_for_script(&script.id)
        .map(|status| status.phase)
        .unwrap_or(ScriptExecutionPhase::Disabled);

    let fill_color = if is_selected {
        SURFACE_HOVER
    } else {
        SURFACE_ELEVATED
    };
    let border_color = if is_selected {
        ACCENT_PRIMARY
    } else if !is_valid {
        STATUS_DANGER
    } else {
        BORDER_SUBTLE
    };

    let row_frame = Frame::NONE
        .fill(fill_color)
        .stroke(Stroke::new(1.0, border_color))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(8, 6));

    let frame_response = row_frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let dot_response =
                paint_status_dot(ui, phase_status_color(runtime_phase), STATUS_DOT_RADIUS_PX);
            dot_response.on_hover_text(runtime_phase.display_label());

            let display_name = truncate_display_text(&script.name, MAX_SCRIPT_TITLE_CHARS);
            let title_label = if is_dirty {
                format!("{display_name} *")
            } else {
                display_name
            };
            let title_response = ui.add(
                egui::Label::new(
                    RichText::new(title_label)
                        .size(FONT_SIZE_BODY_PX)
                        .strong()
                        .color(TEXT_PRIMARY),
                )
                .sense(Sense::click()),
            );
            if title_response.clicked() {
                emitted_actions.push(SidebarAction::SelectScript(script.id.clone()));
            }
            if script.name.chars().count() > MAX_SCRIPT_TITLE_CHARS {
                title_response.on_hover_text(&script.name);
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut enabled_flag = script.is_enabled;
                let checkbox_response =
                    ui.add_enabled(is_valid, Checkbox::without_text(&mut enabled_flag));
                if checkbox_response.changed() {
                    emitted_actions.push(SidebarAction::ToggleScriptEnabled {
                        script_id: script.id.clone(),
                        is_enabled: enabled_flag,
                    });
                }
                if !is_valid {
                    let first_reason = validation_report
                        .issues
                        .first()
                        .map(|issue| issue.message.as_str())
                        .unwrap_or("Script failed safety validation.");
                    checkbox_response.on_disabled_hover_text(format!("Blocked: {first_reason}"));
                } else {
                    checkbox_response.on_hover_text("Enable or disable this script in background");
                }
            });
        });

        ui.add_space(2.0);
        render_script_row_metadata(ui, script, is_valid, emitted_actions);
    });

    if frame_response.response.interact(Sense::click()).clicked() {
        emitted_actions.push(SidebarAction::SelectScript(script.id.clone()));
    }
}

fn render_script_row_metadata(
    ui: &mut Ui,
    script: &ScriptDocument,
    is_valid: bool,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    ui.horizontal(|ui| {
        let subtitle = match &script.target_scope {
            TargetScope::Global => script.mode.display_label().to_string(),
            TargetScope::SelectedGame(_) => {
                truncate_display_text(&script.target_scope.display_label(), MAX_SUBTITLE_CHARS)
            }
        };
        ui.label(
            RichText::new(subtitle)
                .size(FONT_SIZE_SMALL_PX)
                .color(TEXT_MUTED),
        );

        if !is_valid {
            render_badge_pill(
                ui,
                "Check",
                STATUS_DANGER_TINT,
                STATUS_DANGER,
                STATUS_DANGER,
            );
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let delete_button = Button::new(RichText::new("Del").size(FONT_SIZE_SMALL_PX))
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX));
            if ui
                .add(delete_button)
                .on_hover_text("Delete script (creates backup first)")
                .clicked()
            {
                emitted_actions.push(SidebarAction::DeleteScript(script.id.clone()));
            }

            let duplicate_button = Button::new(RichText::new("Dup").size(FONT_SIZE_SMALL_PX))
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX));
            if ui
                .add(duplicate_button)
                .on_hover_text("Duplicate script")
                .clicked()
            {
                emitted_actions.push(SidebarAction::DuplicateScript(script.id.clone()));
            }
        });
    });
}

fn render_sidebar_footer(
    ui: &mut Ui,
    settings: &AppSettings,
    has_system_tray: bool,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    let mut minimize_to_tray = settings.minimize_to_tray_on_close;
    let tray_checkbox = ui.add_enabled(
        has_system_tray,
        Checkbox::new(&mut minimize_to_tray, "Minimize to tray on close"),
    );
    if tray_checkbox.changed() {
        emitted_actions.push(SidebarAction::SetMinimizeToTray(minimize_to_tray));
    }
    if !has_system_tray {
        tray_checkbox.on_disabled_hover_text(
            "System tray service (StatusNotifierWatcher) was not detected.",
        );
    }

    ui.add_space(4.0);
    let drawer_label = if settings.show_telemetry_drawer {
        "Hide Telemetry Drawer"
    } else {
        "Show Telemetry Drawer"
    };
    if ui
        .add(
            Button::new(RichText::new(drawer_label).size(FONT_SIZE_SMALL_PX))
                .min_size(vec2(ui.available_width(), 24.0)),
        )
        .clicked()
    {
        emitted_actions.push(SidebarAction::ToggleTelemetryDrawer);
    }
}

/// Renders the modal dialog for creating a new blank script or Building Upgrade template copy.
pub fn render_new_script_modal(
    egui_context: &egui::Context,
    modal_state: &mut NewScriptModalState,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    if !modal_state.is_open {
        return;
    }

    let modal_frame = Frame::NONE
        .fill(SURFACE_PANEL)
        .stroke(Stroke::new(1.0, BORDER_STRONG))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
        .inner_margin(Margin::same(16));

    let modal_response = Modal::new(Id::unique("new_script_modal"))
        .frame(modal_frame)
        .show(egui_context, |ui| {
            ui.set_width(NEW_SCRIPT_MODAL_WIDTH_PX);
            ui.label(
                RichText::new("Create New Script")
                    .size(FONT_SIZE_HEADING_PX)
                    .strong()
                    .color(TEXT_PRIMARY),
            );
            ui.add_space(10.0);

            ui.label(RichText::new("Script Name").size(FONT_SIZE_SMALL_PX).color(TEXT_MUTED));
            ui.add(
                TextEdit::singleline(&mut modal_state.script_name)
                    .desired_width(f32::INFINITY)
                    .hint_text("Enter script name"),
            );
            ui.add_space(10.0);

            render_scope_selector(ui, &mut modal_state.is_selected_game_scope);
            ui.add_space(10.0);

            render_mode_selector(ui, &mut modal_state.starting_mode);
            ui.add_space(10.0);

            ui.label(
                RichText::new(
                    "New blank scripts start without a Hard Kill-Switch key. Bind one before saving or enabling.",
                )
                .size(FONT_SIZE_SMALL_PX)
                .color(TEXT_MUTED),
            );

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Starter Template:")
                        .size(FONT_SIZE_SMALL_PX)
                        .color(TEXT_MUTED),
                );
                let template_button = Button::new(
                    RichText::new("From Building Upgrade Template")
                        .size(FONT_SIZE_SMALL_PX)
                        .color(ACCENT_PRIMARY),
                )
                .fill(ACCENT_PRIMARY_TINT)
                .stroke(Stroke::new(1.0, ACCENT_PRIMARY));
                if ui.add(template_button).clicked() {
                    emitted_actions.push(SidebarAction::CreateFromBuildingUpgradeTemplate);
                    modal_state.is_open = false;
                }
            });

            ui.add_space(12.0);
            render_new_script_footer_buttons(ui, modal_state, emitted_actions);
        });

    if modal_response.should_close() {
        modal_state.is_open = false;
    }
}

fn render_scope_selector(ui: &mut Ui, is_selected_game_scope: &mut bool) {
    ui.label(
        RichText::new("Execution Scope")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    );
    ui.horizontal(|ui| {
        if ui
            .selectable_label(*is_selected_game_scope, "Selected Game Only")
            .clicked()
        {
            *is_selected_game_scope = true;
        }
        if ui
            .selectable_label(!*is_selected_game_scope, "Global (Any Window)")
            .clicked()
        {
            *is_selected_game_scope = false;
        }
    });
}

fn render_mode_selector(ui: &mut Ui, starting_mode: &mut ScriptMode) {
    ui.label(
        RichText::new("Authoring Mode")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    );
    ui.horizontal(|ui| {
        if ui
            .selectable_label(*starting_mode == ScriptMode::VisualBlocks, "Visual Blocks")
            .clicked()
        {
            *starting_mode = ScriptMode::VisualBlocks;
        }
        if ui
            .selectable_label(*starting_mode == ScriptMode::AdvancedRust, "Advanced Rust")
            .clicked()
        {
            *starting_mode = ScriptMode::AdvancedRust;
        }
    });
}

fn render_new_script_footer_buttons(
    ui: &mut Ui,
    modal_state: &mut NewScriptModalState,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    let trimmed_name = modal_state.script_name.trim();
    let can_create = !trimmed_name.is_empty();

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let create_button =
            Button::new(RichText::new("Create Script").strong().color(INK_ON_ACCENT))
                .fill(ACCENT_PRIMARY)
                .stroke(Stroke::NONE);
        if ui.add_enabled(can_create, create_button).clicked() {
            emitted_actions.push(SidebarAction::CreateBlankScript {
                name: trimmed_name.to_string(),
                is_selected_game_scope: modal_state.is_selected_game_scope,
                mode: modal_state.starting_mode,
            });
            modal_state.is_open = false;
        }

        if ui.button("Cancel").clicked() {
            modal_state.is_open = false;
        }
    });
}
