mod new_script_modal;

use std::collections::HashSet;

use clickforge_sdk::KeyCode;
use egui::{
    Align, Checkbox, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Layout, Margin,
    PointerButton, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

use crate::models::{
    CoordinatorSnapshot, ScriptDocument, ScriptExecutionPhase, TargetScope,
    inspect_script_validation,
};
use crate::storage::AppSettings;
use crate::ui::app::WorkbenchTab;
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BG_CANVAS, BORDER_STRONG, BORDER_SUBTLE,
    CORNER_RADIUS_PILL_PX, CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX, FONT_SIZE_HEADING_PX,
    FONT_SIZE_SMALL_PX, STATUS_DANGER, STATUS_DANGER_INK, STATUS_DANGER_TINT, STATUS_INFO_INK,
    STATUS_RUNNING, STATUS_RUNNING_INK, STATUS_RUNNING_TINT, SURFACE_ELEVATED, SURFACE_HOVER,
    SURFACE_PANEL, TEXT_MUTED, TEXT_PRIMARY, build_styled_button, font_family_mono_bold,
    font_family_semibold, paint_status_dot, phase_status_color, rich_bold, rich_mono,
    rich_mono_bold, rich_semibold, truncate_display_text,
};

pub use new_script_modal::{
    NewScriptModalState, NewScriptTemplateChoice, render_new_script_modal, script_mode_for_template,
};

/// Default sidebar width in logical pixels.
pub const SIDEBAR_DEFAULT_WIDTH_PX: f32 = 246.0;
/// Minimum sidebar width in logical pixels.
pub const SIDEBAR_MIN_WIDTH_PX: f32 = 212.0;
/// Maximum sidebar width in logical pixels.
pub const SIDEBAR_MAX_WIDTH_PX: f32 = 340.0;

const STATUS_DOT_RADIUS_PX: f32 = 4.0;
const BRAND_MARK_SIZE_PX: f32 = 20.0;
const SELECTED_BAR_WIDTH_PX: f32 = 3.0;
const SELECTED_BAR_INSET_Y_PX: f32 = 5.0;
const ROW_ICON_SIZE_PX: f32 = 20.0;
const FOOTER_RESERVED_HEIGHT_PX: f32 = 54.0;
const TAB_SEGMENT_HEIGHT_PX: f32 = 30.0;
const TAB_SEGMENT_GAP_PX: f32 = 3.0;
const TAB_ACTIVE_WIDTH_RATIO: f32 = 0.60;
const TAB_LABEL_FONT_SIZE_PX: f32 = 12.5;
const TAB_BADGE_FONT_SIZE_PX: f32 = 10.0;
const TAB_BADGE_HEIGHT_PX: f32 = 17.0;
const TAB_BADGE_PADDING_X_PX: f32 = 5.0;
const TAB_LABEL_TO_BADGE_GAP_PX: f32 = 5.0;
const TAB_INNER_RADIUS_PX: u8 = CORNER_RADIUS_PILL_PX.saturating_sub(3);
const MAX_SCRIPT_TITLE_CHARS: usize = 16;
const MAX_SUBTITLE_IDLE_CHARS: usize = 18;
const MAX_SUBTITLE_HOVERED_CHARS: usize = 11;
const ROW_HOVER_FILL: Color32 = Color32::from_rgb(0x26, 0x2C, 0x39);

pub(crate) type BadgeColors = (Color32, Color32, Color32);

const BADGE_RUNNING: BadgeColors = (STATUS_RUNNING_TINT, STATUS_RUNNING_INK, STATUS_RUNNING);
pub(crate) const BADGE_ACCENT: BadgeColors = (ACCENT_PRIMARY_TINT, ACCENT_PRIMARY, ACCENT_PRIMARY);
const BADGE_DANGER: BadgeColors = (STATUS_DANGER_TINT, STATUS_DANGER_INK, STATUS_DANGER);
const BADGE_INFO: BadgeColors = (SURFACE_PANEL, STATUS_INFO_INK, BORDER_STRONG);
const BADGE_MUTED: BadgeColors = (SURFACE_PANEL, TEXT_MUTED, BORDER_SUBTLE);
const BADGE_OFF: BadgeColors = (BG_CANVAS, TEXT_MUTED, BORDER_SUBTLE);
pub(crate) const BADGE_HOVER_ACTION: BadgeColors = (SURFACE_HOVER, TEXT_PRIMARY, BORDER_STRONG);

