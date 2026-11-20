use egui::{Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Stroke, Ui, vec2};

use crate::models::{CoordinatorSnapshot, ScriptDocument, ValidationIssueCode, ValidationReport};
use crate::ui::active_dashboard::classify_script_live_status;
use crate::ui::script_settings_bar::{
    HEADER_CONTROL_HEIGHT_PX, INLINE_STRIP_BUTTON_HEIGHT_PX, KeyCaptureTarget, SettingsBarAction,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BORDER_STRONG, CORNER_RADIUS_PILL_PX, CORNER_RADIUS_SM_PX,
    FONT_SIZE_SMALL_PX, STATUS_DANGER, STATUS_DANGER_INK, STATUS_DANGER_SOLID, STATUS_DANGER_TINT,
    SURFACE_ELEVATED, SURFACE_HOVER, TEXT_PRIMARY, build_styled_button, render_badge_pill,
    rich_mono_bold, rich_semibold,
};

const MAX_INLINE_VALIDATION_ISSUES: usize = 2;

/// Renders the clickable live runtime status pill beside the Editor Enable toggle.
pub(crate) fn render_editor_live_status_pill(
    ui: &mut Ui,
    script: &ScriptDocument,
    validation_report: &ValidationReport,
    snapshot: &CoordinatorSnapshot,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let status_info = classify_script_live_status(script, validation_report, snapshot);
    let pill_button = build_styled_button(
        rich_mono_bold(
            status_info.compact_label,
            FONT_SIZE_SMALL_PX,
            status_info.ink_color,
        ),
        status_info.fill_color,
        status_info.border_color,
        CORNER_RADIUS_PILL_PX,
        vec2(0.0, HEADER_CONTROL_HEIGHT_PX),
    );
    let tooltip = format!(
        "{}\nClick to open the Active live dashboard.",
        status_info.explanation
    );
    if ui.add(pill_button).on_hover_text(tooltip).clicked() {
        emitted_actions.push(SettingsBarAction::OpenActiveDashboard);
    }
}

/// Renders the red Emergency Stop notification strip with a quick Clear Stop button.
pub(crate) fn render_emergency_stop_banner(
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
                ui.label(rich_semibold(
                    format!(
                        "Emergency Stop active (count: {emergency_stop_count}). All scripts are halted and inputs released."
                    ),
                    FONT_SIZE_SMALL_PX,
                    STATUS_DANGER_INK,
                ));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(build_styled_button(
                            rich_semibold("Clear Stop", FONT_SIZE_SMALL_PX, TEXT_PRIMARY),
                            SURFACE_ELEVATED,
                            BORDER_STRONG,
                            CORNER_RADIUS_SM_PX,
                            vec2(0.0, INLINE_STRIP_BUTTON_HEIGHT_PX),
                        ))
                        .clicked()
                    {
                        emitted_actions.push(SettingsBarAction::ClearEmergencyStop);
                    }
                });
            });
        });
}

/// Renders the inline validation/setup guidance strip with one-click remediation buttons.
pub(crate) fn render_inline_setup_strip(
    ui: &mut Ui,
    validation_report: &ValidationReport,
    emitted_actions: &mut Vec<SettingsBarAction>,
) {
    let needs_kill_switch = validation_report.has_issue(ValidationIssueCode::MissingKillSwitchKey)
        || has_kill_switch_collision(validation_report);
    let needs_target = validation_report.has_issue(ValidationIssueCode::InvalidSelectedGameTarget);
    let is_setup_only = validation_report.issues.iter().all(|issue| {
        matches!(
            issue.code,
            ValidationIssueCode::MissingKillSwitchKey
                | ValidationIssueCode::InvalidSelectedGameTarget
        )
    });

    let (strip_fill, strip_border, badge_label, badge_fill, badge_ink, badge_border) =
        if is_setup_only {
            (
                SURFACE_ELEVATED,
                BORDER_STRONG,
                "Setup",
                ACCENT_PRIMARY_TINT,
                ACCENT_PRIMARY,
                ACCENT_PRIMARY,
            )
        } else {
            (
                STATUS_DANGER_TINT,
                STATUS_DANGER,
                "Safety Gate",
                STATUS_DANGER_SOLID,
                Color32::WHITE,
                STATUS_DANGER_SOLID,
            )
        };

    Frame::NONE
        .fill(strip_fill)
        .stroke(Stroke::new(1.0, strip_border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                render_badge_pill(ui, badge_label, badge_fill, badge_ink, badge_border);
                ui.label(
                    RichText::new(build_concise_validation_summary(validation_report))
                        .size(FONT_SIZE_SMALL_PX)
                        .color(TEXT_PRIMARY),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if needs_target
                        && ui
                            .add(build_styled_button(
                                rich_semibold("Attach Target", FONT_SIZE_SMALL_PX, TEXT_PRIMARY),
                                SURFACE_HOVER,
                                BORDER_STRONG,
                                CORNER_RADIUS_SM_PX,
                                vec2(0.0, INLINE_STRIP_BUTTON_HEIGHT_PX),
                            ))
                            .clicked()
                    {
                        emitted_actions.push(SettingsBarAction::OpenTargetPicker);
                    }
                    if needs_kill_switch
                        && ui
                            .add(build_styled_button(
                                rich_semibold(
                                    "Bind Kill-Switch",
                                    FONT_SIZE_SMALL_PX,
                                    ACCENT_PRIMARY,
                                ),
                                ACCENT_PRIMARY_TINT,
                                ACCENT_PRIMARY,
                                CORNER_RADIUS_SM_PX,
                                vec2(0.0, INLINE_STRIP_BUTTON_HEIGHT_PX),
                            ))
                            .clicked()
                    {
                        emitted_actions.push(SettingsBarAction::BeginKeyCapture(
                            KeyCaptureTarget::KillSwitch,
                        ));
                    }
                });
            });
        });
}

/// Returns true if the script's Kill-Switch key collides with any trigger, arm, or action key.
pub(crate) fn has_kill_switch_collision(validation_report: &ValidationReport) -> bool {
    validation_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithTriggerKey)
        || validation_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithArmToggleKey)
        || validation_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithEmittedKey)
        || validation_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithConditionKey)
        || validation_report.has_issue(ValidationIssueCode::KillSwitchCollision)
}

fn build_concise_validation_summary(validation_report: &ValidationReport) -> String {
    let mut parts: Vec<String> = validation_report
        .issues
        .iter()
        .take(MAX_INLINE_VALIDATION_ISSUES)
        .map(|issue| issue.message.clone())
        .collect();
    if validation_report.issues.len() > MAX_INLINE_VALIDATION_ISSUES {
        let hidden_count = validation_report
            .issues
            .len()
            .saturating_sub(MAX_INLINE_VALIDATION_ISSUES);
        parts.push(format!("+{hidden_count} more"));
    }
    parts.join("; ")
}
