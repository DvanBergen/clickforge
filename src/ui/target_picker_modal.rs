use egui::{
    Align, Button, Color32, CornerRadius, Frame, Id, Layout, Margin, Modal, RichText, ScrollArea,
    Sense, Stroke, TextEdit, Ui, vec2,
};

use crate::models::{
    ActiveWindowSnapshot, GameTargetRule, RunningProcessEntry, TargetMatchMode,
    correlate_process_for_window,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BORDER_STRONG, BORDER_SUBTLE, CORNER_RADIUS_MD_PX,
    CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX, FONT_SIZE_HEADING_PX, FONT_SIZE_SMALL_PX,
    INK_ON_ACCENT, STATUS_RUNNING, STATUS_RUNNING_TINT, SURFACE_ELEVATED, SURFACE_HOVER,
    SURFACE_PANEL, TEXT_MUTED, TEXT_PRIMARY, render_badge_pill, truncate_display_text,
};

const PICKER_MODAL_WIDTH_PX: f32 = 760.0;
const PICKER_LIST_HEIGHT_PX: f32 = 340.0;
const PROCESS_ROW_HEIGHT_PX: f32 = 44.0;
const WINDOW_ROW_HEIGHT_PX: f32 = 46.0;
const MAX_TITLE_CHARS: usize = 52;
const MAX_CMDLINE_CHARS: usize = 78;

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
    /// Opens the modal and clears any stale search query.
    pub fn open(&mut self) {
        self.is_open = true;
        self.search_query.clear();
    }
}

/// Actions emitted when the user interacts with the target picker modal.
#[derive(Debug, Clone)]
pub enum TargetPickerAction {
    RefreshTargets,
    AttachTargetRule(GameTargetRule),
    SetScopeGlobal,
}

struct CorrelatedWindowRow<'slice> {
    window: &'slice ActiveWindowSnapshot,
    correlated_process: Option<&'slice RunningProcessEntry>,
    steam_app_id: Option<String>,
    is_steam_or_proton: bool,
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

    let modal_frame = Frame::NONE
        .fill(SURFACE_PANEL)
        .stroke(Stroke::new(1.0, BORDER_STRONG))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
        .inner_margin(Margin::same(16));

    let modal_response = Modal::new(Id::unique("target_picker_modal"))
        .frame(modal_frame)
        .show(egui_context, |ui| {
            ui.set_width(PICKER_MODAL_WIDTH_PX);
            render_picker_header(ui, picker_state, emitted_actions);
            ui.add_space(8.0);
            render_picker_tabs_and_filter(
                ui,
                picker_state,
                open_windows.len(),
                running_processes.len(),
                emitted_actions,
            );
            ui.add_space(8.0);

            let double_clicked_rule = match picker_state.active_tab {
                TargetPickerTab::WindowsAndGames => {
                    render_windows_tab(ui, picker_state, open_windows, running_processes)
                }
                TargetPickerTab::Processes => {
                    render_processes_tab(ui, picker_state, running_processes)
                }
            };

            if let Some(attached_rule) = double_clicked_rule {
                emitted_actions.push(TargetPickerAction::AttachTargetRule(attached_rule));
                picker_state.is_open = false;
                return;
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);
            render_match_strategy_selector(ui, &mut picker_state.match_mode);
            ui.add_space(10.0);
            render_picker_footer(
                ui,
                picker_state,
                open_windows,
                running_processes,
                emitted_actions,
            );
        });

    if modal_response.should_close() {
        picker_state.is_open = false;
    }
}

fn render_picker_header(
    ui: &mut Ui,
    picker_state: &mut TargetPickerState,
    emitted_actions: &mut Vec<TargetPickerAction>,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Attach Process / Window")
                .size(FONT_SIZE_HEADING_PX)
                .strong()
                .color(TEXT_PRIMARY),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Close").clicked() {
                picker_state.is_open = false;
            }
            if ui
                .button("Refresh")
                .on_hover_text("Rescan open compositor windows and /proc processes")
                .clicked()
            {
                emitted_actions.push(TargetPickerAction::RefreshTargets);
            }
        });
    });
}