/// User actions emitted by the left script sidebar and New Script modal.
#[derive(Debug, Clone)]
pub enum SidebarAction {
    SelectTab(WorkbenchTab),
    SelectScript(String),
    OpenScriptInEditor(String),
    ToggleScriptEnabled {
        script_id: String,
        is_enabled: bool,
    },
    DuplicateScript(String),
    DeleteScript(String),
    OpenNewScriptModal,
    CreateScript {
        name: String,
        is_selected_game_scope: bool,
        template_choice: NewScriptTemplateChoice,
        kill_switch_key: Option<KeyCode>,
    },
    SetMinimizeToTray(bool),
}

/// Read-only projection passed into `render_sidebar`.
pub struct SidebarViewModel<'frame> {
    pub active_tab: WorkbenchTab,
    pub scripts: &'frame [ScriptDocument],
    pub selected_script_id: Option<&'frame str>,
    pub dirty_script_ids: &'frame HashSet<String>,
    pub snapshot: &'frame CoordinatorSnapshot,
    pub settings: &'frame AppSettings,
    pub has_system_tray: bool,
}

/// Renders the full-height left script rail with Active/Editor tabs and grouped scripts.
pub fn render_sidebar(
    ui: &mut Ui,
    view_model: &SidebarViewModel<'_>,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    render_sidebar_header(ui, view_model, emitted_actions);
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);

    let scroll_height_px = (ui.available_height() - FOOTER_RESERVED_HEIGHT_PX).max(120.0);
    ScrollArea::vertical()
        .id_salt("sidebar_scripts_scroll")
        .max_height(scroll_height_px)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            render_filtered_script_group(ui, "Selected Game", true, view_model, emitted_actions);
            ui.add_space(12.0);
            render_filtered_script_group(ui, "Global", false, view_model, emitted_actions);
        });

    ui.separator();
    ui.add_space(6.0);
    let mut minimize_to_tray = view_model.settings.minimize_to_tray_on_close;
    let tray_checkbox = ui.add_enabled(
        view_model.has_system_tray,
        Checkbox::new(
            &mut minimize_to_tray,
            RichText::new("Minimize to tray on close")
                .size(FONT_SIZE_SMALL_PX)
                .color(TEXT_MUTED),
        ),
    );
    if tray_checkbox.changed() {
        emitted_actions.push(SidebarAction::SetMinimizeToTray(minimize_to_tray));
    }
    if !view_model.has_system_tray {
        tray_checkbox.on_disabled_hover_text(
            "System tray service (StatusNotifierWatcher) was not detected.",
        );
    }
}

fn render_sidebar_header(
    ui: &mut Ui,
    view_model: &SidebarViewModel<'_>,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 7.0;
        let (mark_rect, _) =
            ui.allocate_exact_size(vec2(BRAND_MARK_SIZE_PX, BRAND_MARK_SIZE_PX), Sense::hover());
        let painter = ui.painter();
        painter.rect(
            mark_rect,
            CornerRadius::same(5),
            ACCENT_PRIMARY_TINT,
            Stroke::new(1.0, ACCENT_PRIMARY),
            StrokeKind::Inside,
        );
        let center = mark_rect.center();
        let spark_points = vec![
            pos2(center.x, center.y - 5.0),
            pos2(center.x + 4.5, center.y),
            pos2(center.x, center.y + 5.0),
            pos2(center.x - 4.5, center.y),
        ];
        painter.add(egui::Shape::convex_polygon(
            spark_points,
            ACCENT_PRIMARY,
            Stroke::NONE,
        ));
        ui.label(rich_bold("ClickForge", FONT_SIZE_HEADING_PX, TEXT_PRIMARY));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            render_engine_status_pill(ui, view_model.snapshot);
        });
    });

    ui.add_space(10.0);
    render_workspace_tab_switcher(ui, view_model, emitted_actions);

    ui.add_space(8.0);
    let new_button = build_styled_button(
        rich_semibold("+ New Script", FONT_SIZE_BODY_PX, TEXT_PRIMARY),
        SURFACE_ELEVATED,
        BORDER_STRONG,
        CORNER_RADIUS_SM_PX,
        vec2(ui.available_width(), 32.0),
    );
    if ui.add(new_button).clicked() {
        emitted_actions.push(SidebarAction::OpenNewScriptModal);
    }
}

