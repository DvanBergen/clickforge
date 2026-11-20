use egui::{
    Align, Align2, Button, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Key, Layout,
    Margin, Modal, Modifiers, Painter, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind,
    TextEdit, Ui, pos2, vec2,
};

use crate::models::{
    ActiveWindowSnapshot, GameTargetRule, RunningProcessEntry, TargetMatchMode,
    correlate_process_for_window,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BORDER_STRONG, BORDER_SUBTLE, CORNER_RADIUS_MD_PX,
    CORNER_RADIUS_PILL_PX, CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX, FONT_SIZE_HEADING_PX,
    FONT_SIZE_SMALL_PX, INK_ON_ACCENT, STATUS_RUNNING_INK, STATUS_RUNNING_TINT, SURFACE_ELEVATED,
    SURFACE_HOVER, SURFACE_PANEL, TEXT_MUTED, TEXT_PRIMARY, font_family_semibold, rich_bold,
    rich_semibold, truncate_display_text,
};

const PICKER_MODAL_WIDTH_PX: f32 = 780.0;
const PICKER_LIST_HEIGHT_PX: f32 = 340.0;
const WINDOW_ROW_HEIGHT_PX: f32 = 54.0;
const PROCESS_ROW_HEIGHT_PX: f32 = 52.0;
const ROW_GAP_Y_PX: f32 = 4.0;
const ACCENT_BAR_WIDTH_PX: f32 = 3.0;
const ROW_PADDING_X_PX: f32 = 12.0;
const BADGE_PADDING_X_PX: f32 = 7.0;
const BADGE_HEIGHT_PX: f32 = 19.0;
const EMPTY_MIN_HEIGHT_PX: f32 = 110.0;
const MAX_TITLE_CHARS: usize = 54;
const MAX_COMMAND_CHARS: usize = 84;
const SELF_APP_ID_LOWER: &str = "clickforge";
const SELF_APP_TOOLTIP: &str =
    "This is ClickForge itself. Avoid binding an automated clicker here.";

/// Active tab inside the Cheat Engine style target picker dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetPickerTab {
    #[default]
    WindowsAndGames,
    Processes,
}

/// Persistent UI state for the target picker modal.
#[derive(Debug, Clone, Default)]
pub struct TargetPickerState {
    pub is_open: bool,
    pub active_tab: TargetPickerTab,
    pub search_query: String,
    pub match_mode: TargetMatchMode,
    pub selected_window_identity: Option<WindowSelectionIdentity>,
    pub selected_process_pid: Option<u32>,
    pub is_games_only_enabled: bool,
    should_focus_search: bool,
}

/// Stable identity key used to retain window selection across live refreshes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSelectionIdentity {
    pub window_id: Option<u64>,
    pub app_id: String,
    pub title: String,
}

impl WindowSelectionIdentity {
    fn from_snapshot(window: &ActiveWindowSnapshot) -> Self {
        Self {
            window_id: window.window_id,
            app_id: window.app_id.clone(),
            title: window.title.clone(),
        }
    }
}

impl TargetPickerState {
    /// Opens the modal, clears any stale search query, and schedules search input focus.
    pub fn open(&mut self) {
        self.is_open = true;
        self.search_query.clear();
        self.should_focus_search = true;
    }

    fn close(&mut self) {
        self.is_open = false;
        self.should_focus_search = false;
    }
}

/// Actions emitted when the user interacts with the target picker modal.
#[derive(Debug, Clone)]
pub enum TargetPickerAction {
    RefreshTargets,
    AttachTargetRule(GameTargetRule),
    SetScopeGlobal,
}

type BadgePalette = (Color32, Color32, Color32);

const BADGE_ACCENT: BadgePalette = (ACCENT_PRIMARY_TINT, ACCENT_PRIMARY, ACCENT_PRIMARY);
const BADGE_RUNNING: BadgePalette = (STATUS_RUNNING_TINT, STATUS_RUNNING_INK, STATUS_RUNNING_INK);
const BADGE_MUTED: BadgePalette = (SURFACE_PANEL, TEXT_MUTED, BORDER_SUBTLE);

