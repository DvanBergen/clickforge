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
const KEY_PICKER_POPUP_WIDTH_PX: f32 = 316.0;
const KEY_PICKER_SCROLL_HEIGHT_PX: f32 = 236.0;

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

/// Renders a clickable key pill that opens a live-searchable categorized key picker popup.
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
        let capture_width_px = 92.0;
        let search_width_px = (ui.available_width() - capture_width_px - 6.0).max(120.0);
        let search_response = ui.add(
            TextEdit::singleline(&mut popup_state.search_query)
                .desired_width(search_width_px)
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
            .min_size(vec2(capture_width_px, 24.0));
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
        .max_height(KEY_PICKER_SCROLL_HEIGHT_PX)
        .auto_shrink([false, true])
        .show(ui, |ui| {
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
    render_keycap_wrap_group(ui, current_key, &matched_keys)
}

fn render_categorized_key_grids(ui: &mut Ui, current_key: KeyCode) -> Option<KeyCode> {
    let all_keys = KeyCode::all_keys();
    let sections: [(&str, &[KeyCode]); 6] = [
        ("Letters", all_keys.get(0..26).unwrap_or(&[])),
        ("Numbers", all_keys.get(26..36).unwrap_or(&[])),
        ("Function Keys", all_keys.get(36..48).unwrap_or(&[])),
        ("Modifiers", all_keys.get(48..57).unwrap_or(&[])),
        ("Navigation", all_keys.get(57..75).unwrap_or(&[])),
        ("Symbols & Numpad", all_keys.get(75..).unwrap_or(&[])),
    ];
    for (section_label, section_keys) in sections {
        ui.label(rich_semibold(section_label, 11.0, TEXT_MUTED));
        ui.add_space(2.0);
        if let Some(selected_key) = render_keycap_wrap_group(ui, current_key, section_keys) {
            return Some(selected_key);
        }
        ui.add_space(6.0);
    }
    None
}

fn render_keycap_wrap_group(
    ui: &mut Ui,
    current_key: KeyCode,
    candidate_keys: &[KeyCode],
) -> Option<KeyCode> {
    let mut selected_key = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for candidate in candidate_keys {
            let (fill, border, ink) = if *candidate == current_key {
                (ACCENT_PRIMARY_TINT, ACCENT_PRIMARY, ACCENT_PRIMARY)
            } else {
                (SURFACE_ELEVATED, BORDER_SUBTLE, TEXT_PRIMARY)
            };
            let button = Button::new(rich_mono_bold(
                candidate.display_label(),
                FONT_SIZE_SMALL_PX,
                ink,
            ))
            .fill(fill)
            .stroke(Stroke::new(1.0, border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
            .min_size(vec2(30.0, 24.0));
            if ui.add(button).clicked() {
                selected_key = Some(*candidate);
            }
        }
    });
    selected_key
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