fn render_picker_tabs_and_filter(
    ui: &mut Ui,
    picker_state: &mut TargetPickerState,
    window_count: usize,
    process_count: usize,
    emitted_actions: &mut Vec<TargetPickerAction>,
) {
    ui.horizontal(|ui| {
        render_tab_button(
            ui,
            &format!("Windows / Games ({window_count})"),
            picker_state.active_tab == TargetPickerTab::WindowsAndGames,
            || {
                picker_state.active_tab = TargetPickerTab::WindowsAndGames;
            },
        );
        render_tab_button(
            ui,
            &format!("Processes ({process_count})"),
            picker_state.active_tab == TargetPickerTab::Processes,
            || {
                picker_state.active_tab = TargetPickerTab::Processes;
            },
        );

        ui.add_space(12.0);
        let search_response = ui.add(
            TextEdit::singleline(&mut picker_state.search_query)
                .desired_width(ui.available_width())
                .hint_text("Filter by window title, app_id, .exe, Steam App ID, or PID..."),
        );
        if search_response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::F5)) {
            emitted_actions.push(TargetPickerAction::RefreshTargets);
        }
    });
}

fn render_tab_button(ui: &mut Ui, label_text: &str, is_active: bool, on_select: impl FnOnce()) {
    let tab_button = Button::new(
        RichText::new(label_text)
            .size(FONT_SIZE_BODY_PX)
            .strong()
            .color(if is_active {
                INK_ON_ACCENT
            } else {
                TEXT_PRIMARY
            }),
    )
    .fill(if is_active {
        ACCENT_PRIMARY
    } else {
        SURFACE_ELEVATED
    })
    .stroke(Stroke::new(
        1.0,
        if is_active {
            ACCENT_PRIMARY
        } else {
            BORDER_SUBTLE
        },
    ));

    if ui.add(tab_button).clicked() {
        on_select();
    }
}

fn render_windows_tab(
    ui: &mut Ui,
    picker_state: &mut TargetPickerState,
    open_windows: &[ActiveWindowSnapshot],
    running_processes: &[RunningProcessEntry],
) -> Option<GameTargetRule> {
    let query_lower = picker_state.search_query.trim().to_ascii_lowercase();
    let mut window_rows = build_sorted_window_rows(open_windows, running_processes, &query_lower);

    if window_rows.is_empty() {
        render_empty_picker_state(
            ui,
            "No matching windows found. Click Refresh or switch to the Processes tab.",
        );
        return None;
    }

    let mut double_clicked_rule: Option<GameTargetRule> = None;

    ScrollArea::vertical()
        .id_salt("target_picker_windows_scroll")
        .max_height(PICKER_LIST_HEIGHT_PX)
        .auto_shrink([false, false])
        .show_rows(
            ui,
            WINDOW_ROW_HEIGHT_PX,
            window_rows.len(),
            |ui, row_range| {
                for row_entry in window_rows.drain(row_range.start..row_range.end) {
                    let row_identity = WindowSelectionIdentity::from_snapshot(row_entry.window);
                    let is_selected =
                        picker_state.selected_window_identity.as_ref() == Some(&row_identity);

                    let row_response = render_window_row(ui, &row_entry, is_selected);
                    if row_response.clicked() {
                        picker_state.selected_window_identity = Some(row_identity);
                    }
                    if row_response.double_clicked() {
                        double_clicked_rule = Some(GameTargetRule::from_window_and_processes(
                            row_entry.window,
                            running_processes,
                            picker_state.match_mode,
                        ));
                    }
                }
            },
        );

    double_clicked_rule
}