enum PickerRowSource<'slice> {
    Window(&'slice ActiveWindowSnapshot),
    Process(&'slice RunningProcessEntry),
}

struct PickerRowItem<'slice> {
    title: String,
    subtitle: String,
    sort_key: (bool, bool, bool, String, u32),
    badges: Vec<(String, BadgePalette)>,
    is_self_app: bool,
    hover_tooltip: Option<String>,
    source: PickerRowSource<'slice>,
}

impl PickerRowItem<'_> {
    fn is_selected(&self, state: &TargetPickerState) -> bool {
        match self.source {
            PickerRowSource::Window(window) => {
                let identity = WindowSelectionIdentity::from_snapshot(window);
                state.selected_window_identity.as_ref() == Some(&identity)
            }
            PickerRowSource::Process(process) => state.selected_process_pid == Some(process.pid),
        }
    }

    fn select_in(&self, state: &mut TargetPickerState) {
        match self.source {
            PickerRowSource::Window(window) => {
                state.selected_window_identity =
                    Some(WindowSelectionIdentity::from_snapshot(window));
            }
            PickerRowSource::Process(process) => state.selected_process_pid = Some(process.pid),
        }
    }

    fn build_rule(
        &self,
        processes: &[RunningProcessEntry],
        mode: TargetMatchMode,
    ) -> GameTargetRule {
        match self.source {
            PickerRowSource::Window(window) => {
                GameTargetRule::from_window_and_processes(window, processes, mode)
            }
            PickerRowSource::Process(process) => GameTargetRule::from_process(process, mode),
        }
    }
}

/// Renders the 2-tab Cheat Engine style target picker modal when open.
pub fn render_target_picker_modal(
    egui_context: &egui::Context,
    picker_state: &mut TargetPickerState,
    open_windows: &[ActiveWindowSnapshot],
    running_processes: &[RunningProcessEntry],
    emitted_actions: &mut Vec<TargetPickerAction>,
) {
    if !picker_state.is_open {
        return;
    }

    let (is_escape, is_refresh, is_enter, step_delta) = egui_context.input_mut(|input| {
        let down = input.consume_key(Modifiers::NONE, Key::ArrowDown);
        let up = input.consume_key(Modifiers::NONE, Key::ArrowUp);
        let delta = i8::from(down) - i8::from(up);
        let escape = input.consume_key(Modifiers::NONE, Key::Escape);
        let refresh = input.consume_key(Modifiers::NONE, Key::F5);
        (
            escape,
            refresh,
            input.key_pressed(Key::Enter),
            delta as isize,
        )
    });
    if is_escape {
        picker_state.close();
        return;
    }
    if is_refresh {
        emitted_actions.push(TargetPickerAction::RefreshTargets);
    }

    let active_rows = build_active_tab_rows(picker_state, open_windows, running_processes);
    if step_delta != 0 && !active_rows.is_empty() {
        step_row_selection(picker_state, &active_rows, step_delta);
    }

    let modal_frame = Frame::NONE
        .fill(SURFACE_PANEL)
        .stroke(Stroke::new(1.0, BORDER_STRONG))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
        .shadow(
            egui_context
                .style_of(egui::Theme::Dark)
                .visuals
                .window_shadow,
        )
        .inner_margin(Margin::same(16));

    let modal_response = Modal::new(Id::unique("target_picker_modal"))
        .frame(modal_frame)
        .show(egui_context, |ui| {
            ui.set_width(PICKER_MODAL_WIDTH_PX);
            let counts = (open_windows.len(), running_processes.len());
            render_top_bar(ui, picker_state, counts.0, counts.1, emitted_actions);
            ui.add_space(8.0);

            let double_clicked = render_row_list(
                ui,
                picker_state,
                &active_rows,
                running_processes,
                emitted_actions,
            );
            let mut candidate = active_rows
                .iter()
                .find(|row| row.is_selected(picker_state))
                .map(|row| row.build_rule(running_processes, picker_state.match_mode));

            let confirmed =
                double_clicked.or_else(|| if is_enter { candidate.take() } else { None });
            if let Some(attached_rule) = confirmed {
                emitted_actions.push(TargetPickerAction::AttachTargetRule(attached_rule));
                picker_state.close();
                return;
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);
            render_bottom_controls(ui, picker_state, candidate, emitted_actions);
        });

    if modal_response.should_close() {
        picker_state.close();
    }
}

