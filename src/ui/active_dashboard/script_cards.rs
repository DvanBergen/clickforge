use egui::{Align, CornerRadius, Frame, Label, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};

use crate::models::{
    ScriptDocument, ScriptMode, ScriptRuntimeStatus, TargetScope, ValidationReport,
    inspect_script_validation_with_custom_blocks,
};
use crate::ui::active_dashboard::status_classifier::classify_script_live_status;
use crate::ui::active_dashboard::{
    ActiveDashboardAction, ActiveDashboardViewModel, HERO_BUTTON_HEIGHT_PX, paint_left_state_bar,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BG_CANVAS, BORDER_STRONG, BORDER_SUBTLE,
    CORNER_RADIUS_MD_PX, CORNER_RADIUS_PILL_PX, CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX,
    FONT_SIZE_HEADING_PX, FONT_SIZE_SMALL_PX, INK_ON_ACCENT, STATUS_RUNNING, STATUS_RUNNING_INK,
    STATUS_RUNNING_TINT, SURFACE_ELEVATED, SURFACE_HOVER, SURFACE_PANEL, TEXT_MUTED, TEXT_PRIMARY,
    build_styled_button, render_badge_pill, rich_bold, rich_mono_bold, rich_semibold,
    truncate_display_text,
};

const CARD_TITLE_FONT_SIZE_PX: f32 = 15.0;
const CARD_BADGE_FONT_SIZE_PX: f32 = 11.5;
const CARD_CONTROL_HEIGHT_PX: f32 = 28.0;
const TOGGLE_BUTTON_MIN_WIDTH_PX: f32 = 58.0;
const COMPACT_CARD_WIDTH_THRESHOLD_PX: f32 = 720.0;
const MAX_CARD_TITLE_CHARS_WIDE: usize = 28;
const MAX_CARD_TITLE_CHARS_COMPACT: usize = 16;
const MAX_TARGET_PILL_CHARS: usize = 24;
const MAX_TRIGGER_PILL_CHARS: usize = 30;

/// Renders the grouped Enabled and Disabled/Standby script status card sections.
pub(crate) fn render_grouped_script_sections(
    ui: &mut Ui,
    view_model: &ActiveDashboardViewModel<'_>,
    emitted_actions: &mut Vec<ActiveDashboardAction>,
) {
    if view_model.scripts.is_empty() {
        render_zero_scripts_card(ui, emitted_actions);
        return;
    }

    let enabled_scripts: Vec<&ScriptDocument> = view_model
        .scripts
        .iter()
        .filter(|script| script.is_enabled)
        .collect();
    let disabled_scripts: Vec<&ScriptDocument> = view_model
        .scripts
        .iter()
        .filter(|script| !script.is_enabled)
        .collect();

    render_section_heading(ui, "Enabled Scripts", enabled_scripts.len());
    ui.add_space(8.0);
    if enabled_scripts.is_empty() {
        Frame::NONE
            .fill(SURFACE_PANEL)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(
                        "No scripts are enabled yet. Turn any script ON below to arm it.",
                    )
                    .size(FONT_SIZE_BODY_PX)
                    .color(TEXT_MUTED),
                );
            });
    } else {
        for script in enabled_scripts {
            render_script_status_card(ui, script, view_model, emitted_actions);
            ui.add_space(8.0);
        }
    }

    ui.add_space(12.0);
    render_section_heading(ui, "Disabled / Standby Scripts", disabled_scripts.len());
    ui.add_space(8.0);
    if disabled_scripts.is_empty() {
        ui.label(
            RichText::new("Every script is currently enabled above.")
                .size(FONT_SIZE_SMALL_PX)
                .color(TEXT_MUTED),
        );
        return;
    }

    for script in disabled_scripts {
        render_script_status_card(ui, script, view_model, emitted_actions);
        ui.add_space(8.0);
    }
}