fn render_workspace_tab_switcher(
    ui: &mut Ui,
    view_model: &SidebarViewModel<'_>,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    let (active_pill_text, active_pill_colors) =
        compute_active_tab_pill(view_model.scripts, view_model.snapshot);

    Frame::NONE
        .fill(BG_CANVAS)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let inner_width_px = ui.available_width();
            let active_width_px =
                ((inner_width_px - TAB_SEGMENT_GAP_PX) * TAB_ACTIVE_WIDTH_RATIO).max(102.0);
            let editor_width_px = (inner_width_px - TAB_SEGMENT_GAP_PX - active_width_px).max(58.0);

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = TAB_SEGMENT_GAP_PX;
                let active_response = render_tab_segment(
                    ui,
                    active_width_px,
                    "Active",
                    Some((&active_pill_text, active_pill_colors)),
                    view_model.active_tab == WorkbenchTab::Active,
                    "Live Control Center showing running, armed, and waiting scripts",
                );
                if active_response.clicked() && view_model.active_tab != WorkbenchTab::Active {
                    emitted_actions.push(SidebarAction::SelectTab(WorkbenchTab::Active));
                }

                let editor_response = render_tab_segment(
                    ui,
                    editor_width_px,
                    "Editor",
                    None,
                    view_model.active_tab == WorkbenchTab::Editor,
                    "Edit Visual Blocks or Advanced Rust code for the selected script",
                );
                if editor_response.clicked() && view_model.active_tab != WorkbenchTab::Editor {
                    emitted_actions.push(SidebarAction::SelectTab(WorkbenchTab::Editor));
                }
            });
        });
}

fn compute_active_tab_pill(
    scripts: &[ScriptDocument],
    snapshot: &CoordinatorSnapshot,
) -> (String, BadgeColors) {
    if snapshot.is_emergency_stopped {
        return ("STOPPED".to_string(), BADGE_DANGER);
    }
    let enabled_count = scripts.iter().filter(|script| script.is_enabled).count();
    if enabled_count == 0 {
        return ("0".to_string(), BADGE_MUTED);
    }

    let has_running_or_armed = scripts
        .iter()
        .filter(|script| script.is_enabled)
        .any(|script| {
            matches!(
                snapshot
                    .status_for_script(&script.id)
                    .map(|status| status.phase),
                Some(ScriptExecutionPhase::Running | ScriptExecutionPhase::Armed)
            )
        });
    if has_running_or_armed {
        return (format!("{enabled_count} ON"), BADGE_RUNNING);
    }

    let has_error = scripts
        .iter()
        .filter(|script| script.is_enabled)
        .any(|script| {
            snapshot
                .status_for_script(&script.id)
                .is_some_and(|status| status.phase == ScriptExecutionPhase::Error)
        });
    if has_error {
        return (format!("{enabled_count} ERR"), BADGE_DANGER);
    }

    (format!("{enabled_count} ON"), BADGE_ACCENT)
}

