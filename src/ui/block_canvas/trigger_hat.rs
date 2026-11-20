use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};
use egui::{Button, ComboBox, CornerRadius, Frame, Margin, Pos2, Sense, Shape, Stroke, Ui, vec2};

use crate::models::{BlockCategory, MAX_TRIGGER_AND_INPUTS, TriggerBlock};
use crate::ui::block_field_editors::{render_input_trigger_picker, with_pill_well_style};
use crate::ui::block_node_renderer::{
    BLOCK_MAX_WIDTH_PX, FULL_BLOCK_GEOMETRY, build_trigger_hat_shapes,
};
use crate::ui::block_palette::BlockDragState;
use crate::ui::theme::{
    ACCENT_PRIMARY, BlockVisualColors, CORNER_RADIUS_PILL_PX, FONT_SIZE_BODY_PX,
    FONT_SIZE_SMALL_PX, block_visual_colors, rich_mono_bold, rich_semibold,
};

const HAT_MIN_WIDTH_PX: f32 = 320.0;
const TRIGGER_MODE_COMBO_WIDTH_PX: f32 = 156.0;
const CHORD_REMOVE_BUTTON_SIZE_PX: f32 = 22.0;
const DEFAULT_CHORD_CANDIDATES: [InputTrigger; 5] = [
    InputTrigger::Key(KeyCode::LeftShift),
    InputTrigger::Key(KeyCode::LeftCtrl),
    InputTrigger::Key(KeyCode::LeftAlt),
    InputTrigger::Key(KeyCode::RightShift),
    InputTrigger::Key(KeyCode::Space),
];

/// Renders the Trigger Hat block at the top of the visual script stack, including `+ AND` chords.
pub(super) fn render_trigger_hat_block(
    ui: &mut Ui,
    script_id: &str,
    trigger: &mut TriggerBlock,
    pointer_pos: Option<Pos2>,
    drag_state: &mut BlockDragState,
) -> bool {
    let visual_colors = block_visual_colors(BlockCategory::Trigger);
    let mut did_change = false;
    let background_shape_index = ui.painter().add(Shape::Noop);

    let hat_response = Frame::NONE
        .inner_margin(Margin {
            left: 14,
            right: 16,
            top: (FULL_BLOCK_GEOMETRY.hat_dome_height_px as i8) + 8,
            bottom: 9,
        })
        .show(ui, |ui| {
            ui.set_min_width(HAT_MIN_WIDTH_PX);
            ui.set_max_width(BLOCK_MAX_WIDTH_PX);
            with_pill_well_style(ui, visual_colors, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(rich_semibold(
                        "When",
                        FONT_SIZE_BODY_PX,
                        visual_colors.text_primary,
                    ));

                    ComboBox::from_id_salt(("trigger_mode", script_id))
                        .selected_text(trigger_mode_label(trigger))
                        .width(TRIGGER_MODE_COMBO_WIDTH_PX)
                        .show_ui(ui, |ui| {
                            did_change |= render_trigger_mode_options(ui, trigger);
                        });

                    did_change |=
                        render_trigger_inputs_inline(ui, script_id, trigger, visual_colors);
                });
            });
        });

    let hat_rect = hat_response.response.rect;
    let is_hovering_replacement = drag_state.is_dragging_trigger()
        && pointer_pos.is_some_and(|pointer| hat_rect.expand(14.0).contains(pointer));
    if is_hovering_replacement {
        drag_state.is_hovering_trigger_hat = true;
    }

    let (fill_color, border_color, stroke_width_px) = if is_hovering_replacement {
        (visual_colors.fill_hover, ACCENT_PRIMARY, 2.25)
    } else if drag_state.is_dragging_trigger() {
        (visual_colors.fill, ACCENT_PRIMARY, 1.75)
    } else {
        (
            visual_colors.fill,
            visual_colors.border,
            FULL_BLOCK_GEOMETRY.stroke_width_px,
        )
    };

    let hat_shapes = build_trigger_hat_shapes(
        hat_rect,
        FULL_BLOCK_GEOMETRY,
        fill_color,
        border_color,
        visual_colors.top_highlight,
        stroke_width_px,
    );
    ui.painter()
        .set(background_shape_index, Shape::Vec(hat_shapes));

    did_change
}

fn render_trigger_inputs_inline(
    ui: &mut Ui,
    script_id: &str,
    trigger: &mut TriggerBlock,
    visual_colors: BlockVisualColors,
) -> bool {
    match trigger {
        TriggerBlock::WhileInputHeld {
            trigger_input,
            additional_held_inputs,
        }
        | TriggerBlock::OnInputToggle {
            trigger_input,
            additional_held_inputs,
        }
        | TriggerBlock::OnInputPressed {
            trigger_input,
            additional_held_inputs,
        } => {
            let mut did_change = render_input_trigger_picker(
                ui,
                ("hat_input", script_id),
                trigger_input,
                visual_colors,
            );
            did_change |= render_additional_chord_inputs(
                ui,
                script_id,
                *trigger_input,
                additional_held_inputs,
                visual_colors,
            );
            did_change
        }
        TriggerBlock::LoopWhileActive => {
            ui.label(rich_semibold(
                "while armed & focused",
                FONT_SIZE_SMALL_PX,
                visual_colors.text_muted,
            ));
            false
        }
    }
}

