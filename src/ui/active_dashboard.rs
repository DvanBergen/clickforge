mod script_cards;
mod status_classifier;

use std::collections::HashSet;

use egui::{
    Align, Button, Color32, CornerRadius, Frame, Label, Layout, Margin, Rect, RichText, ScrollArea,
    Stroke, Ui, pos2, vec2,
};

use crate::models::{
    ActiveWindowSnapshot, CoordinatorSnapshot, CustomBlockDefinition, InputSubsystemStatus,
    ScriptDocument,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, BORDER_STRONG, BORDER_SUBTLE, CORNER_RADIUS_MD_PX, CORNER_RADIUS_SM_PX,
    FONT_SIZE_BODY_PX, FONT_SIZE_SMALL_PX, INK_ON_ACCENT, STATUS_DANGER, STATUS_DANGER_INK,
    STATUS_DANGER_SOLID, STATUS_DANGER_TINT, STATUS_RUNNING, STATUS_RUNNING_INK,
    STATUS_RUNNING_TINT, SURFACE_ELEVATED, SURFACE_PANEL, TEXT_MUTED, TEXT_PRIMARY,
    build_styled_button, paint_status_dot, render_badge_pill, rich_bold, rich_mono, rich_semibold,
    truncate_display_text,
};

use script_cards::render_grouped_script_sections;
use status_classifier::{DEFAULT_UINPUT_OFFLINE_HINT, classify_global_engine_state};

pub(crate) use status_classifier::classify_script_live_status;

pub(crate) const HERO_BUTTON_HEIGHT_PX: f32 = 34.0;
const STATE_BAR_WIDTH_PX: f32 = 4.0;
const STATE_BAR_INSET_Y_PX: f32 = 5.0;
const HERO_HEADLINE_FONT_SIZE_PX: f32 = 19.0;
const HERO_DOT_RADIUS_PX: f32 = 5.0;
const STRIP_CONTROL_HEIGHT_PX: f32 = 24.0;
const HERO_WIDE_LAYOUT_THRESHOLD_PX: f32 = 980.0;
const STRIP_WIDE_LAYOUT_THRESHOLD_PX: f32 = 940.0;
const MAX_STRIP_TITLE_CHARS_WIDE: usize = 24;
const MAX_STRIP_TITLE_CHARS_NARROW: usize = 26;
const MAX_FOCUSED_APP_ID_CHARS: usize = 16;

/// User actions emitted by the Active live control center dashboard.
#[derive(Debug, Clone)]
pub enum ActiveDashboardAction {
    SelectScript(String),
    EditScript(String),
    OpenEditorTab,
    OpenNewScriptModal,
    OpenTargetPickerForScript(String),
    ToggleScriptEnabled { script_id: String, is_enabled: bool },
    EmergencyStopAll,
    ClearEmergencyStop,
    DisableAllScripts,
    RefreshFocusedWindow,
}

/// Read-only projection passed into `render_active_dashboard`.
pub struct ActiveDashboardViewModel<'frame> {
    pub scripts: &'frame [ScriptDocument],
    pub custom_blocks: &'frame [CustomBlockDefinition],
    pub selected_script_id: Option<&'frame str>,
    pub dirty_script_ids: &'frame HashSet<String>,
    pub snapshot: &'frame CoordinatorSnapshot,
}

/// Renders the actionable `/dev/uinput` offline warning banner whenever virtual input is unavailable.
pub fn render_uinput_offline_banner(ui: &mut Ui, input_status: &InputSubsystemStatus) {
    if input_status.is_uinput_ready {
        return;
    }
    let diagnostic_detail = input_status
        .uinput_error
        .as_deref()
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .unwrap_or(DEFAULT_UINPUT_OFFLINE_HINT);

    let frame_response = Frame::NONE
        .fill(STATUS_DANGER_TINT)
        .stroke(Stroke::new(1.0, STATUS_DANGER))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin {
            left: 14,
            right: 12,
            top: 8,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                render_badge_pill(
                    ui,
                    "UINPUT OFFLINE",
                    STATUS_DANGER_SOLID,
                    Color32::WHITE,
                    STATUS_DANGER_SOLID,
                );
                ui.label(rich_bold(
                    "Virtual Input (/dev/uinput) Offline: Scripts Cannot Inject Clicks Yet",
                    FONT_SIZE_BODY_PX,
                    STATUS_DANGER_INK,
                ));
            });
            ui.add_space(4.0);
            ui.add(
                Label::new(
                    RichText::new(diagnostic_detail)
                        .size(FONT_SIZE_SMALL_PX)
                        .color(TEXT_PRIMARY),
                )
                .wrap(),
            );
        });

    paint_left_state_bar(ui, frame_response.response.rect, STATUS_DANGER_SOLID);
}

