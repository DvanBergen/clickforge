mod status_strips;

use egui::{
    Align, Button, Checkbox, Color32, CornerRadius, FontId, Frame, Id, Layout, Margin, Popup,
    PopupCloseBehavior, RichText, Sense, Stroke, TextEdit, Ui, vec2,
};

use crate::models::{
    CoordinatorSnapshot, ScriptDocument, ScriptMode, TargetScope, ValidationReport,
    inspect_script_validation,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BG_CANVAS, BORDER_STRONG, BORDER_SUBTLE,
    CORNER_RADIUS_MD_PX, CORNER_RADIUS_PILL_PX, CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX,
    FONT_SIZE_SMALL_PX, INK_ON_ACCENT, STATUS_DANGER, STATUS_DANGER_INK, STATUS_DANGER_SOLID,
    STATUS_DANGER_TINT, STATUS_RUNNING, STATUS_RUNNING_INK, STATUS_RUNNING_TINT, SURFACE_CODE,
    SURFACE_ELEVATED, SURFACE_HOVER, SURFACE_PANEL, TEXT_MUTED, TEXT_PRIMARY, build_styled_button,
    font_family_semibold, rich_bold, rich_mono, rich_mono_bold, rich_semibold,
    truncate_display_text,
};

use status_strips::{
    has_kill_switch_collision, render_editor_live_status_pill, render_emergency_stop_banner,
    render_inline_setup_strip,
};

const SCRIPT_NAME_INPUT_WIDTH_PX: f32 = 196.0;
const SCRIPT_NAME_MIN_WIDTH_PX: f32 = 96.0;
const COMPACT_ROW_WIDTH_THRESHOLD_PX: f32 = 760.0;
const COMPACT_STOPPED_ROW_WIDTH_THRESHOLD_PX: f32 = 850.0;
const ROW_ONE_RESERVED_WIDE_PX: f32 = 576.0;
const ROW_ONE_RESERVED_WIDE_STOPPED_PX: f32 = 650.0;
const ROW_ONE_RESERVED_COMPACT_PX: f32 = 486.0;
const ROW_ONE_RESERVED_COMPACT_STOPPED_PX: f32 = 560.0;
const SCRIPT_NAME_FONT_SIZE_PX: f32 = 15.5;
pub(crate) const HEADER_CONTROL_HEIGHT_PX: f32 = 28.0;
const CONTEXT_CHIP_HEIGHT_PX: f32 = 25.0;
pub(crate) const INLINE_STRIP_BUTTON_HEIGHT_PX: f32 = 22.0;
const SEGMENT_INNER_RADIUS_PX: u8 = CORNER_RADIUS_PILL_PX.saturating_sub(3);
const MAX_TARGET_LABEL_CHARS: usize = 22;
const OPTIONS_POPOVER_WIDTH_PX: f32 = 284.0;

/// Identifies which hotkey binding control is actively awaiting a keyboard press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCaptureTarget {
    KillSwitch,
    ArmToggle,
}

/// Actions emitted by the top script configuration and safety gate bar.
#[derive(Debug, Clone)]
pub enum SettingsBarAction {
    OpenTargetPicker,
    SetScopeGlobal,
    BeginKeyCapture(KeyCaptureTarget),
    CancelKeyCapture,
    ClearArmToggleKey,
    SaveActiveScript,
    ToggleActiveScriptEnabled(bool),
    OpenActiveDashboard,
    EmergencyStopAll,
    ClearEmergencyStop,
    MarkActiveScriptDirty,
    ExportToRust,
}