fn render_section_heading(ui: &mut Ui, title: &str, item_count: usize) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(rich_bold(title, FONT_SIZE_BODY_PX, TEXT_PRIMARY));
        render_badge_pill(
            ui,
            &item_count.to_string(),
            SURFACE_ELEVATED,
            TEXT_MUTED,
            BORDER_SUBTLE,
        );
    });
}

fn render_zero_scripts_card(ui: &mut Ui, emitted_actions: &mut Vec<ActiveDashboardAction>) {
    Frame::NONE
        .fill(SURFACE_PANEL)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
        .inner_margin(Margin::same(24))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(rich_bold(
                    "No Scripts Created Yet",
                    FONT_SIZE_HEADING_PX,
                    TEXT_PRIMARY,
                ));
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Create your first Visual Blocks or Advanced Rust script to arm and monitor it here.",
                    )
                    .size(FONT_SIZE_BODY_PX)
                    .color(TEXT_MUTED),
                );
                ui.add_space(12.0);
                let create_button = build_styled_button(
                    rich_semibold("+ New Script", FONT_SIZE_BODY_PX, INK_ON_ACCENT),
                    ACCENT_PRIMARY,
                    ACCENT_PRIMARY,
                    CORNER_RADIUS_SM_PX,
                    vec2(132.0, HERO_BUTTON_HEIGHT_PX),
                );
                if ui.add(create_button).clicked() {
                    emitted_actions.push(ActiveDashboardAction::OpenNewScriptModal);
                }
            });
        });
}

fn render_script_status_card(
    ui: &mut Ui,
    script: &ScriptDocument,
    view_model: &ActiveDashboardViewModel<'_>,
    emitted_actions: &mut Vec<ActiveDashboardAction>,
) {
    let validation_report =
        inspect_script_validation_with_custom_blocks(script, view_model.custom_blocks);
    let is_valid = validation_report.is_valid();
    let status_info = classify_script_live_status(script, &validation_report, view_model.snapshot);
    let is_selected = view_model.selected_script_id == Some(script.id.as_str());
    let is_dirty = view_model.dirty_script_ids.contains(&script.id);
    let runtime_status = view_model.snapshot.status_for_script(&script.id);

    let (card_fill, card_border) = if is_selected {
        (SURFACE_ELEVATED, BORDER_STRONG)
    } else if status_info.is_prominent {
        (SURFACE_PANEL, BORDER_STRONG)
    } else {
        (SURFACE_PANEL, BORDER_SUBTLE)
    };

    let frame_response = Frame::NONE
        .fill(card_fill)
        .stroke(Stroke::new(1.0, card_border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
        .inner_margin(Margin {
            left: 16,
            right: 14,
            top: 11,
            bottom: 11,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            render_card_header_row(
                ui,
                script,
                is_valid,
                is_dirty,
                &validation_report,
                &status_info,
                emitted_actions,
            );
            ui.add_space(6.0);
            let explanation_color = if script.is_enabled || !is_valid {
                TEXT_PRIMARY
            } else {
                TEXT_MUTED
            };
            ui.add(
                Label::new(
                    RichText::new(&status_info.explanation)
                        .size(FONT_SIZE_BODY_PX)
                        .color(explanation_color),
                )
                .wrap(),
            );
            ui.add_space(8.0);
            render_card_metadata_pills(ui, script, runtime_status);
        });

    let bar_color = if is_selected && !status_info.is_prominent {
        ACCENT_PRIMARY
    } else {
        status_info.bar_color
    };
    paint_left_state_bar(ui, frame_response.response.rect, bar_color);
}

fn render_card_header_row(
    ui: &mut Ui,
    script: &ScriptDocument,
    is_valid: bool,
    is_dirty: bool,
    validation_report: &ValidationReport,
    status_info: &crate::ui::active_dashboard::status_classifier::ScriptLiveStatusInfo,
    emitted_actions: &mut Vec<ActiveDashboardAction>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        render_card_enable_controls(ui, script, is_valid, validation_report, emitted_actions);

        let max_title_chars = if !is_valid || ui.available_width() < COMPACT_CARD_WIDTH_THRESHOLD_PX
        {
            MAX_CARD_TITLE_CHARS_COMPACT
        } else {
            MAX_CARD_TITLE_CHARS_WIDE
        };
        let short_name = truncate_display_text(&script.name, max_title_chars);
        let display_name = if is_dirty {
            format!("{short_name} *")
        } else {
            short_name
        };
        let title_response = ui.add(
            Label::new(rich_bold(
                display_name,
                CARD_TITLE_FONT_SIZE_PX,
                TEXT_PRIMARY,
            ))
            .sense(Sense::click()),
        );
        if title_response.clicked() {
            emitted_actions.push(ActiveDashboardAction::SelectScript(script.id.clone()));
        }
        if script.name.chars().count() > max_title_chars {
            title_response.on_hover_text(&script.name);
        }

        Frame::NONE
            .fill(status_info.fill_color)
            .stroke(Stroke::new(1.0, status_info.border_color))
            .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
            .inner_margin(Margin::symmetric(9, 3))
            .show(ui, |ui| {
                ui.label(rich_mono_bold(
                    status_info.card_label,
                    CARD_BADGE_FONT_SIZE_PX,
                    status_info.ink_color,
                ));
            });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let edit_button = build_styled_button(
                rich_semibold("Edit", FONT_SIZE_SMALL_PX, TEXT_PRIMARY),
                SURFACE_HOVER,
                BORDER_STRONG,
                CORNER_RADIUS_SM_PX,
                vec2(58.0, CARD_CONTROL_HEIGHT_PX),
            );
            if ui.add(edit_button).clicked() {
                emitted_actions.push(ActiveDashboardAction::EditScript(script.id.clone()));
            }

            let target_button = build_styled_button(
                rich_semibold("Target...", FONT_SIZE_SMALL_PX, TEXT_PRIMARY),
                SURFACE_ELEVATED,
                BORDER_SUBTLE,
                CORNER_RADIUS_SM_PX,
                vec2(0.0, CARD_CONTROL_HEIGHT_PX),
            );
            if ui.add(target_button).clicked() {
                emitted_actions.push(ActiveDashboardAction::OpenTargetPickerForScript(
                    script.id.clone(),
                ));
            }
        });
    });
}