fn step_row_selection(state: &mut TargetPickerState, rows: &[PickerRowItem<'_>], delta: isize) {
    let last_index = rows.len().saturating_sub(1);
    let next_index = match rows.iter().position(|row| row.is_selected(state)) {
        None if delta >= 0 => 0,
        None => last_index,
        Some(index) if delta >= 0 => index.saturating_add(1).min(last_index),
        Some(index) => index.saturating_sub(1),
    };
    rows[next_index].select_in(state);
}

fn render_top_bar(
    ui: &mut Ui,
    picker_state: &mut TargetPickerState,
    window_count: usize,
    process_count: usize,
    emitted_actions: &mut Vec<TargetPickerAction>,
) {
    ui.horizontal(|ui| {
        ui.label(rich_bold(
            "Attach Process / Window",
            FONT_SIZE_HEADING_PX,
            TEXT_PRIMARY,
        ));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let close = ui.button("Close").on_hover_text("Close dialog (Esc)");
            if close.clicked() {
                picker_state.close();
            }
            let refresh_tooltip = "Rescan open compositor windows and /proc processes (F5)";
            let refresh = ui.button("Refresh").on_hover_text(refresh_tooltip);
            if refresh.clicked() {
                emitted_actions.push(TargetPickerAction::RefreshTargets);
            }
        });
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let windows_label = format!("Windows / Games ({window_count})");
        let processes_label = format!("Processes ({process_count})");
        for (tab_kind, label_text) in [
            (TargetPickerTab::WindowsAndGames, windows_label),
            (TargetPickerTab::Processes, processes_label),
        ] {
            let is_active = picker_state.active_tab == tab_kind;
            let (fill, border, ink) = if is_active {
                (ACCENT_PRIMARY, ACCENT_PRIMARY, INK_ON_ACCENT)
            } else {
                (SURFACE_ELEVATED, BORDER_SUBTLE, TEXT_PRIMARY)
            };
            let button = Button::new(rich_semibold(label_text, FONT_SIZE_BODY_PX, ink))
                .fill(fill)
                .stroke(Stroke::new(1.0, border));
            if ui.add(button).clicked() {
                picker_state.active_tab = tab_kind;
            }
        }

        ui.add_space(6.0);
        let is_games_only = picker_state.is_games_only_enabled;
        let (fill, ink, border) = if is_games_only {
            BADGE_ACCENT
        } else {
            (SURFACE_ELEVATED, TEXT_MUTED, BORDER_SUBTLE)
        };
        let games_chip = Button::new(rich_semibold("Games Only", FONT_SIZE_SMALL_PX, ink))
            .fill(fill)
            .stroke(Stroke::new(1.0, border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX));
        let games_tooltip = "Filter to Steam, Proton, Wine, and Windows .exe targets";
        if ui.add(games_chip).on_hover_text(games_tooltip).clicked() {
            picker_state.is_games_only_enabled = !is_games_only;
        }

        ui.add_space(4.0);
        let search = TextEdit::singleline(&mut picker_state.search_query)
            .id_salt("target_picker_search_input")
            .desired_width(ui.available_width())
            .hint_text("Filter title, app_id, .exe, Steam ID, or PID...");
        let response = ui.add(search);
        if picker_state.should_focus_search {
            response.request_focus();
            picker_state.should_focus_search = false;
        }
    });
}

fn render_row_list(
    ui: &mut Ui,
    picker_state: &mut TargetPickerState,
    active_rows: &[PickerRowItem<'_>],
    running_processes: &[RunningProcessEntry],
    emitted_actions: &mut Vec<TargetPickerAction>,
) -> Option<GameTargetRule> {
    let is_windows_tab = picker_state.active_tab == TargetPickerTab::WindowsAndGames;
    if active_rows.is_empty() {
        render_empty_state(ui, picker_state, is_windows_tab, emitted_actions);
        return None;
    }

    let (scroll_salt, row_height_px) = if is_windows_tab {
        ("target_picker_windows_scroll", WINDOW_ROW_HEIGHT_PX)
    } else {
        ("target_picker_processes_scroll", PROCESS_ROW_HEIGHT_PX)
    };
    let mut double_clicked_rule: Option<GameTargetRule> = None;

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP_Y_PX;
        ScrollArea::vertical()
            .id_salt(scroll_salt)
            .max_height(PICKER_LIST_HEIGHT_PX)
            .auto_shrink([false, false])
            .show_rows(ui, row_height_px, active_rows.len(), |ui, row_range| {
                for row_item in &active_rows[row_range] {
                    let is_selected = row_item.is_selected(picker_state);
                    let response = paint_picker_row(ui, row_item, row_height_px, is_selected);
                    if response.clicked() {
                        row_item.select_in(picker_state);
                    }
                    if response.double_clicked() {
                        let mode = picker_state.match_mode;
                        double_clicked_rule = Some(row_item.build_rule(running_processes, mode));
                    }
                }
            });
    });

    double_clicked_rule
}