/// Renders the calm 2-row script header, context strip, and inline setup/safety strip.
pub fn render_script_settings_bar(
    ui: &mut Ui,
    active_script: Option<&mut ScriptDocument>,
    is_dirty: bool,
    active_key_capture: Option<KeyCaptureTarget>,
    snapshot: &CoordinatorSnapshot,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let Some(script) = active_script else {
        render_empty_settings_bar(ui, snapshot, emitted_actions);
        return;
    };

    let validation_report = inspect_script_validation(script);
    let is_valid = validation_report.is_valid();

    let row_width_px = ui.available_width();
    let is_compact_row = row_width_px < COMPACT_ROW_WIDTH_THRESHOLD_PX
        || (snapshot.is_emergency_stopped && row_width_px < COMPACT_STOPPED_ROW_WIDTH_THRESHOLD_PX);
    let reserved_width_px = match (is_compact_row, snapshot.is_emergency_stopped) {
        (true, true) => ROW_ONE_RESERVED_COMPACT_STOPPED_PX,
        (true, false) => ROW_ONE_RESERVED_COMPACT_PX,
        (false, true) => ROW_ONE_RESERVED_WIDE_STOPPED_PX,
        (false, false) => ROW_ONE_RESERVED_WIDE_PX,
    };
    let desired_name_width_px = (row_width_px - reserved_width_px)
        .clamp(SCRIPT_NAME_MIN_WIDTH_PX, SCRIPT_NAME_INPUT_WIDTH_PX);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        render_inline_script_name_field(ui, script, desired_name_width_px, emitted_actions);
        render_mode_segmented_pill(ui, &mut script.mode, is_compact_row, emitted_actions);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            render_right_action_buttons(
                ui,
                script,
                is_dirty,
                is_valid,
                &validation_report,
                snapshot,
                emitted_actions,
            );
        });
    });

    ui.add_space(6.0);
    paint_subtle_horizontal_divider(ui);
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        render_target_chip(ui, &script.target_scope, emitted_actions);
        render_kill_switch_badge(
            ui,
            script,
            &validation_report,
            active_key_capture,
            emitted_actions,
        );
        render_arm_toggle_badge(ui, script, active_key_capture, emitted_actions);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            render_options_popover_button(ui, script, emitted_actions);
        });
    });

    if snapshot.is_emergency_stopped {
        ui.add_space(6.0);
        render_emergency_stop_banner(ui, snapshot.emergency_stop_count, emitted_actions);
    }

    if !is_valid {
        ui.add_space(6.0);
        render_inline_setup_strip(ui, &validation_report, emitted_actions);
    }
}

fn render_empty_settings_bar(
    ui: &mut Ui,
    snapshot: &CoordinatorSnapshot,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Select a script on the left or create a new one to start.")
                .size(FONT_SIZE_BODY_PX)
                .color(TEXT_MUTED),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            render_stop_all_button(ui, snapshot.is_emergency_stopped, emitted_actions);
        });
    });
}

fn render_inline_script_name_field(
    ui: &mut Ui,
    script: &mut ScriptDocument,
    desired_name_width_px: f32,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let name_edit_id = Id::unique(("script_header_name_edit", script.id.as_str()));
    let hover_memory_id = Id::unique(("script_header_name_hovered", script.id.as_str()));
    let is_focused = ui.ctx().memory(|memory| memory.has_focus(name_edit_id));
    let was_hovered = ui
        .ctx()
        .data(|data| data.get_temp::<bool>(hover_memory_id))
        .unwrap_or(false);

    let (fill_color, border_color) = if is_focused {
        (SURFACE_CODE, ACCENT_PRIMARY)
    } else if was_hovered {
        (SURFACE_ELEVATED, BORDER_SUBTLE)
    } else {
        (Color32::TRANSPARENT, Color32::TRANSPARENT)
    };

    let frame_response = Frame::NONE
        .fill(fill_color)
        .stroke(Stroke::new(1.0, border_color))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            let text_response = ui.add(
                TextEdit::singleline(&mut script.name)
                    .id(name_edit_id)
                    .frame(Frame::NONE)
                    .font(FontId::new(
                        SCRIPT_NAME_FONT_SIZE_PX,
                        font_family_semibold(),
                    ))
                    .desired_width(desired_name_width_px)
                    .hint_text("Untitled script"),
            );
            if text_response.changed() {
                emitted_actions.push(SettingsBarAction::MarkActiveScriptDirty);
            }
            text_response
        });

    let is_hovered = frame_response.response.hovered() || frame_response.inner.hovered();
    ui.ctx()
        .data_mut(|data| data.insert_temp(hover_memory_id, is_hovered));
}