fn render_card_enable_controls(
    ui: &mut Ui,
    script: &ScriptDocument,
    is_valid: bool,
    validation_report: &ValidationReport,
    emitted_actions: &mut Vec<ActiveDashboardAction>,
) {
    let (toggle_text, toggle_fill, toggle_ink, toggle_border) = if script.is_enabled {
        (
            "ON",
            STATUS_RUNNING_TINT,
            STATUS_RUNNING_INK,
            STATUS_RUNNING,
        )
    } else {
        ("OFF", BG_CANVAS, TEXT_MUTED, BORDER_STRONG)
    };
    let toggle_button = build_styled_button(
        rich_mono_bold(toggle_text, FONT_SIZE_SMALL_PX, toggle_ink),
        toggle_fill,
        toggle_border,
        CORNER_RADIUS_PILL_PX,
        vec2(TOGGLE_BUTTON_MIN_WIDTH_PX, CARD_CONTROL_HEIGHT_PX),
    );
    let can_toggle = is_valid || script.is_enabled;
    let toggle_response = ui.add_enabled(can_toggle, toggle_button);
    if toggle_response.clicked() {
        emitted_actions.push(ActiveDashboardAction::ToggleScriptEnabled {
            script_id: script.id.clone(),
            is_enabled: !script.is_enabled,
        });
    }
    if is_valid {
        let hint = if script.is_enabled {
            "Turn OFF this script"
        } else {
            "Turn ON to arm this script"
        };
        toggle_response.on_hover_text(hint);
        return;
    }

    let first_issue = validation_report
        .issues
        .first()
        .map(|issue| issue.message.as_str())
        .unwrap_or("Script requires setup before enabling.");
    if script.is_enabled {
        toggle_response.on_hover_text(format!(
            "Turn OFF this script (blocked by validation issue: {first_issue})"
        ));
    } else {
        toggle_response.on_disabled_hover_text(format!("Blocked: {first_issue}"));
    }

    let fix_button = build_styled_button(
        rich_semibold("Fix in Editor", FONT_SIZE_SMALL_PX, ACCENT_PRIMARY),
        ACCENT_PRIMARY_TINT,
        ACCENT_PRIMARY,
        CORNER_RADIUS_SM_PX,
        vec2(0.0, CARD_CONTROL_HEIGHT_PX),
    );
    if ui.add(fix_button).clicked() {
        emitted_actions.push(ActiveDashboardAction::EditScript(script.id.clone()));
    }
}