/// Renders the top-level Active Live Control Center dashboard inside the central workspace.
pub fn render_active_dashboard(
    ui: &mut Ui,
    view_model: &ActiveDashboardViewModel<'_>,
    emitted_actions: &mut Vec<ActiveDashboardAction>,
) {
    ScrollArea::vertical()
        .id_salt("active_dashboard_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            Frame::NONE
                .inner_margin(Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    render_global_status_hero_card(ui, view_model, emitted_actions);
                    ui.add_space(10.0);
                    render_target_and_hardware_strip(ui, view_model.snapshot, emitted_actions);
                    ui.add_space(16.0);
                    render_grouped_script_sections(ui, view_model, emitted_actions);
                });
        });
}

fn render_global_status_hero_card(
    ui: &mut Ui,
    view_model: &ActiveDashboardViewModel<'_>,
    emitted_actions: &mut Vec<ActiveDashboardAction>,
) {
    let summary = classify_global_engine_state(view_model.scripts, view_model.snapshot);
    let is_stopped = view_model.snapshot.is_emergency_stopped;
    let is_wide_layout = ui.available_width() >= HERO_WIDE_LAYOUT_THRESHOLD_PX;

    let frame_response = Frame::NONE
        .fill(SURFACE_PANEL)
        .stroke(Stroke::new(1.0, summary.border_color))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
        .inner_margin(Margin {
            left: 18,
            right: 16,
            top: 14,
            bottom: 14,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                paint_status_dot(ui, summary.bar_color, HERO_DOT_RADIUS_PX);
                ui.label(rich_bold(
                    summary.headline,
                    HERO_HEADLINE_FONT_SIZE_PX,
                    summary.headline_color,
                ));
                let enabled_pill = format!(
                    "{} / {} enabled",
                    summary.enabled_count,
                    view_model.scripts.len()
                );
                render_badge_pill(
                    ui,
                    &enabled_pill,
                    SURFACE_ELEVATED,
                    TEXT_MUTED,
                    BORDER_SUBTLE,
                );
                if is_wide_layout {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        render_hero_action_buttons(
                            ui,
                            is_stopped,
                            summary.enabled_count,
                            emitted_actions,
                        );
                    });
                }
            });

            ui.add_space(6.0);
            ui.add(
                Label::new(
                    RichText::new(&summary.subline)
                        .size(FONT_SIZE_BODY_PX)
                        .color(TEXT_PRIMARY),
                )
                .wrap(),
            );

            if !is_wide_layout {
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    render_hero_action_buttons(
                        ui,
                        is_stopped,
                        summary.enabled_count,
                        emitted_actions,
                    );
                });
            }
        });

    paint_left_state_bar(ui, frame_response.response.rect, summary.bar_color);
}

