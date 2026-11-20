use std::collections::HashSet;

use clickforge_sdk::KeyCode;
use egui::{
    Align, Checkbox, Color32, CornerRadius, CursorIcon, Frame, Id, Key, Layout, Margin, Modal,
    Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Theme, Ui, pos2, vec2,
};

use crate::models::{
    CoordinatorSnapshot, ScriptDocument, ScriptExecutionPhase, ScriptMode, TargetScope,
    inspect_script_validation,
};
use crate::storage::AppSettings;
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BG_CANVAS, BORDER_STRONG, BORDER_SUBTLE,
    CORNER_RADIUS_MD_PX, CORNER_RADIUS_PILL_PX, CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX,
    FONT_SIZE_HEADING_PX, FONT_SIZE_SMALL_PX, INK_ON_ACCENT, STATUS_DANGER, STATUS_DANGER_INK,
    STATUS_DANGER_TINT, STATUS_INFO_INK, STATUS_RUNNING, STATUS_RUNNING_INK, STATUS_RUNNING_TINT,
    SURFACE_ELEVATED, SURFACE_HOVER, SURFACE_PANEL, TEXT_MUTED, TEXT_PRIMARY, build_styled_button,
    paint_status_dot, phase_status_color, render_badge_pill, rich_bold, rich_mono, rich_mono_bold,
    rich_semibold, truncate_display_text,
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
const MAX_SCRIPT_TITLE_CHARS: usize = 16;
const MAX_SUBTITLE_IDLE_CHARS: usize = 18;
const MAX_SUBTITLE_HOVERED_CHARS: usize = 11;
const NEW_SCRIPT_MODAL_WIDTH_PX: f32 = 468.0;
const ROW_HOVER_FILL: Color32 = Color32::from_rgb(0x26, 0x2C, 0x39);

type BadgeColors = (Color32, Color32, Color32);

const BADGE_RUNNING: BadgeColors = (STATUS_RUNNING_TINT, STATUS_RUNNING_INK, STATUS_RUNNING);
const BADGE_ACCENT: BadgeColors = (ACCENT_PRIMARY_TINT, ACCENT_PRIMARY, ACCENT_PRIMARY);
const BADGE_DANGER: BadgeColors = (STATUS_DANGER_TINT, STATUS_DANGER_INK, STATUS_DANGER);
const BADGE_INFO: BadgeColors = (SURFACE_PANEL, STATUS_INFO_INK, BORDER_STRONG);
const BADGE_MUTED: BadgeColors = (SURFACE_PANEL, TEXT_MUTED, BORDER_SUBTLE);
const BADGE_OFF: BadgeColors = (BG_CANVAS, TEXT_MUTED, BORDER_SUBTLE);
const BADGE_HOVER_ACTION: BadgeColors = (SURFACE_HOVER, TEXT_PRIMARY, BORDER_STRONG);

const KILL_SWITCH_QUICK_PICKS: [(Option<KeyCode>, &str); 5] = [
    (Some(KeyCode::F8), "F8"),
    (Some(KeyCode::Pause), "Pause"),
    (Some(KeyCode::ScrollLock), "ScrollLock"),
    (Some(KeyCode::F9), "F9"),
    (None, "Bind Later"),
];

const TEMPLATE_CHOICES: [(NewScriptTemplateChoice, &str); 3] = [
    (NewScriptTemplateChoice::VisualBlocks, "Visual Blocks"),
    (NewScriptTemplateChoice::AdvancedRust, "Advanced Rust"),
    (
        NewScriptTemplateChoice::BuildingUpgradeTemplate,
        "Building Upgrade Template",
    ),
];

/// Authoring template option selected inside the New Script modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NewScriptTemplateChoice {
    #[default]
    VisualBlocks,
    AdvancedRust,
    BuildingUpgradeTemplate,
}

/// Transient form state for the New Script creation modal.
#[derive(Debug, Clone, Default)]
pub struct NewScriptModalState {
    pub is_open: bool,
    pub script_name: String,
    pub is_selected_game_scope: bool,
    pub template_choice: NewScriptTemplateChoice,
    pub selected_kill_switch_key: Option<KeyCode>,
    should_focus_name_input: bool,
}

