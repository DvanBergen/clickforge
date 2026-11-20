use std::fmt::Debug;
use std::hash::Hash;

use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};
use egui::{
    Button, ComboBox, CornerRadius, Event, Id, Popup, PopupCloseBehavior, ScrollArea, Sense,
    Stroke, TextEdit, Ui, vec2,
};

use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BORDER_STRONG, BORDER_SUBTLE, BlockVisualColors,
    CORNER_RADIUS_PILL_PX, CORNER_RADIUS_SM_PX, FONT_SIZE_SMALL_PX, SURFACE_ELEVATED, TEXT_MUTED,
    TEXT_PRIMARY, map_egui_key_to_sdk_key, rich_mono_bold, rich_semibold,
};

const PILL_TEXT_SIZE_PX: f32 = 12.5;
const KEY_PICKER_POPUP_WIDTH_PX: f32 = 436.0;
const KEY_PICKER_SCROLL_HEIGHT_PX: f32 = 280.0;
const KEY_PICKER_SCROLLBAR_RESERVE_PX: f32 = 18.0;
const KEY_PICKER_CONTENT_WIDTH_PX: f32 =
    KEY_PICKER_POPUP_WIDTH_PX - KEY_PICKER_SCROLLBAR_RESERVE_PX;
const KEY_PICKER_CAPTURE_WIDTH_PX: f32 = 92.0;
const KEY_PICKER_HEADER_GAP_PX: f32 = 8.0;
const KEY_PICKER_SEARCH_WIDTH_PX: f32 =
    KEY_PICKER_POPUP_WIDTH_PX - KEY_PICKER_CAPTURE_WIDTH_PX - KEY_PICKER_HEADER_GAP_PX - 16.0;
const KEYCAP_GAP_PX: f32 = 4.0;
const KEYCAP_HEIGHT_PX: f32 = 24.0;
const KEYCAP_MIN_WIDTH_PX: f32 = 34.0;
const KEYCAP_BUTTON_PADDING_X_PX: f32 = 5.0;
const KEYCAP_BUTTON_PADDING_Y_PX: f32 = 2.5;
const ESTIMATED_MONO_CHAR_WIDTH_PX: f32 = 7.6;
const ESTIMATED_KEYCAP_CHROME_PX: f32 = 18.0;

#[derive(Debug, Clone, Default)]
struct KeyPickerPopupState {
    search_query: String,
    is_capturing_key: bool,
    should_focus_search: bool,
}

/// Renders a streamlined Mouse/Key selector shared by trigger hats and conditions.
pub(crate) fn render_input_trigger_picker<SaltType: Hash + Debug + Copy>(
    ui: &mut Ui,
    id_salt: SaltType,
    trigger_input: &mut InputTrigger,
    visual_colors: BlockVisualColors,
) -> bool {
    let mut did_change = false;
    let is_mouse = matches!(trigger_input, InputTrigger::Mouse(_));
    let kind_button = Button::new(rich_semibold(
        if is_mouse { "Mouse" } else { "Key" },
        FONT_SIZE_SMALL_PX,
        visual_colors.text_muted,
    ))
    .fill(visual_colors.badge_bg)
    .stroke(Stroke::new(1.0, visual_colors.pill_well_border))
    .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
    .sense(Sense::click_and_drag());

    if ui
        .add(kind_button)
        .on_hover_text("Switch between Mouse button and Keyboard key")
        .clicked()
    {
        *trigger_input = if is_mouse {
            InputTrigger::Key(KeyCode::F5)
        } else {
            InputTrigger::Mouse(MouseButton::Left)
        };
        did_change = true;
    }

    match trigger_input {
        InputTrigger::Mouse(mouse_button) => {
            did_change |= render_mouse_button_combo(ui, (id_salt, "mouse_button"), mouse_button);
        }
        InputTrigger::Key(key_code) => {
            did_change |=
                render_key_code_picker(ui, (id_salt, "key_code"), key_code, visual_colors);
        }
    }
    did_change
}