fn render_tab_segment(
    ui: &mut Ui,
    segment_width_px: f32,
    label_text: &str,
    badge: Option<(&str, BadgeColors)>,
    is_selected: bool,
    tooltip: &str,
) -> egui::Response {
    let (segment_rect, response) = ui.allocate_exact_size(
        vec2(segment_width_px, TAB_SEGMENT_HEIGHT_PX),
        Sense::click(),
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let (bg_fill, border_color, label_ink) = if is_selected {
        (SURFACE_HOVER, ACCENT_PRIMARY, TEXT_PRIMARY)
    } else if response.hovered() {
        (ROW_HOVER_FILL, BORDER_SUBTLE, TEXT_PRIMARY)
    } else {
        (Color32::TRANSPARENT, Color32::TRANSPARENT, TEXT_MUTED)
    };

    let painter = ui.painter();
    let border_stroke = if border_color == Color32::TRANSPARENT {
        Stroke::NONE
    } else {
        Stroke::new(1.0, border_color)
    };
    painter.rect(
        segment_rect,
        CornerRadius::same(TAB_INNER_RADIUS_PX),
        bg_fill,
        border_stroke,
        StrokeKind::Inside,
    );

    let label_font = FontId::new(TAB_LABEL_FONT_SIZE_PX, font_family_semibold());
    let label_galley = painter.layout_no_wrap(label_text.to_string(), label_font, label_ink);
    let center_y_px = segment_rect.center().y;

    if let Some((badge_text, (badge_fill, badge_ink, badge_border))) = badge {
        let badge_font = FontId::new(TAB_BADGE_FONT_SIZE_PX, font_family_mono_bold());
        let badge_galley = painter.layout_no_wrap(badge_text.to_string(), badge_font, badge_ink);
        let badge_width_px = badge_galley.size().x + TAB_BADGE_PADDING_X_PX * 2.0;
        let total_content_width_px =
            label_galley.size().x + TAB_LABEL_TO_BADGE_GAP_PX + badge_width_px;
        let content_start_x_px =
            (segment_rect.center().x - total_content_width_px * 0.5).max(segment_rect.left() + 6.0);

        let label_pos = pos2(
            content_start_x_px,
            center_y_px - label_galley.size().y * 0.5,
        );
        let badge_left_x_px =
            content_start_x_px + label_galley.size().x + TAB_LABEL_TO_BADGE_GAP_PX;
        let badge_rect = Rect::from_min_size(
            pos2(badge_left_x_px, center_y_px - TAB_BADGE_HEIGHT_PX * 0.5),
            vec2(badge_width_px, TAB_BADGE_HEIGHT_PX),
        );
        let badge_text_pos = pos2(
            badge_rect.left() + TAB_BADGE_PADDING_X_PX,
            center_y_px - badge_galley.size().y * 0.5,
        );

        painter.galley(label_pos, label_galley, label_ink);
        painter.rect(
            badge_rect,
            CornerRadius::same(CORNER_RADIUS_SM_PX),
            badge_fill,
            Stroke::new(1.0, badge_border),
            StrokeKind::Inside,
        );
        painter.galley(badge_text_pos, badge_galley, badge_ink);
    } else {
        let label_pos = pos2(
            segment_rect.center().x - label_galley.size().x * 0.5,
            center_y_px - label_galley.size().y * 0.5,
        );
        painter.galley(label_pos, label_galley, label_ink);
    }

    response.on_hover_text(tooltip)
}

fn render_engine_status_pill(ui: &mut Ui, snapshot: &CoordinatorSnapshot) {
    let has_phase = |phase| {
        snapshot
            .script_statuses
            .iter()
            .any(|status| status.phase == phase)
    };
    let input = &snapshot.input_status;

    let (label, colors) = if snapshot.is_emergency_stopped {
        ("STOP", BADGE_DANGER)
    } else if has_phase(ScriptExecutionPhase::Running) {
        ("LIVE", BADGE_RUNNING)
    } else if has_phase(ScriptExecutionPhase::Armed) {
        ("ARMED", BADGE_ACCENT)
    } else if !input.is_evdev_ready || !input.is_uinput_ready {
        ("CHECK", BADGE_DANGER)
    } else {
        ("READY", BADGE_MUTED)
    };

    let evdev_text = if input.is_evdev_ready {
        format!("Ready ({} device(s))", input.monitored_device_count)
    } else {
        input.evdev_error.as_deref().unwrap_or("Unavailable").into()
    };
    let uinput_text = if input.is_uinput_ready {
        "Ready"
    } else {
        input.uinput_error.as_deref().unwrap_or("Unavailable")
    };
    let compositor = if input.compositor_name.is_empty() {
        "Unknown"
    } else {
        &input.compositor_name
    };

    render_mono_badge(
        ui,
        label,
        10.5,
        CORNER_RADIUS_PILL_PX,
        Margin::symmetric(7, 2),
        colors,
    )
    .on_hover_text(format!(
        "evdev: {evdev_text}\nuinput: {uinput_text}\nCompositor: {compositor}"
    ));
}

fn render_filtered_script_group(
    ui: &mut Ui,
    group_title: &str,
    match_selected_game: bool,
    view_model: &SidebarViewModel<'_>,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    let group_scripts: Vec<&ScriptDocument> = view_model
        .scripts
        .iter()
        .filter(|script| {
            matches!(script.target_scope, TargetScope::SelectedGame(_)) == match_selected_game
        })
        .collect();

    ui.horizontal(|ui| {
        ui.label(rich_semibold(group_title, FONT_SIZE_SMALL_PX, TEXT_MUTED));
        ui.label(rich_mono(
            group_scripts.len().to_string(),
            FONT_SIZE_SMALL_PX,
            TEXT_MUTED,
        ));
    });
    ui.add_space(5.0);

    if group_scripts.is_empty() {
        ui.label(
            RichText::new("No scripts in this group")
                .size(FONT_SIZE_SMALL_PX)
                .color(TEXT_MUTED),
        );
        return;
    }

    for script in group_scripts {
        render_script_row(ui, script, view_model, emitted_actions);
        ui.add_space(5.0);
    }
}

fn render_script_row(
    ui: &mut Ui,
    script: &ScriptDocument,
    view_model: &SidebarViewModel<'_>,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    let is_selected = view_model.selected_script_id == Some(script.id.as_str());
    let is_dirty = view_model.dirty_script_ids.contains(&script.id);
    let validation_report = inspect_script_validation(script);
    let is_valid = validation_report.is_valid();
    let runtime_status = view_model.snapshot.status_for_script(&script.id);
    let runtime_phase = runtime_status
        .map(|status| status.phase)
        .unwrap_or(ScriptExecutionPhase::Disabled);
    let is_arm_toggle_off = script.is_enabled
        && script.arm_toggle_key.is_some()
        && !view_model.snapshot.is_emergency_stopped
        && runtime_status.is_some_and(|status| {
            status.phase == ScriptExecutionPhase::Disabled && !status.is_armed_by_toggle
        });

    let rect_memory_id = Id::unique(("sidebar_script_row_rect", script.id.as_str()));
    let is_row_hovered = ui
        .ctx()
        .data(|data| data.get_temp::<Rect>(rect_memory_id))
        .is_some_and(|previous_rect| ui.rect_contains_pointer(previous_rect));

    let (fill_color, border_color) = match (is_selected, is_row_hovered) {
        (true, _) => (SURFACE_HOVER, BORDER_STRONG),
        (false, true) => (ROW_HOVER_FILL, BORDER_STRONG),
        (false, false) => (SURFACE_ELEVATED, BORDER_SUBTLE),
    };
    let first_issue = validation_report
        .issues
        .first()
        .map(|issue| issue.message.as_str())
        .unwrap_or("Script requires setup before enabling.");
    let mut did_click_inner = false;

    let frame_response = Frame::NONE
        .fill(fill_color)
        .stroke(Stroke::new(1.0, border_color))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(9, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                paint_status_dot(ui, phase_status_color(runtime_phase), STATUS_DOT_RADIUS_PX)
                    .on_hover_text(runtime_phase.display_label());
                let short_name = truncate_display_text(&script.name, MAX_SCRIPT_TITLE_CHARS);
                let title = if is_dirty {
                    format!("{short_name} *")
                } else {
                    short_name
                };
                let title_response =
                    ui.label(rich_semibold(title, FONT_SIZE_BODY_PX, TEXT_PRIMARY));
                if script.name.chars().count() > MAX_SCRIPT_TITLE_CHARS {
                    title_response.on_hover_text(&script.name);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mut is_enabled = script.is_enabled;
                    let can_toggle = is_valid || script.is_enabled;
                    let checkbox =
                        ui.add_enabled(can_toggle, Checkbox::without_text(&mut is_enabled));
                    did_click_inner |= checkbox.clicked() || checkbox.changed();
                    if checkbox.changed() {
                        emitted_actions.push(SidebarAction::ToggleScriptEnabled {
                            script_id: script.id.clone(),
                            is_enabled,
                        });
                    }
                    if !can_toggle {
                        checkbox.on_disabled_hover_text(format!("Blocked: {first_issue}"));
                    } else if !is_valid && script.is_enabled {
                        checkbox.on_hover_text(format!(
                            "Disable script (validation issue: {first_issue})"
                        ));
                    } else {
                        checkbox.on_hover_text("Enable or disable this script in background");
                    }
                });
            });
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let (badge_text, badge_colors) = phase_badge_spec(
                    runtime_phase,
                    is_valid,
                    script.is_enabled && view_model.snapshot.is_emergency_stopped,
                    is_arm_toggle_off,
                );
                render_mono_badge(
                    ui,
                    badge_text,
                    10.0,
                    CORNER_RADIUS_SM_PX,
                    Margin::symmetric(5, 1),
                    badge_colors,
                );
                let max_chars = if is_row_hovered {
                    MAX_SUBTITLE_HOVERED_CHARS
                } else {
                    MAX_SUBTITLE_IDLE_CHARS
                };
                let raw_subtitle = match &script.target_scope {
                    TargetScope::Global => script.mode.display_label().to_string(),
                    TargetScope::SelectedGame(_) => script.target_scope.display_label(),
                };
                ui.label(
                    RichText::new(truncate_display_text(&raw_subtitle, max_chars))
                        .size(FONT_SIZE_SMALL_PX)
                        .color(TEXT_MUTED),
                );
                if is_row_hovered {
                    did_click_inner |= render_row_hover_actions(ui, &script.id, emitted_actions);
                }
            });
        });

    let row_rect = frame_response.response.rect;
    ui.ctx()
        .data_mut(|data| data.insert_temp(rect_memory_id, row_rect));

    if is_selected {
        let bar_min = pos2(
            row_rect.left() + 1.0,
            row_rect.top() + SELECTED_BAR_INSET_Y_PX,
        );
        let bar_max = pos2(
            bar_min.x + SELECTED_BAR_WIDTH_PX,
            row_rect.bottom() - SELECTED_BAR_INSET_Y_PX,
        );
        ui.painter().rect_filled(
            Rect::from_min_max(bar_min, bar_max),
            CornerRadius::same(2),
            ACCENT_PRIMARY,
        );
    }

    if ui.rect_contains_pointer(row_rect) && !did_click_inner {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        let (is_double_clicked, is_single_clicked) = ui.ctx().input(|input| {
            (
                input.pointer.button_double_clicked(PointerButton::Primary),
                input.pointer.primary_clicked(),
            )
        });
        if is_double_clicked {
            emitted_actions.push(SidebarAction::OpenScriptInEditor(script.id.clone()));
        } else if is_single_clicked {
            emitted_actions.push(SidebarAction::SelectScript(script.id.clone()));
        }
    }
}

