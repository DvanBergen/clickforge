use egui::{
    Align, Button, Checkbox, Color32, CornerRadius, Frame, Layout, Margin, RichText, Stroke,
    TextEdit, Ui,
};

use crate::models::{
    CoordinatorSnapshot, ScriptDocument, ScriptMode, TargetScope, ValidationIssueCode,
    ValidationReport, inspect_script_validation,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BORDER_STRONG, BORDER_SUBTLE, CORNER_RADIUS_SM_PX,
    FONT_SIZE_BODY_PX, FONT_SIZE_SMALL_PX, INK_ON_ACCENT, STATUS_DANGER, STATUS_DANGER_TINT,
    STATUS_RUNNING, STATUS_RUNNING_TINT, SURFACE_ELEVATED, TEXT_MUTED, TEXT_PRIMARY,
    render_badge_pill, truncate_display_text,
};

const SCRIPT_NAME_INPUT_WIDTH_PX: f32 = 175.0;
const MAX_TARGET_LABEL_CHARS: usize = 26;
const MAX_INLINE_VALIDATION_ISSUES: usize = 3;

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
    EmergencyStopAll,
    ClearEmergencyStop,
    MarkActiveScriptDirty,
}

/// Renders the top script settings bar, Hard Kill-Switch binder, and inline validation gate.
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

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.spacing_mut().item_spacing.y = 6.0;

        let name_response = ui.add(
            TextEdit::singleline(&mut script.name)
                .desired_width(SCRIPT_NAME_INPUT_WIDTH_PX)
                .hint_text("Script name"),
        );
        if name_response.changed() {
            emitted_actions.push(SettingsBarAction::MarkActiveScriptDirty);
        }

        render_target_controls(ui, &script.target_scope, emitted_actions);
        ui.separator();
        render_mode_segmented_toggle(ui, &mut script.mode, emitted_actions);
        ui.separator();
        render_kill_switch_binder(
            ui,
            script,
            &validation_report,
            active_key_capture,
            emitted_actions,
        );
        render_arm_toggle_binder(ui, script, active_key_capture, emitted_actions);
        render_intercept_trigger_checkbox(ui, script, emitted_actions);

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
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

    if snapshot.is_emergency_stopped {
        ui.add_space(6.0);
        render_emergency_stop_banner(ui, snapshot.emergency_stop_count, emitted_actions);
    }

    if !is_valid {
        ui.add_space(6.0);
        render_validation_blocker_banner(ui, &validation_report);
    }
}

fn render_empty_settings_bar(
    ui: &mut Ui,
    snapshot: &CoordinatorSnapshot,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("No script selected. Create or select a script from the sidebar.")
                .color(TEXT_MUTED),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            render_stop_all_button(ui, snapshot.is_emergency_stopped, emitted_actions);
        });
    });
}

fn render_target_controls(
    ui: &mut Ui,
    target_scope: &TargetScope,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let (badge_text, badge_fill, badge_ink, badge_border) = match target_scope {
        TargetScope::Global => ("Global", SURFACE_ELEVATED, TEXT_MUTED, BORDER_SUBTLE),
        TargetScope::SelectedGame(rule) if rule.has_match_criteria() => {
            ("Game", ACCENT_PRIMARY_TINT, ACCENT_PRIMARY, ACCENT_PRIMARY)
        }
        TargetScope::SelectedGame(_) => {
            ("Unbound", STATUS_DANGER_TINT, STATUS_DANGER, STATUS_DANGER)
        }
    };
    render_badge_pill(ui, badge_text, badge_fill, badge_ink, badge_border);

    let target_label = truncate_display_text(&target_scope.display_label(), MAX_TARGET_LABEL_CHARS);
    let attach_button = Button::new(
        RichText::new(format!("{target_label}  |  Attach Process / Window"))
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_PRIMARY),
    )
    .fill(SURFACE_ELEVATED)
    .stroke(Stroke::new(1.0, BORDER_STRONG))
    .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX));

    if ui
        .add(attach_button)
        .on_hover_text("Open Cheat Engine style Window / Process target picker")
        .clicked()
    {
        emitted_actions.push(SettingsBarAction::OpenTargetPicker);
    }

    if matches!(target_scope, TargetScope::SelectedGame(_))
        && ui
            .add(
                Button::new(RichText::new("Global").size(FONT_SIZE_SMALL_PX))
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .on_hover_text("Switch this script to Global scope (active in any window)")
            .clicked()
    {
        emitted_actions.push(SettingsBarAction::SetScopeGlobal);
    }
}