fn render_window_row(
    ui: &mut Ui,
    row_entry: &CorrelatedWindowRow<'_>,
    is_selected: bool,
) -> egui::Response {
    let fill_color = if is_selected {
        SURFACE_HOVER
    } else {
        SURFACE_ELEVATED
    };
    let border_color = if is_selected {
        ACCENT_PRIMARY
    } else {
        BORDER_SUBTLE
    };

    let frame_response = Frame::NONE
        .fill(fill_color)
        .stroke(Stroke::new(1.0, border_color))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if row_entry.is_steam_or_proton {
                    render_badge_pill(
                        ui,
                        "Steam/Proton",
                        ACCENT_PRIMARY_TINT,
                        ACCENT_PRIMARY,
                        ACCENT_PRIMARY,
                    );
                }
                if row_entry.window.is_focused {
                    render_badge_pill(
                        ui,
                        "Focused",
                        STATUS_RUNNING_TINT,
                        STATUS_RUNNING,
                        STATUS_RUNNING,
                    );
                }

                let title_text = if row_entry.window.title.trim().is_empty() {
                    "(Untitled Window)".to_string()
                } else {
                    truncate_display_text(&row_entry.window.title, MAX_TITLE_CHARS)
                };
                ui.label(
                    RichText::new(title_text)
                        .size(FONT_SIZE_BODY_PX)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
            });

            ui.horizontal(|ui| {
                let app_id_display = if row_entry.window.app_id.trim().is_empty() {
                    "none".to_string()
                } else {
                    row_entry.window.app_id.clone()
                };
                let exe_display = row_entry
                    .correlated_process
                    .map(|process| process.executable_name.as_str())
                    .unwrap_or("-");
                let pid_display = row_entry
                    .correlated_process
                    .map(|process| process.pid.to_string())
                    .or_else(|| row_entry.window.pid.map(|pid| pid.to_string()))
                    .unwrap_or_else(|| "-".to_string());

                let mut metadata = format!(
                    "app_id: {app_id_display}   |   exe: {exe_display}   |   PID: {pid_display}"
                );
                if let Some(steam_id) = &row_entry.steam_app_id {
                    metadata.push_str(&format!("   |   Steam: {steam_id}"));
                }
                ui.label(
                    RichText::new(metadata)
                        .size(FONT_SIZE_SMALL_PX)
                        .monospace()
                        .color(TEXT_MUTED),
                );
            });
        });

    ui.add_space(3.0);
    frame_response.response.interact(Sense::click())
}

fn render_processes_tab(
    ui: &mut Ui,
    picker_state: &mut TargetPickerState,
    running_processes: &[RunningProcessEntry],
) -> Option<GameTargetRule> {
    let query_lower = picker_state.search_query.trim().to_ascii_lowercase();
    let mut filtered_processes: Vec<&RunningProcessEntry> = running_processes
        .iter()
        .filter(|process| matches_process_query(process, &query_lower))
        .collect();

    filtered_processes.sort_by(|left_process, right_process| {
        let left_is_game = left_process.is_wine_or_proton || left_process.steam_app_id.is_some();
        let right_is_game = right_process.is_wine_or_proton || right_process.steam_app_id.is_some();
        right_is_game
            .cmp(&left_is_game)
            .then_with(|| {
                left_process
                    .executable_name
                    .to_ascii_lowercase()
                    .cmp(&right_process.executable_name.to_ascii_lowercase())
            })
            .then_with(|| left_process.pid.cmp(&right_process.pid))
    });

    if filtered_processes.is_empty() {
        render_empty_picker_state(ui, "No running processes match your filter.");
        return None;
    }

    let mut double_clicked_rule: Option<GameTargetRule> = None;

    ScrollArea::vertical()
        .id_salt("target_picker_processes_scroll")
        .max_height(PICKER_LIST_HEIGHT_PX)
        .auto_shrink([false, false])
        .show_rows(
            ui,
            PROCESS_ROW_HEIGHT_PX,
            filtered_processes.len(),
            |ui, row_range| {
                for process_entry in &filtered_processes[row_range] {
                    let is_selected = picker_state.selected_process_pid == Some(process_entry.pid);
                    let row_response = render_process_row(ui, process_entry, is_selected);
                    if row_response.clicked() {
                        picker_state.selected_process_pid = Some(process_entry.pid);
                    }
                    if row_response.double_clicked() {
                        double_clicked_rule = Some(GameTargetRule::from_process(
                            process_entry,
                            picker_state.match_mode,
                        ));
                    }
                }
            },
        );

    double_clicked_rule
}