fn paint_picker_row(
    ui: &mut Ui,
    row_item: &PickerRowItem<'_>,
    row_height_px: f32,
    is_selected: bool,
) -> egui::Response {
    let row_size = vec2(ui.available_width().max(120.0), row_height_px);
    let (row_rect, response) = ui.allocate_exact_size(row_size, Sense::click());
    let is_hovered = response.hovered();
    let (fill, border) = if is_selected {
        (SURFACE_HOVER, ACCENT_PRIMARY)
    } else if is_hovered {
        (SURFACE_HOVER, BORDER_STRONG)
    } else if row_item.is_self_app {
        (SURFACE_PANEL, BORDER_SUBTLE)
    } else {
        (SURFACE_ELEVATED, BORDER_SUBTLE)
    };

    let viewport_clip = ui.clip_rect();
    let painter = ui
        .painter()
        .with_clip_rect(row_rect.intersect(viewport_clip));
    let radius = CornerRadius::same(CORNER_RADIUS_SM_PX);
    let row_stroke = Stroke::new(1.0, border);
    painter.rect(row_rect, radius, fill, row_stroke, StrokeKind::Inside);
    if is_selected {
        let bar_left_x_px = row_rect.min.x + 1.0;
        let bar_rect = Rect::from_min_max(
            pos2(bar_left_x_px, row_rect.min.y + 4.0),
            pos2(bar_left_x_px + ACCENT_BAR_WIDTH_PX, row_rect.max.y - 4.0),
        );
        painter.rect_filled(bar_rect, CornerRadius::same(2), ACCENT_PRIMARY);
    }

    let top_y_px = row_rect.min.y + 17.5;
    let bottom_y_px = row_rect.max.y - 14.5;
    let badges_width_px = paint_right_badges(&painter, row_rect, top_y_px, &row_item.badges);
    let left_x_px = row_rect.min.x + ROW_PADDING_X_PX;
    let title_max_x_px =
        (row_rect.max.x - ROW_PADDING_X_PX - badges_width_px - 10.0).max(left_x_px + 40.0);
    let title_bounds = Rect::from_min_max(
        pos2(left_x_px, row_rect.min.y),
        pos2(title_max_x_px, row_rect.max.y),
    );
    let title_ink = if row_item.is_self_app {
        TEXT_MUTED
    } else {
        TEXT_PRIMARY
    };
    let title_font = FontId::new(FONT_SIZE_BODY_PX, font_family_semibold());
    let title_origin = pos2(left_x_px, top_y_px);
    ui.painter()
        .with_clip_rect(title_bounds.intersect(viewport_clip))
        .text(
            title_origin,
            Align2::LEFT_CENTER,
            &row_item.title,
            title_font,
            title_ink,
        );

    let subtitle_font = FontId::monospace(FONT_SIZE_SMALL_PX);
    let subtitle_origin = pos2(left_x_px, bottom_y_px);
    painter.text(
        subtitle_origin,
        Align2::LEFT_CENTER,
        &row_item.subtitle,
        subtitle_font,
        TEXT_MUTED,
    );

    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    if let Some(tooltip_text) = row_item.hover_tooltip.as_deref() {
        return response.on_hover_text(tooltip_text);
    }
    response
}

