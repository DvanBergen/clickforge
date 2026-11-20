mod step_builder;

use egui::{
    Align, Color32, CornerRadius, Frame, Id, Layout, Margin, Modal, Popup, Sense, Stroke, TextEdit,
    Theme, Ui, vec2,
};

use crate::models::{
    CustomBlockColor, CustomBlockDefinition, MAX_CUSTOM_BLOCK_NAME_CHARS, MAX_CUSTOM_BLOCKS,
    StatementBlock, validate_custom_block_definition,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, BORDER_STRONG, BORDER_SUBTLE, CORNER_RADIUS_MD_PX, CORNER_RADIUS_PILL_PX,
    CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX, FONT_SIZE_HEADING_PX, FONT_SIZE_SMALL_PX,
    INK_ON_ACCENT, STATUS_DANGER, STATUS_DANGER_INK, STATUS_DANGER_TINT, STATUS_RUNNING_INK,
    SURFACE_ELEVATED, SURFACE_HOVER, SURFACE_PANEL, TEXT_MUTED, TEXT_PRIMARY, build_styled_button,
    custom_block_swatch_color, custom_block_visual_colors, paint_status_dot, rich_bold, rich_mono,
    rich_semibold, truncate_display_text,
};

use step_builder::render_steps_section;

const MODAL_WIDTH_PX: f32 = 668.0;
const SWATCH_DOT_RADIUS_PX: f32 = 4.5;
const MAX_PREVIEW_NAME_CHARS: usize = 24;

/// Actions emitted when the user confirms a Custom Tile inside `CustomBlockModalState`.
#[derive(Debug, Clone)]
pub enum CustomBlockModalAction {
    SaveCustomBlock(CustomBlockDefinition),
}

/// Transient editor state for creating or editing a reusable Custom User Tile.
#[derive(Debug, Clone)]
pub struct CustomBlockModalState {
    pub is_open: bool,
    is_editing_existing: bool,
    pub(super) draft: CustomBlockDefinition,
    should_focus_name_input: bool,
}

impl Default for CustomBlockModalState {
    fn default() -> Self {
        Self {
            is_open: false,
            is_editing_existing: false,
            draft: CustomBlockDefinition::new_empty("", "", CustomBlockColor::Coral),
            should_focus_name_input: false,
        }
    }
}

impl CustomBlockModalState {
    /// Opens the modal to create a brand-new blank custom tile.
    pub fn open_new_blank(&mut self, existing_tile_count: usize) {
        let default_name = format!("Custom Tile {}", existing_tile_count.saturating_add(1));
        self.draft = CustomBlockDefinition::new_empty(default_name, "", CustomBlockColor::Coral);
        self.is_editing_existing = false;
        self.should_focus_name_input = true;
        self.is_open = true;
    }

    /// Opens the modal pre-populated with statement blocks captured from the canvas.
    pub fn open_from_statements(
        &mut self,
        suggested_name: String,
        statements: Vec<StatementBlock>,
    ) {
        let sanitized_name: String = suggested_name
            .trim()
            .chars()
            .take(MAX_CUSTOM_BLOCK_NAME_CHARS)
            .collect();
        let resolved_name = if sanitized_name.is_empty() {
            "Custom Tile".to_string()
        } else {
            sanitized_name
        };
        self.draft = CustomBlockDefinition::from_statements(
            resolved_name,
            "",
            CustomBlockColor::Coral,
            statements,
        );
        self.is_editing_existing = false;
        self.should_focus_name_input = true;
        self.is_open = true;
    }

    /// Opens the modal to edit an existing persisted custom tile definition in place.
    pub fn open_edit(&mut self, definition: &CustomBlockDefinition) {
        self.draft = definition.clone();
        self.is_editing_existing = true;
        self.should_focus_name_input = true;
        self.is_open = true;
    }
}