/// Renders a recessed pill dropdown selecting one of the supported `MouseButton` variants.
pub(crate) fn render_mouse_button_combo<SaltType: Hash + Debug>(
    ui: &mut Ui,
    id_salt: SaltType,
    mouse_button: &mut MouseButton,
) -> bool {
    let mut did_change = false;
    ComboBox::from_id_salt(id_salt)
        .selected_text(mouse_button.display_label())
        .width(126.0)
        .show_ui(ui, |ui| {
            for candidate in MouseButton::all_buttons() {
                did_change |= ui
                    .selectable_value(mouse_button, *candidate, candidate.display_label())
                    .changed();
            }
        });
    did_change
}

/// Renders a clickable key pill that opens a fixed-width searchable categorized key picker popup.
pub(crate) fn render_key_code_picker<SaltType: Hash + Debug>(
    ui: &mut Ui,
    id_salt: SaltType,
    key_code: &mut KeyCode,
    visual_colors: BlockVisualColors,
) -> bool {
    let pill_button = Button::new(rich_mono_bold(
        format!("{} ▾", key_code.display_label()),
        PILL_TEXT_SIZE_PX,
        visual_colors.text_primary,
    ))
    .fill(visual_colors.pill_well_bg)
    .stroke(Stroke::new(1.0, visual_colors.pill_well_border))
    .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
    .sense(Sense::click_and_drag());
    let trigger_response = ui
        .add(pill_button)
        .on_hover_text("Click to search or capture a keyboard key");

    let popup_id = Id::unique(("searchable_key_picker_popup", id_salt));
    let state_id = popup_id.with("state");
    if trigger_response.clicked() {
        let fresh_state = KeyPickerPopupState {
            search_query: String::new(),
            is_capturing_key: false,
            should_focus_search: true,
        };
        ui.ctx()
            .data_mut(|data| data.insert_temp(state_id, fresh_state));
    }

    let mut did_change = false;
    let is_open = Popup::from_toggle_button_response(&trigger_response)
        .id(popup_id)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .width(KEY_PICKER_POPUP_WIDTH_PX)
        .show(|ui| {
            ui.set_min_width(KEY_PICKER_POPUP_WIDTH_PX);
            ui.set_max_width(KEY_PICKER_POPUP_WIDTH_PX);

            let mut state = ui
                .ctx()
                .data_mut(|data| data.get_temp::<KeyPickerPopupState>(state_id))
                .unwrap_or_default();
            if let Some(selected_key) = render_key_picker_popup_contents(ui, *key_code, &mut state)
            {
                *key_code = selected_key;
                did_change = true;
                Popup::close_id(ui.ctx(), popup_id);
                ui.ctx()
                    .data_mut(|data| data.remove::<KeyPickerPopupState>(state_id));
            } else {
                ui.ctx().data_mut(|data| data.insert_temp(state_id, state));
            }
        })
        .is_some();

    if !is_open {
        ui.ctx()
            .data_mut(|data| data.remove::<KeyPickerPopupState>(state_id));
    }
    did_change
}

fn render_key_picker_popup_contents(
    ui: &mut Ui,
    current_key: KeyCode,
    popup_state: &mut KeyPickerPopupState,
) -> Option<KeyCode> {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = KEY_PICKER_HEADER_GAP_PX;
        let search_response = ui.add(
            TextEdit::singleline(&mut popup_state.search_query)
                .desired_width(KEY_PICKER_SEARCH_WIDTH_PX)
                .hint_text("Search key..."),
        );
        if popup_state.should_focus_search {
            search_response.request_focus();
            popup_state.should_focus_search = false;
        }
        if search_response.gained_focus() || search_response.changed() {
            popup_state.is_capturing_key = false;
        }
        let (label, fill, border, ink) = if popup_state.is_capturing_key {
            (
                "Listening...",
                ACCENT_PRIMARY_TINT,
                ACCENT_PRIMARY,
                ACCENT_PRIMARY,
            )
        } else {
            ("Press Key", SURFACE_ELEVATED, BORDER_STRONG, TEXT_PRIMARY)
        };
        let capture_button = Button::new(rich_semibold(label, FONT_SIZE_SMALL_PX, ink))
            .fill(fill)
            .stroke(Stroke::new(1.0, border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
            .min_size(vec2(KEY_PICKER_CAPTURE_WIDTH_PX, KEYCAP_HEIGHT_PX));
        if ui.add(capture_button).clicked() {
            popup_state.is_capturing_key = !popup_state.is_capturing_key;
            if popup_state.is_capturing_key {
                search_response.surrender_focus();
            }
        }
    });

    if popup_state.is_capturing_key
        && let Some(captured_key) = detect_popup_key_press(ui)
    {
        popup_state.is_capturing_key = false;
        if captured_key != KeyCode::Escape {
            return Some(captured_key);
        }
    }

    ui.add_space(6.0);
    let query = popup_state.search_query.trim().to_ascii_lowercase();
    let mut picked_key = None;
    ScrollArea::vertical()
        .max_width(KEY_PICKER_POPUP_WIDTH_PX)
        .max_height(KEY_PICKER_SCROLL_HEIGHT_PX)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.set_min_width(KEY_PICKER_CONTENT_WIDTH_PX);
            ui.set_max_width(KEY_PICKER_CONTENT_WIDTH_PX);
            ui.spacing_mut().button_padding =
                vec2(KEYCAP_BUTTON_PADDING_X_PX, KEYCAP_BUTTON_PADDING_Y_PX);
            ui.spacing_mut().item_spacing = vec2(KEYCAP_GAP_PX, KEYCAP_GAP_PX);
            picked_key = if query.is_empty() {
                render_categorized_key_grids(ui, current_key)
            } else {
                render_filtered_key_grid(ui, current_key, &query)
            };
        });
    picked_key
}