fn render_mode_segmented_toggle(
    ui: &mut Ui,
    mode: &mut ScriptMode,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let is_blocks = *mode == ScriptMode::VisualBlocks;
    let is_rust = *mode == ScriptMode::AdvancedRust;

    let blocks_button = Button::new(
        RichText::new("Visual Blocks")
            .size(FONT_SIZE_SMALL_PX)
            .color(if is_blocks {
                INK_ON_ACCENT
            } else {
                TEXT_PRIMARY
            }),
    )
    .fill(if is_blocks {
        ACCENT_PRIMARY
    } else {
        SURFACE_ELEVATED
    })
    .stroke(Stroke::new(
        1.0,
        if is_blocks {
            ACCENT_PRIMARY
        } else {
            BORDER_SUBTLE
        },
    ));

    if ui.add(blocks_button).clicked() && !is_blocks {
        *mode = ScriptMode::VisualBlocks;
        emitted_actions.push(SettingsBarAction::MarkActiveScriptDirty);
    }

    let rust_button = Button::new(
        RichText::new("Advanced Rust")
            .size(FONT_SIZE_SMALL_PX)
            .color(if is_rust { INK_ON_ACCENT } else { TEXT_PRIMARY }),
    )
    .fill(if is_rust {
        ACCENT_PRIMARY
    } else {
        SURFACE_ELEVATED
    })
    .stroke(Stroke::new(
        1.0,
        if is_rust {
            ACCENT_PRIMARY
        } else {
            BORDER_SUBTLE
        },
    ));

    if ui.add(rust_button).clicked() && !is_rust {
        *mode = ScriptMode::AdvancedRust;
        emitted_actions.push(SettingsBarAction::MarkActiveScriptDirty);
    }
}

fn render_kill_switch_binder(
    ui: &mut Ui,
    script: &ScriptDocument,
    validation_report: &ValidationReport,
    active_key_capture: Option<KeyCaptureTarget>,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let is_capturing = active_key_capture == Some(KeyCaptureTarget::KillSwitch);
    let has_kill_switch_error = validation_report
        .has_issue(ValidationIssueCode::MissingKillSwitchKey)
        || validation_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithTriggerKey)
        || validation_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithArmToggleKey)
        || validation_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithEmittedKey)
        || validation_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithConditionKey);

    let (label_text, fill_color, text_color, border_color) = if is_capturing {
        (
            "Kill: Press key... (Esc)".to_string(),
            ACCENT_PRIMARY_TINT,
            ACCENT_PRIMARY,
            ACCENT_PRIMARY,
        )
    } else if let Some(kill_key) = script.kill_switch_key {
        if has_kill_switch_error {
            (
                format!("Kill: {} (Conflict)", kill_key.display_label()),
                STATUS_DANGER_TINT,
                STATUS_DANGER,
                STATUS_DANGER,
            )
        } else {
            (
                format!("Kill: {}", kill_key.display_label()),
                SURFACE_ELEVATED,
                TEXT_PRIMARY,
                STATUS_DANGER,
            )
        }
    } else {
        (
            "Kill: Unset (Required)".to_string(),
            STATUS_DANGER_TINT,
            STATUS_DANGER,
            STATUS_DANGER,
        )
    };

    let binder_button = Button::new(
        RichText::new(label_text)
            .size(FONT_SIZE_SMALL_PX)
            .monospace()
            .strong()
            .color(text_color),
    )
    .fill(fill_color)
    .stroke(Stroke::new(1.5, border_color))
    .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX));

    let response = ui.add(binder_button).on_hover_text(
        "Mandatory Hard Kill-Switch key. Pressing this key physically stops all scripts and releases every input immediately.",
    );
    if response.clicked() {
        if is_capturing {
            emitted_actions.push(SettingsBarAction::CancelKeyCapture);
        } else {
            emitted_actions.push(SettingsBarAction::BeginKeyCapture(
                KeyCaptureTarget::KillSwitch,
            ));
        }
    }
}