fn render_row_hover_actions(
    ui: &mut Ui,
    script_id: &str,
    emitted_actions: &mut Vec<SidebarAction>,
) -> bool {
    let mut did_click = false;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if render_row_icon_button(ui, true, BADGE_DANGER, "Delete script (5 s Undo)") {
            did_click = true;
            emitted_actions.push(SidebarAction::DeleteScript(script_id.to_string()));
        }
        if render_row_icon_button(ui, false, BADGE_HOVER_ACTION, "Duplicate script") {
            did_click = true;
            emitted_actions.push(SidebarAction::DuplicateScript(script_id.to_string()));
        }
    });
    did_click
}

fn phase_badge_spec(
    phase: ScriptExecutionPhase,
    is_valid: bool,
    is_emergency_stopped: bool,
    is_arm_toggle_off: bool,
) -> (&'static str, BadgeColors) {
    if is_emergency_stopped {
        return ("STOP", BADGE_DANGER);
    }
    if !is_valid {
        return ("SETUP", BADGE_ACCENT);
    }
    if is_arm_toggle_off {
        return ("ARM OFF", BADGE_ACCENT);
    }
    match phase {
        ScriptExecutionPhase::Running => ("RUN", BADGE_RUNNING),
        ScriptExecutionPhase::Armed => ("ARMED", BADGE_RUNNING),
        ScriptExecutionPhase::WaitingForFocus => ("WAIT", BADGE_ACCENT),
        ScriptExecutionPhase::Compiling => ("BUILD", BADGE_INFO),
        ScriptExecutionPhase::StoppedByKillSwitch => ("STOP", BADGE_DANGER),
        ScriptExecutionPhase::Error => ("ERR", BADGE_DANGER),
        ScriptExecutionPhase::Disabled => ("OFF", BADGE_OFF),
    }
}