fn render_filtered_key_grid(
    ui: &mut Ui,
    current_key: KeyCode,
    normalized_query: &str,
) -> Option<KeyCode> {
    let matched_keys: Vec<KeyCode> = KeyCode::all_keys()
        .iter()
        .copied()
        .filter(|candidate| {
            candidate
                .display_label()
                .to_ascii_lowercase()
                .contains(normalized_query)
        })
        .collect();
    if matched_keys.is_empty() {
        ui.label(rich_semibold(
            "No keys match your search",
            FONT_SIZE_SMALL_PX,
            TEXT_MUTED,
        ));
        return None;
    }
    if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
        return matched_keys.first().copied();
    }

    let packed_rows = pack_keys_into_bounded_rows(&matched_keys, KEY_PICKER_CONTENT_WIDTH_PX);
    for row_keys in packed_rows {
        if let Some(selected_key) = render_variable_width_key_row(ui, current_key, &row_keys) {
            return Some(selected_key);
        }
    }
    None
}

fn render_categorized_key_grids(ui: &mut Ui, current_key: KeyCode) -> Option<KeyCode> {
    let all_keys = KeyCode::all_keys();
    let sections: [(&str, &[KeyCode], usize); 7] = [
        ("Letters", all_keys.get(0..26).unwrap_or(&[]), 10),
        ("Numbers", all_keys.get(26..36).unwrap_or(&[]), 10),
        ("Function Keys", all_keys.get(36..48).unwrap_or(&[]), 6),
        ("Modifiers", all_keys.get(48..57).unwrap_or(&[]), 4),
        ("Navigation", all_keys.get(57..75).unwrap_or(&[]), 4),
        ("Symbols", all_keys.get(75..86).unwrap_or(&[]), 10),
        ("Numpad", all_keys.get(86..).unwrap_or(&[]), 4),
    ];
    for (section_label, section_keys, columns_per_row) in sections {
        ui.label(rich_semibold(section_label, 11.0, TEXT_MUTED));
        ui.add_space(2.0);
        if let Some(selected_key) =
            render_fixed_column_key_rows(ui, current_key, section_keys, columns_per_row)
        {
            return Some(selected_key);
        }
        ui.add_space(6.0);
    }
    None
}

fn render_fixed_column_key_rows(
    ui: &mut Ui,
    current_key: KeyCode,
    candidate_keys: &[KeyCode],
    columns_per_row: usize,
) -> Option<KeyCode> {
    let safe_columns = columns_per_row.max(1);
    let total_gap_px = KEYCAP_GAP_PX * (safe_columns.saturating_sub(1) as f32);
    let cell_width_px =
        ((KEY_PICKER_CONTENT_WIDTH_PX - total_gap_px) / (safe_columns as f32)).floor();

    for key_chunk in candidate_keys.chunks(safe_columns) {
        let mut selected_in_row = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(KEYCAP_GAP_PX, KEYCAP_GAP_PX);
            for candidate in key_chunk {
                if render_single_keycap_button(ui, current_key, *candidate, cell_width_px) {
                    selected_in_row = Some(*candidate);
                }
            }
        });
        if selected_in_row.is_some() {
            return selected_in_row;
        }
    }
    None
}