fn render_arm_toggle_binder(
    ui: &mut Ui,
    script: &ScriptDocument,
    active_key_capture: Option<KeyCaptureTarget>,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let is_capturing = active_key_capture == Some(KeyCaptureTarget::ArmToggle);
    let (label_text, fill_color, text_color, border_color) = if is_capturing {
        (
            "Arm: Press key... (Esc)".to_string(),
            ACCENT_PRIMARY_TINT,
            ACCENT_PRIMARY,
            ACCENT_PRIMARY,
        )
    } else if let Some(arm_key) = script.arm_toggle_key {
        (
            format!("Arm: {}", arm_key.display_label()),
            SURFACE_ELEVATED,
            TEXT_PRIMARY,
            BORDER_STRONG,
        )
    } else {
        (
            "Arm: None".to_string(),
            SURFACE_ELEVATED,
            TEXT_MUTED,
            BORDER_SUBTLE,
        )
    };

    let arm_button = Button::new(
        RichText::new(label_text)
            .size(FONT_SIZE_SMALL_PX)
            .monospace()
            .color(text_color),
    )
    .fill(fill_color)
    .stroke(Stroke::new(1.0, border_color));

    if ui
        .add(arm_button)
        .on_hover_text("Optional hotkey to toggle whether this script is armed while enabled")
        .clicked()
    {
        if is_capturing {
            emitted_actions.push(SettingsBarAction::CancelKeyCapture);
        } else {
            emitted_actions.push(SettingsBarAction::BeginKeyCapture(
                KeyCaptureTarget::ArmToggle,
            ));
        }
    }

    if script.arm_toggle_key.is_some()
        && ui
            .add(
                Button::new(
                    RichText::new("x")
                        .size(FONT_SIZE_SMALL_PX)
                        .color(TEXT_MUTED),
                )
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .on_hover_text("Clear optional Arm Toggle key")
            .clicked()
    {
        emitted_actions.push(SettingsBarAction::ClearArmToggleKey);
    }
}

fn render_intercept_trigger_checkbox(
    ui: &mut Ui,
    script: &mut ScriptDocument,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let intercept_response = ui
        .add(Checkbox::new(
            &mut script.intercept_trigger_input,
            RichText::new("Intercept Trigger").size(FONT_SIZE_SMALL_PX),
        ))
        .on_hover_text(
            "Temporarily grabs the physical mouse via EVIOCGRAB only while this script is armed and its target window is focused. Forwards pointer movement, scroll, and other buttons while suppressing raw trigger clicks so same-button loops (Hold Left Mouse -> Rapid Click Left Mouse) work on Wayland/Proton. Automatically releases on focus loss, Kill-Switch, or exit.",
        );
    if intercept_response.changed() {
        emitted_actions.push(SettingsBarAction::MarkActiveScriptDirty);
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

    let first_issue_reason = validation_report
        .issues
        .first()
        .map(|issue| issue.message.as_str())
        .unwrap_or("Fix validation errors first.");

    let (enable_label, enable_fill, enable_text, enable_border) = if script.is_enabled {
        (
            "Enabled",
            STATUS_RUNNING_TINT,
            STATUS_RUNNING,
            STATUS_RUNNING,
        )
    } else {
        ("Enable", SURFACE_ELEVATED, TEXT_PRIMARY, BORDER_STRONG)
    };
    let enable_button = Button::new(
        RichText::new(enable_label)
            .size(FONT_SIZE_SMALL_PX)
            .strong()
            .color(enable_text),
    )
    .fill(enable_fill)
    .stroke(Stroke::new(1.0, enable_border));

    let enable_response = ui.add_enabled(is_valid, enable_button);
    if enable_response.clicked() {
        emitted_actions.push(SettingsBarAction::ToggleActiveScriptEnabled(
            !script.is_enabled,
        ));
    }
    if !is_valid {
        enable_response.on_disabled_hover_text(format!("Cannot enable: {first_issue_reason}"));
    }

    let save_label = if is_dirty { "Save *" } else { "Save" };
    let save_button = Button::new(
        RichText::new(save_label)
            .size(FONT_SIZE_SMALL_PX)
            .strong()
            .color(if is_dirty && is_valid {
                INK_ON_ACCENT
            } else {
                TEXT_PRIMARY
            }),
    )
    .fill(if is_dirty && is_valid {
        ACCENT_PRIMARY
    } else {
        SURFACE_ELEVATED
    })
    .stroke(Stroke::new(
        1.0,
        if is_dirty && is_valid {
            ACCENT_PRIMARY
        } else {
            BORDER_STRONG
        },
    ));

    let save_response = ui.add_enabled(is_valid, save_button);
    if save_response.clicked() {
        emitted_actions.push(SettingsBarAction::SaveActiveScript);
    }
    if !is_valid {
        save_response.on_disabled_hover_text(format!("Cannot save: {first_issue_reason}"));
    }
}

fn render_stop_all_button(
    ui: &mut Ui,
    is_emergency_stopped: bool,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let stop_button = Button::new(
        RichText::new("STOP ALL")
            .size(FONT_SIZE_SMALL_PX)
            .strong()
            .color(Color32::WHITE),
    )
    .fill(STATUS_DANGER)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX));

    if ui
        .add(stop_button)
        .on_hover_text(
            "Immediately halt every running script, release grabs, and release all virtual inputs",
        )
        .clicked()
    {
        emitted_actions.push(SettingsBarAction::EmergencyStopAll);
    }

    if is_emergency_stopped {
        let resume_button = Button::new(
            RichText::new("Resume")
                .size(FONT_SIZE_SMALL_PX)
                .color(ACCENT_PRIMARY),
        )
        .fill(ACCENT_PRIMARY_TINT)
        .stroke(Stroke::new(1.0, ACCENT_PRIMARY));
        if ui
            .add(resume_button)
            .on_hover_text("Clear emergency stop latch so enabled scripts can arm again")
            .clicked()
        {
            emitted_actions.push(SettingsBarAction::ClearEmergencyStop);
        }
    }
}