/// Renders the Custom Tile Builder modal when `modal_state.is_open` is true.
pub fn render_custom_block_modal(
    egui_context: &egui::Context,
    modal_state: &mut CustomBlockModalState,
    existing_custom_blocks: &[CustomBlockDefinition],
    emitted_actions: &mut Vec<CustomBlockModalAction>,
) {
    if !modal_state.is_open {
        return;
    }

    let was_popup_open = Popup::is_any_open(egui_context);
    let modal_frame = Frame::NONE
        .fill(SURFACE_PANEL)
        .stroke(Stroke::new(1.0, BORDER_STRONG))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
        .shadow(egui_context.style_of(Theme::Dark).visuals.window_shadow)
        .inner_margin(Margin::same(18));

    let modal_response = Modal::new(Id::unique("custom_block_builder_modal"))
        .frame(modal_frame)
        .show(egui_context, |ui| {
            ui.set_width(MODAL_WIDTH_PX);
            render_modal_header(ui, modal_state);
            ui.add_space(10.0);
            render_metadata_fields(ui, modal_state);
            ui.add_space(10.0);
            render_color_swatch_picker(ui, &mut modal_state.draft.color);
            ui.add_space(12.0);
            render_steps_section(ui, modal_state, existing_custom_blocks);
            ui.add_space(10.0);

            let validation_result = validate_modal_draft(
                &modal_state.draft,
                modal_state.is_editing_existing,
                existing_custom_blocks,
            );
            render_validation_strip(
                ui,
                &validation_result,
                modal_state.draft.total_block_count(),
            );
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(10.0);
            render_modal_footer(ui, modal_state, validation_result.is_ok(), emitted_actions);
        });

    if modal_response.should_close() && !was_popup_open {
        modal_state.is_open = false;
    }
}

fn render_modal_header(ui: &mut Ui, modal_state: &CustomBlockModalState) {
    let heading_text = if modal_state.is_editing_existing {
        "Edit Custom Tile"
    } else {
        "Create Custom Tile"
    };
    let preview_colors = custom_block_visual_colors(modal_state.draft.color);
    let preview_name = if modal_state.draft.name.trim().is_empty() {
        "Untitled Tile".to_string()
    } else {
        truncate_display_text(modal_state.draft.name.trim(), MAX_PREVIEW_NAME_CHARS)
    };
    let step_count = modal_state.draft.total_block_count();
    let step_word = if step_count == 1 { "step" } else { "steps" };

    ui.horizontal(|ui| {
        ui.label(rich_bold(heading_text, FONT_SIZE_HEADING_PX, TEXT_PRIMARY));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            Frame::NONE
                .fill(preview_colors.fill)
                .stroke(Stroke::new(1.0, preview_colors.border))
                .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
                .inner_margin(Margin::symmetric(9, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(rich_semibold(
                            format!("★ {preview_name}"),
                            FONT_SIZE_SMALL_PX,
                            preview_colors.text_primary,
                        ));
                        ui.label(rich_mono(
                            format!("{step_count} {step_word}"),
                            FONT_SIZE_SMALL_PX,
                            preview_colors.text_muted,
                        ));
                    });
                });
        });
    });
}

fn render_metadata_fields(ui: &mut Ui, modal_state: &mut CustomBlockModalState) {
    let name_char_count = modal_state.draft.name.chars().count();
    let count_color = if name_char_count > MAX_CUSTOM_BLOCK_NAME_CHARS {
        STATUS_DANGER_INK
    } else {
        TEXT_MUTED
    };

    ui.horizontal(|ui| {
        ui.label(rich_semibold("Tile Name", FONT_SIZE_SMALL_PX, TEXT_MUTED));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(rich_mono(
                format!("{name_char_count}/{MAX_CUSTOM_BLOCK_NAME_CHARS}"),
                FONT_SIZE_SMALL_PX,
                count_color,
            ));
        });
    });
    ui.add_space(3.0);
    let name_response = ui.add(
        TextEdit::singleline(&mut modal_state.draft.name)
            .desired_width(f32::INFINITY)
            .char_limit(MAX_CUSTOM_BLOCK_NAME_CHARS)
            .hint_text("Enter reusable tile name (e.g. Quick Drop & Confirm)"),
    );
    if modal_state.should_focus_name_input {
        name_response.request_focus();
        modal_state.should_focus_name_input = false;
    }

    ui.add_space(8.0);
    ui.label(rich_semibold(
        "Description (optional)",
        FONT_SIZE_SMALL_PX,
        TEXT_MUTED,
    ));
    ui.add_space(3.0);
    ui.add(
        TextEdit::singleline(&mut modal_state.draft.description)
            .desired_width(f32::INFINITY)
            .hint_text("Short tooltip shown when hovering this tile in My Blocks"),
    );
}