fn render_variable_width_key_row(
    ui: &mut Ui,
    current_key: KeyCode,
    row_keys: &[KeyCode],
) -> Option<KeyCode> {
    let mut selected_in_row = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(KEYCAP_GAP_PX, KEYCAP_GAP_PX);
        for candidate in row_keys {
            let button_width_px = estimate_keycap_width_px(*candidate);
            if render_single_keycap_button(ui, current_key, *candidate, button_width_px) {
                selected_in_row = Some(*candidate);
            }
        }
    });
    selected_in_row
}

fn render_single_keycap_button(
    ui: &mut Ui,
    current_key: KeyCode,
    candidate_key: KeyCode,
    min_width_px: f32,
) -> bool {
    let (fill, border, ink) = if candidate_key == current_key {
        (ACCENT_PRIMARY_TINT, ACCENT_PRIMARY, ACCENT_PRIMARY)
    } else {
        (SURFACE_ELEVATED, BORDER_SUBTLE, TEXT_PRIMARY)
    };
    let button = Button::new(rich_mono_bold(
        candidate_key.display_label(),
        FONT_SIZE_SMALL_PX,
        ink,
    ))
    .fill(fill)
    .stroke(Stroke::new(1.0, border))
    .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
    .min_size(vec2(min_width_px, KEYCAP_HEIGHT_PX));
    ui.add(button).clicked()
}

fn estimate_keycap_width_px(key_code: KeyCode) -> f32 {
    let char_count = key_code.display_label().chars().count() as f32;
    (char_count * ESTIMATED_MONO_CHAR_WIDTH_PX + ESTIMATED_KEYCAP_CHROME_PX)
        .max(KEYCAP_MIN_WIDTH_PX)
        .ceil()
}

fn pack_keys_into_bounded_rows(
    candidate_keys: &[KeyCode],
    max_row_width_px: f32,
) -> Vec<Vec<KeyCode>> {
    let mut rows: Vec<Vec<KeyCode>> = Vec::new();
    let mut current_row: Vec<KeyCode> = Vec::new();
    let mut current_row_width_px: f32 = 0.0;

    for candidate in candidate_keys {
        let key_width_px = estimate_keycap_width_px(*candidate);
        let gap_px = if current_row.is_empty() {
            0.0
        } else {
            KEYCAP_GAP_PX
        };
        if !current_row.is_empty()
            && current_row_width_px + gap_px + key_width_px > max_row_width_px
        {
            rows.push(current_row);
            current_row = vec![*candidate];
            current_row_width_px = key_width_px;
            continue;
        }
        current_row.push(*candidate);
        current_row_width_px += gap_px + key_width_px;
    }

    if !current_row.is_empty() {
        rows.push(current_row);
    }
    rows
}

fn detect_popup_key_press(ui: &Ui) -> Option<KeyCode> {
    ui.input(|input_state| {
        for event in &input_state.events {
            let Event::Key {
                key,
                physical_key,
                pressed: true,
                repeat: false,
                ..
            } = event
            else {
                continue;
            };
            if let Some(sdk_key) = map_egui_key_to_sdk_key(physical_key.unwrap_or(*key)) {
                return Some(sdk_key);
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_key_rows_stay_within_fixed_popup_content_width() {
        let packed_rows =
            pack_keys_into_bounded_rows(KeyCode::all_keys(), KEY_PICKER_CONTENT_WIDTH_PX);
        assert!(!packed_rows.is_empty());

        let mut total_packed_keys: usize = 0;
        for row_keys in packed_rows {
            total_packed_keys = total_packed_keys.saturating_add(row_keys.len());
            let total_gaps_px = KEYCAP_GAP_PX * (row_keys.len().saturating_sub(1) as f32);
            let total_width_px: f32 = row_keys
                .iter()
                .map(|key_code| estimate_keycap_width_px(*key_code))
                .sum::<f32>()
                + total_gaps_px;
            assert!(
                total_width_px <= KEY_PICKER_CONTENT_WIDTH_PX,
                "row exceeded fixed popup content width: {total_width_px} > {KEY_PICKER_CONTENT_WIDTH_PX}"
            );
        }
        assert_eq!(total_packed_keys, KeyCode::all_keys().len());
    }
}