fn render_process_row(
    ui: &mut Ui,
    process_entry: &RunningProcessEntry,
    is_selected: bool,
) -> egui::Response {
    let fill_color = if is_selected {
        SURFACE_HOVER
    } else {
        SURFACE_ELEVATED
    };
    let border_color = if is_selected {
        ACCENT_PRIMARY
    } else {
        BORDER_SUBTLE
    };

    let frame_response = Frame::NONE
        .fill(fill_color)
        .stroke(Stroke::new(1.0, border_color))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if process_entry.is_wine_or_proton || process_entry.steam_app_id.is_some() {
                    render_badge_pill(
                        ui,
                        "Steam/Proton",
                        ACCENT_PRIMARY_TINT,
                        ACCENT_PRIMARY,
                        ACCENT_PRIMARY,
                    );
                }
                ui.label(
                    RichText::new(&process_entry.executable_name)
                        .size(FONT_SIZE_BODY_PX)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                ui.label(
                    RichText::new(format!("PID {}", process_entry.pid))
                        .size(FONT_SIZE_SMALL_PX)
                        .monospace()
                        .color(TEXT_MUTED),
                );
                if let Some(steam_id) = &process_entry.steam_app_id {
                    ui.label(
                        RichText::new(format!("SteamAppId {steam_id}"))
                            .size(FONT_SIZE_SMALL_PX)
                            .monospace()
                            .color(ACCENT_PRIMARY),
                    );
                }
            });

            let cmdline_preview =
                truncate_display_text(&process_entry.command_line, MAX_CMDLINE_CHARS);
            ui.label(
                RichText::new(cmdline_preview)
                    .size(FONT_SIZE_SMALL_PX)
                    .monospace()
                    .color(TEXT_MUTED),
            );
        });

    ui.add_space(3.0);
    frame_response.response.interact(Sense::click())
}

fn render_empty_picker_state(ui: &mut Ui, message: &str) {
    Frame::NONE
        .fill(SURFACE_ELEVATED)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_min_size(vec2(ui.available_width(), 96.0));
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(message).color(TEXT_MUTED));
            });
        });
}

fn render_match_strategy_selector(ui: &mut Ui, match_mode: &mut TargetMatchMode) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new("Matching Strategy:")
                .size(FONT_SIZE_SMALL_PX)
                .strong()
                .color(TEXT_MUTED),
        );
        ui.radio_value(
            match_mode,
            TargetMatchMode::PersistentAppAndTitle,
            "Persistent: App/Exe + Window Title",
        );
        ui.radio_value(
            match_mode,
            TargetMatchMode::StrictPid,
            "Strict PID (Current Instance Only)",
        );
    });
}

fn render_picker_footer(
    ui: &mut Ui,
    picker_state: &mut TargetPickerState,
    open_windows: &[ActiveWindowSnapshot],
    running_processes: &[RunningProcessEntry],
    emitted_actions: &mut Vec<TargetPickerAction>,
) {
    let candidate_rule =
        resolve_selected_target_rule(picker_state, open_windows, running_processes);

    ui.horizontal(|ui| {
        if ui
            .add(
                Button::new(RichText::new("Switch to Global Scope").size(FONT_SIZE_SMALL_PX))
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .clicked()
        {
            emitted_actions.push(TargetPickerAction::SetScopeGlobal);
            picker_state.is_open = false;
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let attach_button = Button::new(
                RichText::new("Attach Selected Target")
                    .strong()
                    .color(INK_ON_ACCENT),
            )
            .fill(ACCENT_PRIMARY)
            .stroke(Stroke::NONE);

            if ui
                .add_enabled(candidate_rule.is_some(), attach_button)
                .clicked()
                && let Some(selected_rule) = candidate_rule
            {
                emitted_actions.push(TargetPickerAction::AttachTargetRule(selected_rule));
                picker_state.is_open = false;
            }

            if ui.button("Cancel").clicked() {
                picker_state.is_open = false;
            }
        });
    });
}