fn render_card_metadata_pills(
    ui: &mut Ui,
    script: &ScriptDocument,
    runtime_status: Option<&ScriptRuntimeStatus>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 5.0);

        let (target_label, target_fill, target_ink, target_border) = match &script.target_scope {
            TargetScope::Global => (
                "Target: Global".to_string(),
                SURFACE_ELEVATED,
                TEXT_MUTED,
                BORDER_SUBTLE,
            ),
            TargetScope::SelectedGame(rule) if rule.has_match_criteria() => (
                format!(
                    "Target: {}",
                    truncate_display_text(&rule.display_name, MAX_TARGET_PILL_CHARS)
                ),
                SURFACE_ELEVATED,
                TEXT_PRIMARY,
                BORDER_SUBTLE,
            ),
            TargetScope::SelectedGame(_) => (
                "Target: Unset".to_string(),
                ACCENT_PRIMARY_TINT,
                ACCENT_PRIMARY,
                ACCENT_PRIMARY,
            ),
        };
        render_badge_pill(ui, &target_label, target_fill, target_ink, target_border);

        let trigger_text = match script.mode {
            ScriptMode::VisualBlocks => format!(
                "Trigger: {}",
                truncate_display_text(
                    &script.visual_program.trigger.display_label(),
                    MAX_TRIGGER_PILL_CHARS,
                )
            ),
            ScriptMode::AdvancedRust => "Trigger: Rust Script".to_string(),
        };
        render_badge_pill(
            ui,
            &trigger_text,
            SURFACE_ELEVATED,
            TEXT_MUTED,
            BORDER_SUBTLE,
        );

        let (kill_label, kill_fill, kill_ink, kill_border) = match script.kill_switch_key {
            Some(key) => (
                format!("Kill-Switch: {}", key.display_label()),
                SURFACE_ELEVATED,
                TEXT_PRIMARY,
                BORDER_SUBTLE,
            ),
            None => (
                "Kill-Switch: Unset".to_string(),
                ACCENT_PRIMARY_TINT,
                ACCENT_PRIMARY,
                ACCENT_PRIMARY,
            ),
        };
        render_badge_pill(ui, &kill_label, kill_fill, kill_ink, kill_border);

        if let Some(arm_key) = script.arm_toggle_key {
            render_badge_pill(
                ui,
                &format!("Arm: {}", arm_key.display_label()),
                SURFACE_ELEVATED,
                TEXT_MUTED,
                BORDER_SUBTLE,
            );
        }

        let mode_label = match script.mode {
            ScriptMode::VisualBlocks => "Mode: Blocks",
            ScriptMode::AdvancedRust => "Mode: Rust",
        };
        render_badge_pill(ui, mode_label, SURFACE_ELEVATED, TEXT_MUTED, BORDER_SUBTLE);

        if runtime_status.is_some_and(|status| status.is_intercept_grab_active) {
            render_badge_pill(
                ui,
                "Intercept Grab Active",
                STATUS_RUNNING_TINT,
                STATUS_RUNNING_INK,
                STATUS_RUNNING,
            );
        }
    });
}