fn render_hero_action_buttons(
    ui: &mut Ui,
    is_emergency_stopped: bool,
    enabled_count: usize,
    emitted_actions: &mut Vec<ActiveDashboardAction>,
) {
    ui.spacing_mut().item_spacing.x = 8.0;

    if is_emergency_stopped {
        let resume_button = build_styled_button(
            rich_bold("Resume Armed Scripts", FONT_SIZE_BODY_PX, INK_ON_ACCENT),
            ACCENT_PRIMARY,
            ACCENT_PRIMARY,
            CORNER_RADIUS_SM_PX,
            vec2(168.0, HERO_BUTTON_HEIGHT_PX),
        );
        if ui
            .add(resume_button)
            .on_hover_text("Clear emergency stop latch so enabled scripts can arm again")
            .clicked()
        {
            emitted_actions.push(ActiveDashboardAction::ClearEmergencyStop);
        }
    } else {
        let stop_button = Button::new(rich_bold("STOP ALL", FONT_SIZE_BODY_PX, Color32::WHITE))
            .fill(STATUS_DANGER_SOLID)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
            .min_size(vec2(102.0, HERO_BUTTON_HEIGHT_PX));
        if ui
            .add(stop_button)
            .on_hover_text("Halt every running script and release all virtual inputs immediately")
            .clicked()
        {
            emitted_actions.push(ActiveDashboardAction::EmergencyStopAll);
        }
    }

    let disable_all_button = build_styled_button(
        rich_semibold(
            "Disable All",
            FONT_SIZE_SMALL_PX,
            if enabled_count > 0 {
                TEXT_PRIMARY
            } else {
                TEXT_MUTED
            },
        ),
        SURFACE_ELEVATED,
        BORDER_STRONG,
        CORNER_RADIUS_SM_PX,
        vec2(0.0, HERO_BUTTON_HEIGHT_PX),
    );
    if ui
        .add_enabled(enabled_count > 0, disable_all_button)
        .on_hover_text("Turn OFF every script and clear emergency stop")
        .clicked()
    {
        emitted_actions.push(ActiveDashboardAction::DisableAllScripts);
    }

    let editor_button = build_styled_button(
        rich_semibold("Open Editor", FONT_SIZE_SMALL_PX, TEXT_PRIMARY),
        SURFACE_ELEVATED,
        BORDER_SUBTLE,
        CORNER_RADIUS_SM_PX,
        vec2(0.0, HERO_BUTTON_HEIGHT_PX),
    );
    if ui.add(editor_button).clicked() {
        emitted_actions.push(ActiveDashboardAction::OpenEditorTab);
    }

    let new_script_button = build_styled_button(
        rich_semibold("+ New Script", FONT_SIZE_SMALL_PX, TEXT_PRIMARY),
        SURFACE_ELEVATED,
        BORDER_SUBTLE,
        CORNER_RADIUS_SM_PX,
        vec2(0.0, HERO_BUTTON_HEIGHT_PX),
    );
    if ui.add(new_script_button).clicked() {
        emitted_actions.push(ActiveDashboardAction::OpenNewScriptModal);
    }
}

fn render_target_and_hardware_strip(
    ui: &mut Ui,
    snapshot: &CoordinatorSnapshot,
    emitted_actions: &mut Vec<ActiveDashboardAction>,
) {
    let is_wide_strip = ui.available_width() >= STRIP_WIDE_LAYOUT_THRESHOLD_PX;
    let max_title_chars = if is_wide_strip {
        MAX_STRIP_TITLE_CHARS_WIDE
    } else {
        MAX_STRIP_TITLE_CHARS_NARROW
    };

    Frame::NONE
        .fill(SURFACE_PANEL)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
        .inner_margin(Margin::symmetric(14, 9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if is_wide_strip {
                ui.horizontal(|ui| {
                    render_focused_window_row_contents(
                        ui,
                        snapshot.focused_window.as_ref(),
                        max_title_chars,
                        emitted_actions,
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        render_hardware_status_badges(ui, &snapshot.input_status);
                    });
                });
            } else {
                ui.horizontal_wrapped(|ui| {
                    render_focused_window_row_contents(
                        ui,
                        snapshot.focused_window.as_ref(),
                        max_title_chars,
                        emitted_actions,
                    );
                });
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    render_hardware_status_badges(ui, &snapshot.input_status);
                });
            }
        });
}

fn render_focused_window_row_contents(
    ui: &mut Ui,
    focused_window: Option<&ActiveWindowSnapshot>,
    max_title_chars: usize,
    emitted_actions: &mut Vec<ActiveDashboardAction>,
) {
    ui.spacing_mut().item_spacing.x = 8.0;
    ui.label(rich_semibold(
        "Focused Window",
        FONT_SIZE_SMALL_PX,
        TEXT_MUTED,
    ));
    render_focused_window_summary(ui, focused_window, max_title_chars);
    let refresh_button = build_styled_button(
        rich_semibold("Refresh", FONT_SIZE_SMALL_PX, TEXT_PRIMARY),
        SURFACE_ELEVATED,
        BORDER_SUBTLE,
        CORNER_RADIUS_SM_PX,
        vec2(0.0, STRIP_CONTROL_HEIGHT_PX),
    );
    if ui
        .add(refresh_button)
        .on_hover_text("Query compositor for current focused window and process list")
        .clicked()
    {
        emitted_actions.push(ActiveDashboardAction::RefreshFocusedWindow);
    }
}