fn paint_right_badges(
    painter: &Painter,
    row_rect: Rect,
    center_y_px: f32,
    badges: &[(String, BadgePalette)],
) -> f32 {
    if badges.is_empty() {
        return 0.0;
    }
    let badge_font = FontId::new(FONT_SIZE_SMALL_PX - 1.0, font_family_semibold());
    let start_right_x_px = row_rect.max.x - ROW_PADDING_X_PX;
    let mut cursor_x_px = start_right_x_px;
    let radius = CornerRadius::same(CORNER_RADIUS_SM_PX);
    let half_height_px = BADGE_HEIGHT_PX * 0.5;

    for (label_text, (fill, ink, border)) in badges {
        let galley = painter.layout_no_wrap(label_text.clone(), badge_font.clone(), *ink);
        let pill_width_px = galley.size().x + BADGE_PADDING_X_PX * 2.0;
        let min_point = pos2(cursor_x_px - pill_width_px, center_y_px - half_height_px);
        let max_point = pos2(cursor_x_px, center_y_px + half_height_px);
        let pill_rect = Rect::from_min_max(min_point, max_point);
        let stroke = Stroke::new(1.0, *border);
        painter.rect(pill_rect, radius, *fill, stroke, StrokeKind::Inside);
        let text_x_px = pill_rect.min.x + BADGE_PADDING_X_PX;
        let text_y_px = center_y_px - galley.size().y * 0.5;
        painter.galley(pos2(text_x_px, text_y_px), galley, *ink);
        cursor_x_px -= pill_width_px + 6.0;
    }
    (start_right_x_px - cursor_x_px).max(0.0)
}

fn render_empty_state(
    ui: &mut Ui,
    picker_state: &mut TargetPickerState,
    is_windows_tab: bool,
    emitted_actions: &mut Vec<TargetPickerAction>,
) {
    let has_filters =
        picker_state.is_games_only_enabled || !picker_state.search_query.trim().is_empty();
    let (switch_label, alternate_tab) = if is_windows_tab {
        ("Switch to Processes Tab", TargetPickerTab::Processes)
    } else {
        ("Switch to Windows Tab", TargetPickerTab::WindowsAndGames)
    };
    let message = if has_filters {
        "No targets match your active search or Games Only filter."
    } else if is_windows_tab {
        "No open compositor windows detected."
    } else {
        "No running processes detected."
    };

    Frame::NONE
        .fill(SURFACE_ELEVATED)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_min_size(vec2(ui.available_width(), EMPTY_MIN_HEIGHT_PX));
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                let label = RichText::new(message)
                    .size(FONT_SIZE_BODY_PX)
                    .color(TEXT_MUTED);
                ui.label(label);
                ui.add_space(12.0);
                let row_width_px = if has_filters { 390.0 } else { 280.0 };
                ui.horizontal(|ui| {
                    ui.add_space(((ui.available_width() - row_width_px) * 0.5).max(0.0));
                    if render_action_chip(ui, "Refresh List", false) {
                        emitted_actions.push(TargetPickerAction::RefreshTargets);
                    }
                    if render_action_chip(ui, switch_label, false) {
                        picker_state.active_tab = alternate_tab;
                    }
                    if has_filters && render_action_chip(ui, "Clear Filters", true) {
                        picker_state.is_games_only_enabled = false;
                        picker_state.search_query.clear();
                        picker_state.should_focus_search = true;
                    }
                });
            });
        });
}

fn render_action_chip(ui: &mut Ui, label_text: &str, is_accent: bool) -> bool {
    let (fill, ink, border) = if is_accent {
        BADGE_ACCENT
    } else {
        (SURFACE_PANEL, TEXT_PRIMARY, BORDER_STRONG)
    };
    let button = Button::new(rich_semibold(label_text, FONT_SIZE_SMALL_PX, ink))
        .fill(fill)
        .stroke(Stroke::new(1.0, border));
    ui.add(button).clicked()
}