impl NewScriptModalState {
    /// Resets form fields with no auto-filled Kill-Switch and opens the modal dialog.
    pub fn open_with_defaults(&mut self, existing_script_count: usize) {
        self.script_name = format!("New Script {}", existing_script_count.saturating_add(1));
        self.is_selected_game_scope = true;
        self.template_choice = NewScriptTemplateChoice::VisualBlocks;
        self.selected_kill_switch_key = None;
        self.should_focus_name_input = true;
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
    pub scripts: &'frame [ScriptDocument],
    pub selected_script_id: Option<&'frame str>,
    pub dirty_script_ids: &'frame HashSet<String>,
    pub snapshot: &'frame CoordinatorSnapshot,
    pub settings: &'frame AppSettings,
    pub has_system_tray: bool,
}

/// Renders the full-height left script rail grouped into Selected Game and Global sections.
pub fn render_sidebar(
    ui: &mut Ui,
    view_model: &SidebarViewModel<'_>,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    render_sidebar_header(ui, view_model.snapshot, emitted_actions);
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
    snapshot: &CoordinatorSnapshot,
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
            render_engine_status_pill(ui, snapshot);
        });
    });

    ui.add_space(10.0);
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
    let runtime_phase = view_model
        .snapshot
        .status_for_script(&script.id)
        .map(|status| status.phase)
        .unwrap_or(ScriptExecutionPhase::Disabled);

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
                    let checkbox =
                        ui.add_enabled(is_valid, Checkbox::without_text(&mut is_enabled));
                    did_click_inner |= checkbox.clicked() || checkbox.changed();
                    if checkbox.changed() {
                        emitted_actions.push(SidebarAction::ToggleScriptEnabled {
                            script_id: script.id.clone(),
                            is_enabled,
                        });
                    }
                    if !is_valid {
                        checkbox.on_disabled_hover_text(format!("Blocked: {first_issue}"));
                    } else {
                        checkbox.on_hover_text("Enable or disable this script in background");
                    }
                });
            });
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let (badge_text, badge_colors) = phase_badge_spec(runtime_phase, is_valid);
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
        if ui.ctx().input(|input| input.pointer.primary_clicked()) {
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

fn phase_badge_spec(phase: ScriptExecutionPhase, is_valid: bool) -> (&'static str, BadgeColors) {
    if !is_valid {
        return ("SETUP", BADGE_ACCENT);
    }
    match phase {
        ScriptExecutionPhase::Running => ("RUN", BADGE_RUNNING),
        ScriptExecutionPhase::Armed => ("ARMED", BADGE_ACCENT),
        ScriptExecutionPhase::WaitingForFocus => ("WAIT", BADGE_MUTED),
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

/// Renders the modal dialog for creating a new script with explicit Kill-Switch selection.
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
        .shadow(egui_context.style_of(Theme::Dark).visuals.window_shadow)
        .inner_margin(Margin::same(18));

    let modal_response = Modal::new(Id::unique("new_script_modal"))
        .frame(modal_frame)
        .show(egui_context, |ui| {
            ui.set_width(NEW_SCRIPT_MODAL_WIDTH_PX);
            ui.label(rich_bold(
                "Create New Script",
                FONT_SIZE_HEADING_PX,
                TEXT_PRIMARY,
            ));
            render_modal_field_label(ui, "Script Name");
            let name_response = ui.add(
                TextEdit::singleline(&mut modal_state.script_name)
                    .desired_width(f32::INFINITY)
                    .hint_text("Enter script name"),
            );
            if modal_state.should_focus_name_input {
                name_response.request_focus();
                modal_state.should_focus_name_input = false;
            }

            render_modal_field_label(ui, "Execution Scope");
            ui.horizontal(|ui| {
                for (is_game, label_text) in
                    [(true, "Selected Game Only"), (false, "Global (Any Window)")]
                {
                    if render_choice_chip(
                        ui,
                        label_text,
                        modal_state.is_selected_game_scope == is_game,
                    ) {
                        modal_state.is_selected_game_scope = is_game;
                    }
                }
            });

            render_modal_field_label(ui, "Starting Mode");
            ui.horizontal(|ui| {
                for (option, label_text) in TEMPLATE_CHOICES {
                    if render_choice_chip(ui, label_text, modal_state.template_choice == option) {
                        modal_state.template_choice = option;
                    }
                }
            });

            ui.add_space(12.0);
            render_kill_switch_quick_picker(ui, &mut modal_state.selected_kill_switch_key);

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(10.0);
            render_new_script_footer_buttons(ui, modal_state, emitted_actions);
        });

    if modal_response.should_close() {
        modal_state.is_open = false;
    }
}

fn render_modal_field_label(ui: &mut Ui, label_text: &str) {
    ui.add_space(10.0);
    ui.label(rich_semibold(label_text, FONT_SIZE_SMALL_PX, TEXT_MUTED));
    ui.add_space(3.0);
}

fn render_kill_switch_quick_picker(ui: &mut Ui, selected_kill_switch_key: &mut Option<KeyCode>) {
    ui.horizontal(|ui| {
        ui.label(rich_semibold(
            "Hard Kill-Switch Key",
            FONT_SIZE_SMALL_PX,
            TEXT_MUTED,
        ));
        render_badge_pill(
            ui,
            "Required before Save / Enable",
            SURFACE_ELEVATED,
            TEXT_MUTED,
            BORDER_SUBTLE,
        );
    });
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (candidate_key, chip_label) in KILL_SWITCH_QUICK_PICKS {
            let is_selected = *selected_kill_switch_key == candidate_key;
            let (fill, ink, border) = match (is_selected, candidate_key.is_some()) {
                (true, true) => BADGE_ACCENT,
                (true, false) => BADGE_HOVER_ACTION,
                (false, _) => (SURFACE_ELEVATED, TEXT_MUTED, BORDER_SUBTLE),
            };
            let label = if candidate_key.is_some() {
                rich_mono_bold(chip_label, FONT_SIZE_SMALL_PX, ink)
            } else {
                rich_semibold(chip_label, FONT_SIZE_SMALL_PX, ink)
            };
            if ui
                .add(build_styled_button(
                    label,
                    fill,
                    border,
                    CORNER_RADIUS_PILL_PX,
                    vec2(0.0, 26.0),
                ))
                .clicked()
            {
                *selected_kill_switch_key = candidate_key;
            }
        }
    });

    ui.add_space(4.0);
    let (helper_text, helper_color) = match selected_kill_switch_key {
        Some(chosen_key) => (
            format!(
                "Pressing {} immediately stops all scripts and releases every input.",
                chosen_key.display_label()
            ),
            STATUS_RUNNING_INK,
        ),
        None => (
            "No Kill-Switch selected. Save, Compile, and Enable stay blocked until one is bound."
                .to_string(),
            TEXT_MUTED,
        ),
    };
    ui.label(
        RichText::new(helper_text)
            .size(FONT_SIZE_SMALL_PX)
            .color(helper_color),
    );
}