fn render_color_swatch_picker(ui: &mut Ui, selected_color: &mut CustomBlockColor) {
    ui.label(rich_semibold("Tile Color", FONT_SIZE_SMALL_PX, TEXT_MUTED));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for candidate_color in CustomBlockColor::all_colors() {
            let is_selected = *selected_color == candidate_color;
            let swatch_accent = custom_block_swatch_color(candidate_color);
            let (fill_color, border_color, text_color) = if is_selected {
                (SURFACE_HOVER, swatch_accent, TEXT_PRIMARY)
            } else {
                (SURFACE_ELEVATED, BORDER_SUBTLE, TEXT_MUTED)
            };

            let chip_frame = Frame::NONE
                .fill(fill_color)
                .stroke(Stroke::new(
                    if is_selected { 1.5 } else { 1.0 },
                    border_color,
                ))
                .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
                .inner_margin(Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        paint_status_dot(ui, swatch_accent, SWATCH_DOT_RADIUS_PX);
                        ui.label(rich_semibold(
                            candidate_color.display_label(),
                            FONT_SIZE_SMALL_PX,
                            text_color,
                        ));
                    });
                });

            let click_response = ui.interact(
                chip_frame.response.rect,
                Id::unique(("custom_tile_swatch", candidate_color.display_label())),
                Sense::click(),
            );
            if click_response.clicked() {
                *selected_color = candidate_color;
            }
        }
    });
}

fn render_validation_strip(
    ui: &mut Ui,
    validation_result: &Result<(), String>,
    total_block_count: usize,
) {
    match validation_result {
        Ok(()) => {
            let block_word = if total_block_count == 1 {
                "block"
            } else {
                "blocks"
            };
            ui.label(rich_semibold(
                format!("Valid custom tile ({total_block_count} {block_word})."),
                FONT_SIZE_SMALL_PX,
                STATUS_RUNNING_INK,
            ));
        }
        Err(error_message) => {
            Frame::NONE
                .fill(STATUS_DANGER_TINT)
                .stroke(Stroke::new(1.0, STATUS_DANGER))
                .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(rich_semibold(
                        error_message,
                        FONT_SIZE_SMALL_PX,
                        STATUS_DANGER_INK,
                    ));
                });
        }
    }
}

fn render_modal_footer(
    ui: &mut Ui,
    modal_state: &mut CustomBlockModalState,
    can_save: bool,
    emitted_actions: &mut Vec<CustomBlockModalAction>,
) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (save_fill, save_ink) = if can_save {
            (ACCENT_PRIMARY, INK_ON_ACCENT)
        } else {
            (SURFACE_ELEVATED, TEXT_MUTED)
        };
        let save_button = build_styled_button(
            rich_semibold("Save Custom Tile", FONT_SIZE_BODY_PX, save_ink),
            save_fill,
            Color32::TRANSPARENT,
            CORNER_RADIUS_SM_PX,
            vec2(138.0, 32.0),
        );
        if ui.add_enabled(can_save, save_button).clicked() {
            let mut finalized_tile = modal_state.draft.clone();
            finalized_tile.name = finalized_tile.name.trim().to_string();
            finalized_tile.description = finalized_tile.description.trim().to_string();
            finalized_tile.touch_updated_timestamp();
            emitted_actions.push(CustomBlockModalAction::SaveCustomBlock(finalized_tile));
            modal_state.is_open = false;
        }

        let cancel_button = build_styled_button(
            rich_semibold("Cancel", FONT_SIZE_BODY_PX, TEXT_PRIMARY),
            SURFACE_ELEVATED,
            BORDER_SUBTLE,
            CORNER_RADIUS_SM_PX,
            vec2(78.0, 32.0),
        );
        if ui.add(cancel_button).clicked() {
            modal_state.is_open = false;
        }
    });
}