fn render_bottom_controls(
    ui: &mut Ui,
    picker_state: &mut TargetPickerState,
    candidate_rule: Option<GameTargetRule>,
    emitted_actions: &mut Vec<TargetPickerAction>,
) {
    ui.horizontal_wrapped(|ui| {
        let heading = rich_semibold("Matching Strategy:", FONT_SIZE_SMALL_PX, TEXT_MUTED);
        ui.label(heading);
        let mode = &mut picker_state.match_mode;
        let persistent = "Persistent: App/Exe + Window Title";
        ui.radio_value(mode, TargetMatchMode::PersistentAppAndTitle, persistent);
        ui.radio_value(
            mode,
            TargetMatchMode::StrictPid,
            "Strict PID (Current Instance Only)",
        );
    });
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let global_button =
            Button::new(RichText::new("Switch to Global Scope").size(FONT_SIZE_SMALL_PX))
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, BORDER_SUBTLE));
        if ui.add(global_button).clicked() {
            emitted_actions.push(TargetPickerAction::SetScopeGlobal);
            picker_state.close();
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let attach_text =
                rich_semibold("Attach Selected Target", FONT_SIZE_BODY_PX, INK_ON_ACCENT);
            let attach_button = Button::new(attach_text)
                .fill(ACCENT_PRIMARY)
                .stroke(Stroke::NONE);
            let response = ui
                .add_enabled(candidate_rule.is_some(), attach_button)
                .on_hover_text("Attach the highlighted target (Enter)");
            if response.clicked()
                && let Some(selected_rule) = candidate_rule
            {
                emitted_actions.push(TargetPickerAction::AttachTargetRule(selected_rule));
                picker_state.close();
            }
            if ui.button("Cancel").clicked() {
                picker_state.close();
            }
        });
    });
}

fn build_active_tab_rows<'slice>(
    picker_state: &TargetPickerState,
    open_windows: &'slice [ActiveWindowSnapshot],
    running_processes: &'slice [RunningProcessEntry],
) -> Vec<PickerRowItem<'slice>> {
    let query_lower = picker_state.search_query.trim().to_ascii_lowercase();
    let filter = (
        query_lower.as_str(),
        picker_state.is_games_only_enabled,
        std::process::id(),
    );
    let mut rows: Vec<PickerRowItem<'slice>> = match picker_state.active_tab {
        TargetPickerTab::WindowsAndGames => open_windows
            .iter()
            .filter_map(|window| build_window_item(window, running_processes, filter))
            .collect(),
        TargetPickerTab::Processes => running_processes
            .iter()
            .filter_map(|process| build_process_item(process, filter))
            .collect(),
    };
    rows.sort_by(|left, right| left.sort_key.cmp(&right.sort_key));
    rows
}

fn build_window_item<'slice>(
    window: &'slice ActiveWindowSnapshot,
    running_processes: &'slice [RunningProcessEntry],
    (query_lower, games_only, current_pid): (&str, bool, u32),
) -> Option<PickerRowItem<'slice>> {
    let correlated = correlate_process_for_window(window, running_processes);
    let steam_app_id = window
        .extracted_steam_app_id()
        .map(ToString::to_string)
        .or_else(|| correlated.and_then(|entry| entry.steam_app_id.clone()));
    let is_steam_or_proton =
        steam_app_id.is_some() || correlated.is_some_and(|entry| entry.is_wine_or_proton);
    let is_self_app = window.pid == Some(current_pid)
        || window.app_id.eq_ignore_ascii_case(SELF_APP_ID_LOWER)
        || correlated.is_some_and(|entry| is_self_process(entry, current_pid));
    let app_lower = window.app_id.to_ascii_lowercase();
    let is_game_candidate = !is_self_app
        && (is_steam_or_proton
            || app_lower.starts_with("steam_app_")
            || app_lower == "gamescope"
            || correlated.is_some_and(is_game_process));
    if games_only && !is_game_candidate {
        return None;
    }

    let pid_string = correlated
        .map(|entry| entry.pid.to_string())
        .or_else(|| window.pid.map(|pid| pid.to_string()))
        .unwrap_or_else(|| "-".to_string());
    let executable = correlated.map_or("-", |entry| entry.executable_name.as_str());
    let steam_text = steam_app_id.as_deref().unwrap_or("");
    let fields = [
        window.title.as_str(),
        window.app_id.as_str(),
        executable,
        &pid_string,
        steam_text,
    ];
    if !matches_any_field(&fields, query_lower) {
        return None;
    }

    let mut badges: Vec<(String, BadgePalette)> = Vec::with_capacity(3);
    if is_self_app {
        badges.push(("This App".to_string(), BADGE_MUTED));
    }
    if window.is_focused {
        badges.push(("Focused".to_string(), BADGE_RUNNING));
    }
    if is_steam_or_proton {
        badges.push(("Steam/Proton".to_string(), BADGE_ACCENT));
    }

    let app_id = if window.app_id.trim().is_empty() {
        "none"
    } else {
        &window.app_id
    };
    let steam_suffix = steam_app_id
        .as_deref()
        .map_or(String::new(), |id| format!("  |  Steam: {id}"));
    let long_title = (window.title.chars().count() > MAX_TITLE_CHARS).then(|| window.title.clone());
    let hover_tooltip = if is_self_app {
        Some(SELF_APP_TOOLTIP.to_string())
    } else {
        long_title
    };
    let raw_title = if window.title.trim().is_empty() {
        "(Untitled Window)"
    } else {
        &window.title
    };
    let key_title = window.title.to_ascii_lowercase();
    let window_pid = window.pid.unwrap_or(0);
    let subtitle =
        format!("app_id: {app_id}  |  exe: {executable}  |  PID: {pid_string}{steam_suffix}");
    Some(PickerRowItem {
        title: truncate_display_text(raw_title, MAX_TITLE_CHARS),
        subtitle,
        sort_key: (
            is_self_app,
            !is_game_candidate,
            !window.is_focused,
            key_title,
            window_pid,
        ),
        badges,
        is_self_app,
        hover_tooltip,
        source: PickerRowSource::Window(window),
    })
}