fn render_emergency_stop_banner(
    ui: &mut Ui,
    emergency_stop_count: u64,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    Frame::NONE
        .fill(STATUS_DANGER_TINT)
        .stroke(Stroke::new(1.0, STATUS_DANGER))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "Emergency Stop active (count: {emergency_stop_count}). All scripts are halted and inputs released."
                    ))
                    .size(FONT_SIZE_BODY_PX)
                    .strong()
                    .color(STATUS_DANGER),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Clear Emergency Stop").clicked() {
                        emitted_actions.push(SettingsBarAction::ClearEmergencyStop);
                    }
                });
            });
        });
}

fn render_validation_blocker_banner(ui: &mut Ui, validation_report: &ValidationReport) {
    Frame::NONE
        .fill(STATUS_DANGER_TINT)
        .stroke(Stroke::new(1.0, STATUS_DANGER))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                render_badge_pill(
                    ui,
                    "Safety Gate",
                    STATUS_DANGER,
                    Color32::WHITE,
                    STATUS_DANGER,
                );
                let visible_issues = validation_report
                    .issues
                    .iter()
                    .take(MAX_INLINE_VALIDATION_ISSUES);
                for (issue_index, issue) in visible_issues.enumerate() {
                    if issue_index > 0 {
                        ui.label(RichText::new("|").color(STATUS_DANGER));
                    }
                    ui.label(
                        RichText::new(&issue.message)
                            .size(FONT_SIZE_SMALL_PX)
                            .color(TEXT_PRIMARY),
                    );
                }
                if validation_report.issues.len() > MAX_INLINE_VALIDATION_ISSUES {
                    let hidden_count = validation_report
                        .issues
                        .len()
                        .saturating_sub(MAX_INLINE_VALIDATION_ISSUES);
                    ui.label(
                        RichText::new(format!("(+{hidden_count} more)"))
                            .size(FONT_SIZE_SMALL_PX)
                            .color(STATUS_DANGER),
                    );
                }
            });
        });
}