fn render_mono_badge(
    ui: &mut Ui,
    text: &str,
    font_size_px: f32,
    radius_px: u8,
    margin: Margin,
    (fill, ink, border): BadgeColors,
) -> egui::Response {
    Frame::NONE
        .fill(fill)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(radius_px))
        .inner_margin(margin)
        .show(ui, |ui| ui.label(rich_mono_bold(text, font_size_px, ink)))
        .response
}

fn render_row_icon_button(
    ui: &mut Ui,
    is_delete_icon: bool,
    (hover_fill, hover_ink, hover_border): BadgeColors,
    tooltip: &str,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ROW_ICON_SIZE_PX, ROW_ICON_SIZE_PX), Sense::click());
    let (bg_fill, border, icon_color) = if response.hovered() {
        (hover_fill, hover_border, hover_ink)
    } else {
        (Color32::TRANSPARENT, Color32::TRANSPARENT, TEXT_MUTED)
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(CORNER_RADIUS_SM_PX),
        bg_fill,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    let center = rect.center();
    let stroke = Stroke::new(1.2, icon_color);
    if is_delete_icon {
        painter.line_segment(
            [
                pos2(center.x - 5.0, center.y - 3.5),
                pos2(center.x + 5.0, center.y - 3.5),
            ],
            stroke,
        );
        painter.line_segment(
            [
                pos2(center.x - 2.0, center.y - 5.2),
                pos2(center.x + 2.0, center.y - 5.2),
            ],
            stroke,
        );
        let body = Rect::from_center_size(pos2(center.x, center.y + 1.0), vec2(7.6, 8.0));
        painter.rect_stroke(body, CornerRadius::same(1), stroke, StrokeKind::Inside);
    } else {
        let back = Rect::from_center_size(pos2(center.x - 1.5, center.y - 1.5), vec2(7.0, 7.0));
        let front = Rect::from_center_size(pos2(center.x + 1.5, center.y + 1.5), vec2(7.0, 7.0));
        painter.rect_stroke(back, CornerRadius::same(1), stroke, StrokeKind::Inside);
        painter.rect_filled(front, CornerRadius::same(1), bg_fill);
        painter.rect_stroke(front, CornerRadius::same(1), stroke, StrokeKind::Inside);
    }
    response.on_hover_text(tooltip).clicked()
}