fn build_process_item<'slice>(
    process: &'slice RunningProcessEntry,
    (query_lower, games_only, current_pid): (&str, bool, u32),
) -> Option<PickerRowItem<'slice>> {
    let is_self_app = is_self_process(process, current_pid);
    let is_game_candidate = !is_self_app && is_game_process(process);
    if games_only && !is_game_candidate {
        return None;
    }
    let pid_string = process.pid.to_string();
    let steam_text = process.steam_app_id.as_deref().unwrap_or("");
    let fields = [
        process.executable_name.as_str(),
        process.process_name.as_str(),
        process.command_line.as_str(),
        &pid_string,
        steam_text,
    ];
    if !matches_any_field(&fields, query_lower) {
        return None;
    }

    let mut badges: Vec<(String, BadgePalette)> = Vec::with_capacity(3);
    if is_self_app {
        badges.push(("This App".to_string(), BADGE_MUTED));
    }
    badges.push((format!("PID {pid_string}"), BADGE_MUTED));
    if let Some(steam_id) = &process.steam_app_id {
        badges.push((format!("Steam {steam_id}"), BADGE_ACCENT));
    } else if process.is_wine_or_proton {
        badges.push(("Steam/Proton".to_string(), BADGE_ACCENT));
    }

    let command_tooltip = (!process.command_line.is_empty()).then(|| process.command_line.clone());
    let hover_tooltip = is_self_app
        .then(|| SELF_APP_TOOLTIP.to_string())
        .or(command_tooltip);
    let key_name = process.executable_name.to_ascii_lowercase();
    let sort_key = (is_self_app, !is_game_candidate, true, key_name, process.pid);
    Some(PickerRowItem {
        title: truncate_display_text(&process.executable_name, MAX_TITLE_CHARS),
        subtitle: truncate_display_text(&process.command_line, MAX_COMMAND_CHARS),
        sort_key,
        badges,
        is_self_app,
        hover_tooltip,
        source: PickerRowSource::Process(process),
    })
}

fn is_self_process(process: &RunningProcessEntry, current_pid: u32) -> bool {
    process.pid == current_pid
        || process
            .executable_name
            .eq_ignore_ascii_case(SELF_APP_ID_LOWER)
        || process.process_name.eq_ignore_ascii_case(SELF_APP_ID_LOWER)
}

fn is_game_process(process: &RunningProcessEntry) -> bool {
    process.is_wine_or_proton
        || process.steam_app_id.is_some()
        || process
            .executable_name
            .to_ascii_lowercase()
            .ends_with(".exe")
}

fn matches_any_field(fields: &[&str], query_lower: &str) -> bool {
    query_lower.is_empty()
        || fields
            .iter()
            .any(|field| field.to_ascii_lowercase().contains(query_lower))
}