fn render_focused_window_summary(
    ui: &mut Ui,
    focused_window: Option<&ActiveWindowSnapshot>,
    max_title_chars: usize,
) {
    let Some(window) = focused_window.filter(|entry| !entry.is_empty()) else {
        ui.label(
            RichText::new("None detected")
                .size(FONT_SIZE_SMALL_PX)
                .color(TEXT_MUTED),
        );
        return;
    };

    let display_title = if window.title.trim().is_empty() {
        window.app_id.trim()
    } else {
        window.title.trim()
    };
    let short_title = truncate_display_text(display_title, max_title_chars);
    let title_response = ui.label(rich_semibold(short_title, FONT_SIZE_BODY_PX, TEXT_PRIMARY));
    if display_title.chars().count() > max_title_chars {
        title_response.on_hover_text(display_title);
    }

    let trimmed_app_id = window.app_id.trim();
    if !trimmed_app_id.is_empty() {
        let short_app_id = truncate_display_text(trimmed_app_id, MAX_FOCUSED_APP_ID_CHARS);
        let app_id_response = ui.label(rich_mono(short_app_id, FONT_SIZE_SMALL_PX, TEXT_MUTED));
        if trimmed_app_id.chars().count() > MAX_FOCUSED_APP_ID_CHARS {
            app_id_response.on_hover_text(trimmed_app_id);
        }
    }
    if let Some(pid) = window.pid {
        ui.label(rich_mono(
            format!("PID {pid}"),
            FONT_SIZE_SMALL_PX,
            TEXT_MUTED,
        ));
    }
}

fn render_hardware_status_badges(ui: &mut Ui, input_status: &InputSubsystemStatus) {
    ui.spacing_mut().item_spacing.x = 6.0;

    let compositor_name = if input_status.compositor_name.trim().is_empty() {
        "Unknown"
    } else {
        input_status.compositor_name.trim()
    };
    render_badge_pill(
        ui,
        &format!("Compositor: {compositor_name}"),
        SURFACE_ELEVATED,
        TEXT_MUTED,
        BORDER_SUBTLE,
    );

    if input_status.is_uinput_ready {
        render_badge_pill(
            ui,
            "uinput: Ready",
            STATUS_RUNNING_TINT,
            STATUS_RUNNING_INK,
            STATUS_RUNNING,
        );
    } else {
        let detail = input_status
            .uinput_error
            .as_deref()
            .unwrap_or(DEFAULT_UINPUT_OFFLINE_HINT);
        Frame::NONE
            .fill(STATUS_DANGER_TINT)
            .stroke(Stroke::new(1.0, STATUS_DANGER))
            .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
            .inner_margin(Margin::symmetric(6, 2))
            .show(ui, |ui| {
                ui.label(rich_semibold(
                    "uinput: Offline",
                    FONT_SIZE_SMALL_PX,
                    STATUS_DANGER_INK,
                ));
            })
            .response
            .on_hover_text(detail);
    }

    if input_status.is_evdev_ready {
        let device_count = input_status.monitored_device_count;
        let device_word = if device_count == 1 {
            "device"
        } else {
            "devices"
        };
        render_badge_pill(
            ui,
            &format!("evdev: {device_count} {device_word}"),
            SURFACE_ELEVATED,
            TEXT_PRIMARY,
            BORDER_SUBTLE,
        );
    } else {
        let detail = input_status
            .evdev_error
            .as_deref()
            .unwrap_or("Physical input devices (/dev/input/event*) are unavailable.");
        Frame::NONE
            .fill(STATUS_DANGER_TINT)
            .stroke(Stroke::new(1.0, STATUS_DANGER))
            .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
            .inner_margin(Margin::symmetric(6, 2))
            .show(ui, |ui| {
                ui.label(rich_semibold(
                    "evdev: Offline",
                    FONT_SIZE_SMALL_PX,
                    STATUS_DANGER_INK,
                ));
            })
            .response
            .on_hover_text(detail);
    }
}

pub(crate) fn paint_left_state_bar(ui: &mut Ui, card_rect: Rect, bar_color: Color32) {
    let bar_min = pos2(
        card_rect.left() + 1.0,
        card_rect.top() + STATE_BAR_INSET_Y_PX,
    );
    let bar_max = pos2(
        bar_min.x + STATE_BAR_WIDTH_PX,
        (card_rect.bottom() - STATE_BAR_INSET_Y_PX).max(bar_min.y + 4.0),
    );
    ui.painter().rect_filled(
        Rect::from_min_max(bar_min, bar_max),
        CornerRadius::same(2),
        bar_color,
    );
}