fn render_additional_chord_inputs(
    ui: &mut Ui,
    script_id: &str,
    primary_trigger_input: InputTrigger,
    additional_held_inputs: &mut Vec<InputTrigger>,
    visual_colors: BlockVisualColors,
) -> bool {
    let mut did_change = false;
    let mut index_to_remove: Option<usize> = None;

    for (chord_index, chord_input) in additional_held_inputs.iter_mut().enumerate() {
        Frame::NONE
            .fill(visual_colors.badge_bg)
            .stroke(Stroke::new(1.0, visual_colors.pill_well_border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
            .inner_margin(Margin::symmetric(6, 2))
            .show(ui, |ui| {
                ui.label(rich_mono_bold(
                    "AND",
                    FONT_SIZE_SMALL_PX,
                    visual_colors.text_primary,
                ));
            });

        did_change |= render_input_trigger_picker(
            ui,
            ("hat_and_input", script_id, chord_index),
            chord_input,
            visual_colors,
        );

        let remove_button = Button::new(rich_semibold(
            "×",
            FONT_SIZE_SMALL_PX,
            visual_colors.text_muted,
        ))
        .fill(visual_colors.badge_bg)
        .stroke(Stroke::new(1.0, visual_colors.pill_well_border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
        .min_size(vec2(
            CHORD_REMOVE_BUTTON_SIZE_PX,
            CHORD_REMOVE_BUTTON_SIZE_PX,
        ))
        .sense(Sense::click_and_drag());

        if ui
            .add(remove_button)
            .on_hover_text("Remove this AND trigger input")
            .clicked()
        {
            index_to_remove = Some(chord_index);
        }
    }

    if let Some(remove_index) = index_to_remove {
        additional_held_inputs.remove(remove_index);
        did_change = true;
    }

    if additional_held_inputs.len() < MAX_TRIGGER_AND_INPUTS {
        let add_chord_button = Button::new(rich_mono_bold(
            "+ AND",
            FONT_SIZE_SMALL_PX,
            visual_colors.text_primary,
        ))
        .fill(visual_colors.badge_bg)
        .stroke(Stroke::new(1.0, visual_colors.pill_well_border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
        .sense(Sense::click_and_drag());

        if ui
            .add(add_chord_button)
            .on_hover_text("Require another key or mouse button to be held simultaneously")
            .clicked()
        {
            let next_chord_input =
                select_next_default_chord_input(primary_trigger_input, additional_held_inputs);
            additional_held_inputs.push(next_chord_input);
            did_change = true;
        }
    }

    did_change
}

fn select_next_default_chord_input(
    primary_trigger_input: InputTrigger,
    existing_additional_inputs: &[InputTrigger],
) -> InputTrigger {
    for candidate_input in DEFAULT_CHORD_CANDIDATES {
        if candidate_input != primary_trigger_input
            && !existing_additional_inputs.contains(&candidate_input)
        {
            return candidate_input;
        }
    }
    InputTrigger::Key(KeyCode::LeftShift)
}

fn render_trigger_mode_options(ui: &mut Ui, trigger: &mut TriggerBlock) -> bool {
    let mut did_change = false;
    let preserved_additional_inputs = trigger.additional_held_inputs().to_vec();

    if ui
        .selectable_label(
            matches!(trigger, TriggerBlock::WhileInputHeld { .. }),
            "While Input Held",
        )
        .clicked()
    {
        let input = trigger
            .trigger_input()
            .unwrap_or(InputTrigger::Mouse(MouseButton::Left));
        *trigger = TriggerBlock::WhileInputHeld {
            trigger_input: input,
            additional_held_inputs: preserved_additional_inputs.clone(),
        };
        did_change = true;
    }
    if ui
        .selectable_label(
            matches!(trigger, TriggerBlock::OnInputToggle { .. }),
            "Toggle on Input",
        )
        .clicked()
    {
        let input = trigger
            .trigger_input()
            .unwrap_or(InputTrigger::Key(KeyCode::F5));
        *trigger = TriggerBlock::OnInputToggle {
            trigger_input: input,
            additional_held_inputs: preserved_additional_inputs.clone(),
        };
        did_change = true;
    }
    if ui
        .selectable_label(
            matches!(trigger, TriggerBlock::OnInputPressed { .. }),
            "Once on Press",
        )
        .clicked()
    {
        let input = trigger
            .trigger_input()
            .unwrap_or(InputTrigger::Mouse(MouseButton::Side));
        *trigger = TriggerBlock::OnInputPressed {
            trigger_input: input,
            additional_held_inputs: preserved_additional_inputs,
        };
        did_change = true;
    }
    if ui
        .selectable_label(
            matches!(trigger, TriggerBlock::LoopWhileActive),
            "Loop While Active",
        )
        .clicked()
    {
        *trigger = TriggerBlock::LoopWhileActive;
        did_change = true;
    }
    did_change
}

fn trigger_mode_label(trigger: &TriggerBlock) -> &'static str {
    match trigger {
        TriggerBlock::WhileInputHeld { .. } => "While Input Held",
        TriggerBlock::OnInputToggle { .. } => "Toggle on Input",
        TriggerBlock::OnInputPressed { .. } => "Once on Press",
        TriggerBlock::LoopWhileActive => "Loop While Active",
    }
}