fn render_choice_chip(ui: &mut Ui, label_text: &str, is_selected: bool) -> bool {
    let (fill, border, label) = if is_selected {
        (
            SURFACE_HOVER,
            ACCENT_PRIMARY,
            rich_semibold(label_text, FONT_SIZE_SMALL_PX, TEXT_PRIMARY),
        )
    } else {
        (
            SURFACE_ELEVATED,
            BORDER_SUBTLE,
            RichText::new(label_text)
                .size(FONT_SIZE_SMALL_PX)
                .color(TEXT_MUTED),
        )
    };
    ui.add(build_styled_button(
        label,
        fill,
        border,
        CORNER_RADIUS_SM_PX,
        vec2(0.0, 26.0),
    ))
    .clicked()
}

fn render_new_script_footer_buttons(
    ui: &mut Ui,
    modal_state: &mut NewScriptModalState,
    emitted_actions: &mut Vec<SidebarAction>,
) {
    let trimmed_name = modal_state.script_name.trim().to_string();
    let can_create = !trimmed_name.is_empty();
    let mut should_submit = can_create && ui.ctx().input(|input| input.key_pressed(Key::Enter));

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let (fill, ink) = if can_create {
            (ACCENT_PRIMARY, INK_ON_ACCENT)
        } else {
            (SURFACE_ELEVATED, TEXT_MUTED)
        };
        let create = build_styled_button(
            rich_semibold("Create Script", FONT_SIZE_BODY_PX, ink),
            fill,
            Color32::TRANSPARENT,
            CORNER_RADIUS_SM_PX,
            vec2(110.0, 30.0),
        );
        should_submit |= ui.add_enabled(can_create, create).clicked();
        let cancel = build_styled_button(
            rich_semibold("Cancel", FONT_SIZE_BODY_PX, TEXT_PRIMARY),
            SURFACE_ELEVATED,
            BORDER_SUBTLE,
            CORNER_RADIUS_SM_PX,
            vec2(74.0, 30.0),
        );
        if ui.add(cancel).clicked() {
            modal_state.is_open = false;
        }
    });

    if should_submit {
        emitted_actions.push(SidebarAction::CreateScript {
            name: trimmed_name,
            is_selected_game_scope: modal_state.is_selected_game_scope,
            template_choice: modal_state.template_choice,
            kill_switch_key: modal_state.selected_kill_switch_key,
        });
        modal_state.is_open = false;
    }
}

/// Maps `NewScriptTemplateChoice` to its underlying `ScriptMode`.
pub fn script_mode_for_template(template_choice: NewScriptTemplateChoice) -> ScriptMode {
    match template_choice {
        NewScriptTemplateChoice::VisualBlocks
        | NewScriptTemplateChoice::BuildingUpgradeTemplate => ScriptMode::VisualBlocks,
        NewScriptTemplateChoice::AdvancedRust => ScriptMode::AdvancedRust,
    }
}