fn render_mode_segmented_pill(
    ui: &mut Ui,
    mode: &mut ScriptMode,
    is_compact_row: bool,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let mode_choices = if is_compact_row {
        [
            (ScriptMode::VisualBlocks, "Blocks", "Visual Blocks mode"),
            (ScriptMode::AdvancedRust, "Rust", "Advanced Rust mode"),
        ]
    } else {
        [
            (
                ScriptMode::VisualBlocks,
                "Visual Blocks",
                "Visual Blocks mode",
            ),
            (
                ScriptMode::AdvancedRust,
                "Advanced Rust",
                "Advanced Rust mode",
            ),
        ]
    };

    Frame::NONE
        .fill(BG_CANVAS)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (candidate_mode, label_text, tooltip_text) in mode_choices {
                    let is_selected = *mode == candidate_mode;
                    let (label, fill, border) = if is_selected {
                        (
                            rich_semibold(label_text, FONT_SIZE_SMALL_PX, TEXT_PRIMARY),
                            SURFACE_HOVER,
                            BORDER_STRONG,
                        )
                    } else {
                        (
                            RichText::new(label_text)
                                .size(FONT_SIZE_SMALL_PX)
                                .color(TEXT_MUTED),
                            Color32::TRANSPARENT,
                            Color32::TRANSPARENT,
                        )
                    };
                    let button = build_styled_button(
                        label,
                        fill,
                        border,
                        SEGMENT_INNER_RADIUS_PX,
                        vec2(0.0, INLINE_STRIP_BUTTON_HEIGHT_PX),
                    );
                    let response = ui.add(button);
                    let clicked = if is_compact_row {
                        response.on_hover_text(tooltip_text).clicked()
                    } else {
                        response.clicked()
                    };
                    if clicked && !is_selected {
                        *mode = candidate_mode;
                        emitted_actions.push(SettingsBarAction::MarkActiveScriptDirty);
                    }
                }
            });
        });
}

fn render_target_chip(
    ui: &mut Ui,
    target_scope: &TargetScope,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let (chip_text, fill, ink, border, hover_detail) = match target_scope {
        TargetScope::Global => (
            "Scope: Global".to_string(),
            SURFACE_ELEVATED,
            TEXT_PRIMARY,
            BORDER_SUBTLE,
            "Active in any focused window. Click to attach a game window or process.".to_string(),
        ),
        TargetScope::SelectedGame(rule) if rule.has_match_criteria() => (
            format!(
                "Game: {}",
                truncate_display_text(&rule.display_name, MAX_TARGET_LABEL_CHARS)
            ),
            SURFACE_ELEVATED,
            TEXT_PRIMARY,
            BORDER_STRONG,
            format!(
                "Scoped to '{}'. Click to change target window or process.",
                rule.display_name
            ),
        ),
        TargetScope::SelectedGame(_) => (
            "Game: Attach Target…".to_string(),
            ACCENT_PRIMARY_TINT,
            ACCENT_PRIMARY,
            ACCENT_PRIMARY,
            "Selected Game scope needs a target window or process. Click to open Target Picker."
                .to_string(),
        ),
    };

    let target_button = build_styled_button(
        rich_semibold(chip_text, FONT_SIZE_SMALL_PX, ink),
        fill,
        border,
        CORNER_RADIUS_PILL_PX,
        vec2(0.0, CONTEXT_CHIP_HEIGHT_PX),
    );
    if ui.add(target_button).on_hover_text(hover_detail).clicked() {
        emitted_actions.push(SettingsBarAction::OpenTargetPicker);
    }
}