fn validate_modal_draft(
    draft: &CustomBlockDefinition,
    is_editing_existing: bool,
    existing_custom_blocks: &[CustomBlockDefinition],
) -> Result<(), String> {
    if !is_editing_existing && existing_custom_blocks.len() >= MAX_CUSTOM_BLOCKS {
        return Err(format!(
            "Custom tile limit reached ({MAX_CUSTOM_BLOCKS} maximum)."
        ));
    }
    let mut candidate_library: Vec<CustomBlockDefinition> = existing_custom_blocks.to_vec();
    if let Some(existing_entry) = candidate_library
        .iter_mut()
        .find(|entry| entry.id == draft.id)
    {
        *existing_entry = draft.clone();
    } else {
        candidate_library.push(draft.clone());
    }
    validate_custom_block_definition(draft, &candidate_library)
}

#[cfg(test)]
mod tests {
    use clickforge_sdk::{InputTrigger, MouseButton};

    use super::*;
    use crate::models::{BlockId, StatementKind};

    #[test]
    fn custom_block_modal_state_opens_in_blank_captured_and_edit_modes() {
        let mut modal_state = CustomBlockModalState::default();
        modal_state.open_new_blank(2);
        assert!(modal_state.is_open);
        assert!(!modal_state.is_editing_existing);
        assert_eq!(modal_state.draft.name, "Custom Tile 3");
        assert!(validate_modal_draft(&modal_state.draft, false, &[]).is_err());

        let captured_step = StatementBlock {
            id: BlockId(99),
            kind: StatementKind::MouseHoldWhilePressed {
                button: MouseButton::Left,
                watch_input: InputTrigger::Mouse(MouseButton::Left),
            },
        };
        modal_state.open_from_statements("Drag Hold".to_string(), vec![captured_step]);
        assert!(modal_state.is_open);
        assert!(!modal_state.is_editing_existing);
        assert_eq!(modal_state.draft.statements.len(), 1);
        assert_eq!(modal_state.draft.statements[0].id, BlockId(1));
        assert!(validate_modal_draft(&modal_state.draft, false, &[]).is_ok());

        let existing_definition = modal_state.draft.clone();
        modal_state.open_edit(&existing_definition);
        assert!(modal_state.is_open);
        assert!(modal_state.is_editing_existing);
        assert_eq!(modal_state.draft.id, existing_definition.id);
    }

    #[test]
    fn validate_modal_draft_rejects_indirect_cycles_and_library_overflow() {
        let tile_a = CustomBlockDefinition::from_statements(
            "Tile A",
            "",
            CustomBlockColor::Coral,
            vec![StatementBlock {
                id: BlockId(1),
                kind: StatementKind::MouseClick {
                    mouse_button: MouseButton::Left,
                    hold_duration_ms: 10,
                },
            }],
        );
        let tile_b = CustomBlockDefinition::from_statements(
            "Tile B",
            "",
            CustomBlockColor::Blue,
            vec![StatementBlock {
                id: BlockId(1),
                kind: StatementKind::RunCustomBlock {
                    custom_block_id: tile_a.id.clone(),
                    cached_name: tile_a.name.clone(),
                },
            }],
        );
        let library = vec![tile_a.clone(), tile_b.clone()];

        let mut edited_tile_a = tile_a;
        edited_tile_a.statements.push(StatementBlock {
            id: BlockId(2),
            kind: StatementKind::RunCustomBlock {
                custom_block_id: tile_b.id.clone(),
                cached_name: tile_b.name.clone(),
            },
        });
        assert!(validate_modal_draft(&edited_tile_a, true, &library).is_err());
    }
}