fn resolve_selected_target_rule(
    picker_state: &TargetPickerState,
    open_windows: &[ActiveWindowSnapshot],
    running_processes: &[RunningProcessEntry],
) -> Option<GameTargetRule> {
    match picker_state.active_tab {
        TargetPickerTab::WindowsAndGames => {
            let selected_identity = picker_state.selected_window_identity.as_ref()?;
            let matched_window = open_windows.iter().find(|window| {
                WindowSelectionIdentity::from_snapshot(window) == *selected_identity
            })?;
            Some(GameTargetRule::from_window_and_processes(
                matched_window,
                running_processes,
                picker_state.match_mode,
            ))
        }
        TargetPickerTab::Processes => {
            let selected_pid = picker_state.selected_process_pid?;
            let matched_process = running_processes
                .iter()
                .find(|process| process.pid == selected_pid)?;
            Some(GameTargetRule::from_process(
                matched_process,
                picker_state.match_mode,
            ))
        }
    }
}

fn build_sorted_window_rows<'slice>(
    open_windows: &'slice [ActiveWindowSnapshot],
    running_processes: &'slice [RunningProcessEntry],
    query_lower: &str,
) -> Vec<CorrelatedWindowRow<'slice>> {
    let mut rows: Vec<CorrelatedWindowRow<'slice>> = open_windows
        .iter()
        .filter_map(|window| {
            let correlated_process = correlate_process_for_window(window, running_processes);
            let steam_app_id = window
                .extracted_steam_app_id()
                .map(ToString::to_string)
                .or_else(|| correlated_process.and_then(|entry| entry.steam_app_id.clone()));
            let is_steam_or_proton = steam_app_id.is_some()
                || correlated_process.is_some_and(|entry| entry.is_wine_or_proton);

            let row = CorrelatedWindowRow {
                window,
                correlated_process,
                steam_app_id,
                is_steam_or_proton,
            };
            if matches_window_query(&row, query_lower) {
                Some(row)
            } else {
                None
            }
        })
        .collect();

    rows.sort_by(|left_row, right_row| {
        right_row
            .is_steam_or_proton
            .cmp(&left_row.is_steam_or_proton)
            .then_with(|| right_row.window.is_focused.cmp(&left_row.window.is_focused))
            .then_with(|| {
                left_row
                    .window
                    .title
                    .to_ascii_lowercase()
                    .cmp(&right_row.window.title.to_ascii_lowercase())
            })
    });
    rows
}

fn matches_window_query(row: &CorrelatedWindowRow<'_>, query_lower: &str) -> bool {
    if query_lower.is_empty() {
        return true;
    }
    if row.window.title.to_ascii_lowercase().contains(query_lower)
        || row.window.app_id.to_ascii_lowercase().contains(query_lower)
    {
        return true;
    }
    if row
        .window
        .pid
        .is_some_and(|pid| pid.to_string().contains(query_lower))
    {
        return true;
    }
    if row
        .steam_app_id
        .as_deref()
        .is_some_and(|steam_id| steam_id.to_ascii_lowercase().contains(query_lower))
    {
        return true;
    }
    row.correlated_process.is_some_and(|process| {
        process
            .executable_name
            .to_ascii_lowercase()
            .contains(query_lower)
            || process.pid.to_string().contains(query_lower)
    })
}

fn matches_process_query(process: &RunningProcessEntry, query_lower: &str) -> bool {
    if query_lower.is_empty() {
        return true;
    }
    process
        .executable_name
        .to_ascii_lowercase()
        .contains(query_lower)
        || process
            .process_name
            .to_ascii_lowercase()
            .contains(query_lower)
        || process
            .command_line
            .to_ascii_lowercase()
            .contains(query_lower)
        || process.pid.to_string().contains(query_lower)
        || process
            .steam_app_id
            .as_deref()
            .is_some_and(|steam_id| steam_id.to_ascii_lowercase().contains(query_lower))
}