fn render_kill_switch_badge(
    ui: &mut Ui,
    script: &ScriptDocument,
    validation_report: &ValidationReport,
    active_key_capture: Option<KeyCaptureTarget>,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let is_capturing = active_key_capture == Some(KeyCaptureTarget::KillSwitch);
    let has_conflict = has_kill_switch_collision(validation_report);
    let (label_text, fill, ink, border) = match (is_capturing, script.kill_switch_key, has_conflict)
    {
        (true, _, _) => (
            "Kill: Press key (Esc)".to_string(),
            ACCENT_PRIMARY_TINT,
            ACCENT_PRIMARY,
            ACCENT_PRIMARY,
        ),
        (false, Some(key), true) => (
            format!("Kill: {} (Conflict)", key.display_label()),
            STATUS_DANGER_TINT,
            STATUS_DANGER_INK,
            STATUS_DANGER,
        ),
        (false, Some(key), false) => (
            format!("Kill: {}", key.display_label()),
            SURFACE_ELEVATED,
            TEXT_PRIMARY,
            BORDER_STRONG,
        ),
        (false, None, _) => (
            "Kill: Unset".to_string(),
            ACCENT_PRIMARY_TINT,
            ACCENT_PRIMARY,
            ACCENT_PRIMARY,
        ),
    };

    let button = build_styled_button(
        rich_mono_bold(label_text, FONT_SIZE_SMALL_PX, ink),
        fill,
        border,
        CORNER_RADIUS_PILL_PX,
        vec2(0.0, CONTEXT_CHIP_HEIGHT_PX),
    );
    let tooltip = "Mandatory Hard Kill-Switch key. Pressing it immediately halts all scripts and releases every held input.";
    if ui.add(button).on_hover_text(tooltip).clicked() {
        emitted_actions.push(if is_capturing {
            SettingsBarAction::CancelKeyCapture
        } else {
            SettingsBarAction::BeginKeyCapture(KeyCaptureTarget::KillSwitch)
        });
    }
}

fn render_arm_toggle_badge(
    ui: &mut Ui,
    script: &ScriptDocument,
    active_key_capture: Option<KeyCaptureTarget>,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let is_capturing = active_key_capture == Some(KeyCaptureTarget::ArmToggle);
    let (label_text, fill, ink, border) = match (is_capturing, script.arm_toggle_key) {
        (true, _) => (
            "Arm: Press key (Esc)".to_string(),
            ACCENT_PRIMARY_TINT,
            ACCENT_PRIMARY,
            ACCENT_PRIMARY,
        ),
        (false, Some(key)) => (
            format!("Arm: {}", key.display_label()),
            SURFACE_ELEVATED,
            TEXT_PRIMARY,
            BORDER_SUBTLE,
        ),
        (false, None) => (
            "Arm: None".to_string(),
            Color32::TRANSPARENT,
            TEXT_MUTED,
            BORDER_SUBTLE,
        ),
    };

    let arm_button = build_styled_button(
        rich_mono(label_text, FONT_SIZE_SMALL_PX, ink),
        fill,
        border,
        CORNER_RADIUS_PILL_PX,
        vec2(0.0, CONTEXT_CHIP_HEIGHT_PX),
    );
    if ui
        .add(arm_button)
        .on_hover_text("Optional hotkey to toggle whether this script is armed while enabled")
        .clicked()
    {
        emitted_actions.push(if is_capturing {
            SettingsBarAction::CancelKeyCapture
        } else {
            SettingsBarAction::BeginKeyCapture(KeyCaptureTarget::ArmToggle)
        });
    }

    if script.arm_toggle_key.is_some()
        && ui
            .add(build_styled_button(
                rich_semibold("×", FONT_SIZE_SMALL_PX, TEXT_MUTED),
                Color32::TRANSPARENT,
                BORDER_SUBTLE,
                CORNER_RADIUS_PILL_PX,
                vec2(22.0, CONTEXT_CHIP_HEIGHT_PX),
            ))
            .on_hover_text("Clear optional Arm Toggle key")
            .clicked()
    {
        emitted_actions.push(SettingsBarAction::ClearArmToggleKey);
    }
}

fn render_options_popover_button(
    ui: &mut Ui,
    script: &mut ScriptDocument,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let border_color = if script.intercept_trigger_input {
        BORDER_STRONG
    } else {
        BORDER_SUBTLE
    };
    let button_response = ui
        .add(build_styled_button(
            rich_semibold("Options ▾", FONT_SIZE_SMALL_PX, TEXT_PRIMARY),
            SURFACE_ELEVATED,
            border_color,
            CORNER_RADIUS_SM_PX,
            vec2(0.0, CONTEXT_CHIP_HEIGHT_PX),
        ))
        .on_hover_text("Trigger interception, scope switching, and Rust export options");

    let popover_frame = Frame::NONE
        .fill(SURFACE_PANEL)
        .stroke(Stroke::new(1.0, BORDER_STRONG))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
        .shadow(ui.style().visuals.popup_shadow)
        .inner_margin(Margin::same(12));

    Popup::from_toggle_button_response(&button_response)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .gap(6.0)
        .width(OPTIONS_POPOVER_WIDTH_PX)
        .frame(popover_frame)
        .show(|ui| render_options_popover_body(ui, script, emitted_actions));
}

fn render_options_popover_body(
    ui: &mut Ui,
    script: &mut ScriptDocument,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let intercept_response = ui.add(Checkbox::new(
        &mut script.intercept_trigger_input,
        rich_semibold("Intercept Trigger", FONT_SIZE_BODY_PX, TEXT_PRIMARY),
    ));
    if intercept_response.changed() {
        emitted_actions.push(SettingsBarAction::MarkActiveScriptDirty);
    }
    ui.add_space(2.0);
    ui.label(
        RichText::new(
            "Grabs the physical mouse via EVIOCGRAB while armed and focused so same-button loops work on Wayland/Proton. Auto-releases on focus loss or Kill-Switch.",
        )
        .size(FONT_SIZE_SMALL_PX)
        .color(TEXT_MUTED),
    );
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);

    let full_width_px = ui.available_width();
    let (scope_label, scope_action) = match script.target_scope {
        TargetScope::SelectedGame(_) => {
            ("Switch to Global Scope", SettingsBarAction::SetScopeGlobal)
        }
        TargetScope::Global => ("Target Selected Game…", SettingsBarAction::OpenTargetPicker),
    };
    if ui
        .add(build_styled_button(
            rich_semibold(scope_label, FONT_SIZE_SMALL_PX, TEXT_PRIMARY),
            SURFACE_ELEVATED,
            BORDER_SUBTLE,
            CORNER_RADIUS_SM_PX,
            vec2(full_width_px, HEADER_CONTROL_HEIGHT_PX),
        ))
        .clicked()
    {
        emitted_actions.push(scope_action);
        ui.close();
    }

    if script.mode == ScriptMode::VisualBlocks {
        ui.add_space(6.0);
        if ui
            .add(build_styled_button(
                rich_semibold(
                    "Export Visual Blocks to Rust",
                    FONT_SIZE_SMALL_PX,
                    TEXT_PRIMARY,
                ),
                SURFACE_ELEVATED,
                BORDER_STRONG,
                CORNER_RADIUS_SM_PX,
                vec2(full_width_px, HEADER_CONTROL_HEIGHT_PX),
            ))
            .on_hover_text("Generate equivalent clickforge_sdk Rust code and open Advanced Rust")
            .clicked()
        {
            emitted_actions.push(SettingsBarAction::ExportToRust);
            ui.close();
        }
    }
}

fn render_right_action_buttons(
    ui: &mut Ui,
    script: &ScriptDocument,
    is_dirty: bool,
    is_valid: bool,
    validation_report: &ValidationReport,
    snapshot: &CoordinatorSnapshot,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    render_stop_all_button(ui, snapshot.is_emergency_stopped, emitted_actions);
    render_editor_live_status_pill(ui, script, validation_report, snapshot, emitted_actions);

    let first_issue = validation_report
        .issues
        .first()
        .map(|issue| issue.message.as_str())
        .unwrap_or("Resolve setup issues first.");

    let (enable_label, enable_fill, enable_ink, enable_border) = if script.is_enabled {
        (
            "Enabled",
            STATUS_RUNNING_TINT,
            STATUS_RUNNING_INK,
            STATUS_RUNNING,
        )
    } else {
        ("Enable", SURFACE_ELEVATED, TEXT_PRIMARY, BORDER_STRONG)
    };
    let can_toggle_enable = is_valid || script.is_enabled;
    let enable_response = ui.add_enabled(
        can_toggle_enable,
        build_styled_button(
            rich_semibold(enable_label, FONT_SIZE_SMALL_PX, enable_ink),
            enable_fill,
            enable_border,
            CORNER_RADIUS_PILL_PX,
            vec2(72.0, HEADER_CONTROL_HEIGHT_PX),
        ),
    );
    if enable_response.clicked() {
        emitted_actions.push(SettingsBarAction::ToggleActiveScriptEnabled(
            !script.is_enabled,
        ));
    }
    if !can_toggle_enable {
        enable_response.on_disabled_hover_text(format!("Cannot enable: {first_issue}"));
    } else if !is_valid && script.is_enabled {
        enable_response.on_hover_text(format!("Disable script (validation issue: {first_issue})"));
    }

    let (save_fill, save_ink, save_border) = if is_dirty && is_valid {
        (ACCENT_PRIMARY, INK_ON_ACCENT, ACCENT_PRIMARY)
    } else {
        (Color32::TRANSPARENT, TEXT_MUTED, BORDER_SUBTLE)
    };
    let save_label = if is_dirty { "Save *" } else { "Save" };
    let save_response = ui.add_enabled(
        is_valid,
        build_styled_button(
            rich_semibold(save_label, FONT_SIZE_SMALL_PX, save_ink),
            save_fill,
            save_border,
            CORNER_RADIUS_SM_PX,
            vec2(64.0, HEADER_CONTROL_HEIGHT_PX),
        ),
    );
    if save_response.clicked() {
        emitted_actions.push(SettingsBarAction::SaveActiveScript);
    }
    if !is_valid {
        save_response.on_disabled_hover_text(format!("Cannot save: {first_issue}"));
    } else {
        let hint = if is_dirty {
            "Save script changes (Ctrl+S)"
        } else {
            "All changes saved (Ctrl+S)"
        };
        save_response.on_hover_text(hint);
    }
}

fn render_stop_all_button(
    ui: &mut Ui,
    is_emergency_stopped: bool,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let stop_button = Button::new(rich_bold("STOP ALL", FONT_SIZE_SMALL_PX, Color32::WHITE))
        .fill(STATUS_DANGER_SOLID)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .min_size(vec2(80.0, HEADER_CONTROL_HEIGHT_PX));
    if ui
        .add(stop_button)
        .on_hover_text(
            "Immediately halt every running script, release grabs, and release all virtual inputs",
        )
        .clicked()
    {
        emitted_actions.push(SettingsBarAction::EmergencyStopAll);
    }

    if is_emergency_stopped
        && ui
            .add(build_styled_button(
                rich_semibold("Resume", FONT_SIZE_SMALL_PX, ACCENT_PRIMARY),
                ACCENT_PRIMARY_TINT,
                ACCENT_PRIMARY,
                CORNER_RADIUS_SM_PX,
                vec2(0.0, HEADER_CONTROL_HEIGHT_PX),
            ))
            .on_hover_text("Clear emergency stop latch so enabled scripts can arm again")
            .clicked()
    {
        emitted_actions.push(SettingsBarAction::ClearEmergencyStop);
    }
}

fn paint_subtle_horizontal_divider(ui: &mut Ui) {
    let available_width_px = ui.available_width();
    let (divider_rect, _) = ui.allocate_exact_size(vec2(available_width_px, 1.0), Sense::hover());
    ui.painter().line_segment(
        [divider_rect.left_center(), divider_rect.right_center()],
        Stroke::new(1.0, BORDER_SUBTLE),
    );
}
