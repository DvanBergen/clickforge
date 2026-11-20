use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};
use egui::{
    Align, Area, Button, Color32, ComboBox, CornerRadius, Frame, Id, Layout, Margin, Order, Pos2,
    Rect, RichText, ScrollArea, Stroke, StrokeKind, Ui, pos2, vec2,
};

use crate::models::{
    BlockCategory, BlockId, BlockSlotTarget, ScriptDocument, TriggerBlock, ValidationReport,
};
use crate::ui::block_field_editors::render_input_trigger_picker;
use crate::ui::block_node_renderer::{
    BlockRenderContext, BlockRenderMutation, paint_bottom_puzzle_tab, render_statement_stack,
};
use crate::ui::block_palette::{BlockDragPayload, BlockDragState};
use crate::ui::theme::{
    ACCENT_PRIMARY, BG_CANVAS, BORDER_SUBTLE, CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX,
    FONT_SIZE_SMALL_PX, INK_ON_ACCENT, SURFACE_ELEVATED, TEXT_MUTED, TEXT_PRIMARY, category_color,
};

const CANVAS_MAX_CONTENT_WIDTH_PX: f32 = 880.0;
const GRID_SPACING_PX: f32 = 24.0;
const GRID_DOT_RADIUS_PX: f32 = 1.0;
const HAT_TOP_CORNER_RADIUS_PX: u8 = 14;
const HAT_STRIPE_WIDTH_PX: f32 = 6.0;
const DROP_LINE_STROKE_WIDTH_PX: f32 = 3.0;
const DROP_ENDCAP_RADIUS_PX: f32 = 4.0;
const GHOST_OFFSET_X_PX: f32 = 14.0;
const GHOST_OFFSET_Y_PX: f32 = 12.0;

/// High-level actions emitted by the visual block canvas toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockCanvasAction {
    MarkDirty,
    ExportToRust,
}

/// Renders the Scratch-style visual block workspace, trigger hat, and drag-and-drop canvas.
pub fn render_block_canvas(
    ui: &mut Ui,
    script: &mut ScriptDocument,
    active_block_id: Option<BlockId>,
    validation_report: &ValidationReport,
    drag_state: &mut BlockDragState,
    emitted_actions: &mut Vec<BlockCanvasAction>,
) {
    drag_state.reset_frame_targets();

    let canvas_rect = ui.available_rect_before_wrap();
    paint_subtle_canvas_dot_grid(ui, canvas_rect);

    render_canvas_toolbar(ui, script, emitted_actions);
    ui.add_space(6.0);

    let pointer_pos = ui.input(|input| input.pointer.hover_pos());
    let mut render_mutations = Vec::new();
    let mut trigger_did_change = false;

    ScrollArea::both()
        .id_salt(("block_canvas_scroll", script.id.as_str()))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_max_width(CANVAS_MAX_CONTENT_WIDTH_PX);
            ui.add_space(4.0);

            trigger_did_change = render_trigger_hat_block(
                ui,
                &script.id,
                &mut script.visual_program.trigger,
                pointer_pos,
                drag_state,
            );

            ui.add_space(8.0);

            let render_context = BlockRenderContext {
                active_block_id,
                validation_report,
                pointer_pos,
            };
            render_statement_stack(
                ui,
                &mut script.visual_program.statements,
                |slot_index| BlockSlotTarget::Root {
                    insert_index: slot_index,
                },
                &render_context,
                drag_state,
                &mut render_mutations,
            );

            ui.add_space(48.0);
        });

    if trigger_did_change {
        emitted_actions.push(BlockCanvasAction::MarkDirty);
    }

    if apply_render_mutations(script, render_mutations) {
        emitted_actions.push(BlockCanvasAction::MarkDirty);
    }

    paint_drop_indicator_overlay(ui, drag_state);
    paint_drag_ghost_preview(ui, pointer_pos, drag_state);

    let is_pointer_down = ui.input(|input| input.pointer.primary_down());
    let is_escape_pressed = ui.input(|input| input.key_pressed(egui::Key::Escape));

    if is_escape_pressed {
        drag_state.active_drag = None;
        return;
    }

    if drag_state.active_drag.is_some() && !is_pointer_down {
        let did_drop_mutate = finalize_active_drag(script, canvas_rect, pointer_pos, drag_state);
        if did_drop_mutate {
            emitted_actions.push(BlockCanvasAction::MarkDirty);
        }
    }
}

fn render_canvas_toolbar(
    ui: &mut Ui,
    script: &ScriptDocument,
    emitted_actions: &mut Vec<BlockCanvasAction>,
) {
    let block_count = script.visual_program.total_block_count();
    let nesting_depth = script.visual_program.max_nesting_depth();

    Frame::NONE
        .fill(Color32::from_rgba_unmultiplied(0x18, 0x1B, 0x21, 220))
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{block_count} blocks   |   Nesting depth: {nesting_depth}"
                    ))
                    .size(FONT_SIZE_SMALL_PX)
                    .monospace()
                    .color(TEXT_MUTED),
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let export_button = Button::new(
                        RichText::new("Export to Rust")
                            .size(FONT_SIZE_SMALL_PX)
                            .strong()
                            .color(INK_ON_ACCENT),
                    )
                    .fill(ACCENT_PRIMARY)
                    .stroke(Stroke::NONE)
                    .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX));

                    if ui
                        .add(export_button)
                        .on_hover_text(
                            "Generate idiomatic clickforge_sdk Rust source from these visual blocks and switch to Advanced Rust mode",
                        )
                        .clicked()
                    {
                        emitted_actions.push(BlockCanvasAction::ExportToRust);
                    }
                });
            });
        });
}

fn render_trigger_hat_block(
    ui: &mut Ui,
    script_id: &str,
    trigger: &mut TriggerBlock,
    pointer_pos: Option<Pos2>,
    drag_state: &mut BlockDragState,
) -> bool {
    let trigger_color = category_color(BlockCategory::Trigger);
    let mut did_change = false;

    let mut frame = Frame::NONE
        .fill(SURFACE_ELEVATED)
        .stroke(Stroke::new(1.5, trigger_color))
        .corner_radius(CornerRadius {
            nw: HAT_TOP_CORNER_RADIUS_PX,
            ne: HAT_TOP_CORNER_RADIUS_PX,
            sw: CORNER_RADIUS_SM_PX,
            se: CORNER_RADIUS_SM_PX,
        })
        .inner_margin(Margin {
            left: 14,
            right: 12,
            top: 9,
            bottom: 9,
        });

    if drag_state.is_dragging_trigger() {
        frame = frame.stroke(Stroke::new(2.0, ACCENT_PRIMARY));
    }

    let hat_response = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("Trigger")
                    .size(FONT_SIZE_SMALL_PX)
                    .strong()
                    .color(trigger_color),
            );

            ComboBox::from_id_salt(("trigger_mode", script_id))
                .selected_text(trigger_mode_label(trigger))
                .width(156.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            matches!(trigger, TriggerBlock::WhileInputHeld { .. }),
                            "While Input Held",
                        )
                        .clicked()
                    {
                        let current_input = trigger
                            .trigger_input()
                            .unwrap_or(InputTrigger::Mouse(MouseButton::Left));
                        *trigger = TriggerBlock::WhileInputHeld {
                            trigger_input: current_input,
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
                        let current_input = trigger
                            .trigger_input()
                            .unwrap_or(InputTrigger::Key(KeyCode::F5));
                        *trigger = TriggerBlock::OnInputToggle {
                            trigger_input: current_input,
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
                        let current_input = trigger
                            .trigger_input()
                            .unwrap_or(InputTrigger::Mouse(MouseButton::Side));
                        *trigger = TriggerBlock::OnInputPressed {
                            trigger_input: current_input,
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
                });

            match trigger {
                TriggerBlock::WhileInputHeld { trigger_input }
                | TriggerBlock::OnInputToggle { trigger_input }
                | TriggerBlock::OnInputPressed { trigger_input } => {
                    did_change |=
                        render_input_trigger_picker(ui, ("hat_input", script_id), trigger_input);
                }
                TriggerBlock::LoopWhileActive => {
                    ui.label(
                        RichText::new("Runs continuously while armed and focused")
                            .size(FONT_SIZE_SMALL_PX)
                            .color(TEXT_MUTED),
                    );
                }
            }
        });
    });

    let hat_rect = hat_response.response.rect;
    let stripe_rect = Rect::from_min_max(
        hat_rect.min,
        pos2(hat_rect.min.x + HAT_STRIPE_WIDTH_PX, hat_rect.max.y),
    );
    ui.painter().rect_filled(
        stripe_rect,
        CornerRadius {
            nw: HAT_TOP_CORNER_RADIUS_PX,
            sw: CORNER_RADIUS_SM_PX,
            ne: 0,
            se: 0,
        },
        trigger_color,
    );
    paint_bottom_puzzle_tab(ui.painter(), hat_rect, trigger_color);

    if drag_state.is_dragging_trigger()
        && let Some(pointer) = pointer_pos
        && hat_rect.expand(12.0).contains(pointer)
    {
        drag_state.is_hovering_trigger_hat = true;
        ui.painter().rect_stroke(
            hat_rect.expand(2.0),
            CornerRadius::same(HAT_TOP_CORNER_RADIUS_PX),
            Stroke::new(2.0, ACCENT_PRIMARY),
            StrokeKind::Outside,
        );
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

fn apply_render_mutations(
    script: &mut ScriptDocument,
    mutations: Vec<BlockRenderMutation>,
) -> bool {
    let mut did_modify = false;
    for mutation in mutations {
        match mutation {
            BlockRenderMutation::MarkDirty => {
                did_modify = true;
            }
            BlockRenderMutation::MoveBlock {
                block_id,
                target_slot,
            } => {
                if script.visual_program.move_block(block_id, target_slot) {
                    did_modify = true;
                }
            }
            BlockRenderMutation::DeleteBlock(block_id) => {
                if script.visual_program.remove_block(block_id).is_some() {
                    did_modify = true;
                }
            }
        }
    }
    did_modify
}

fn finalize_active_drag(
    script: &mut ScriptDocument,
    canvas_rect: Rect,
    pointer_pos: Option<Pos2>,
    drag_state: &mut BlockDragState,
) -> bool {
    let Some(active_drag) = drag_state.active_drag.take() else {
        return false;
    };
    let hovered_slot = drag_state.hovered_slot.take();
    let is_inside_canvas = pointer_pos.is_some_and(|pointer| canvas_rect.contains(pointer));

    match active_drag.payload {
        BlockDragPayload::ReplaceTrigger(new_trigger) => {
            if drag_state.is_hovering_trigger_hat || is_inside_canvas {
                script.visual_program.trigger = new_trigger;
                return true;
            }
            false
        }
        BlockDragPayload::NewStatement(statement_kind) => {
            if let Some(slot) = hovered_slot {
                let new_block = script.visual_program.create_block(statement_kind);
                return script
                    .visual_program
                    .insert_block(slot.target_slot, new_block);
            }
            if is_inside_canvas {
                script.visual_program.append_root_block(statement_kind);
                return true;
            }
            false
        }
        BlockDragPayload::MoveExistingBlock(source_block_id) => {
            let Some(slot) = hovered_slot else {
                return false;
            };
            script
                .visual_program
                .move_block(source_block_id, slot.target_slot)
        }
    }
}

fn paint_drop_indicator_overlay(ui: &Ui, drag_state: &BlockDragState) {
    let Some(hovered_slot) = drag_state.hovered_slot else {
        return;
    };
    let line_left = pos2(
        hovered_slot.indicator_min_x_px,
        hovered_slot.indicator_line_y_px,
    );
    let line_right = pos2(
        hovered_slot.indicator_max_x_px,
        hovered_slot.indicator_line_y_px,
    );
    let overlay_painter = ui.ctx().layer_painter(egui::LayerId::new(
        Order::Foreground,
        Id::unique("block_drop_indicator_layer"),
    ));
    overlay_painter.line_segment(
        [line_left, line_right],
        Stroke::new(DROP_LINE_STROKE_WIDTH_PX, ACCENT_PRIMARY),
    );
    overlay_painter.circle_filled(line_left, DROP_ENDCAP_RADIUS_PX, ACCENT_PRIMARY);
    overlay_painter.circle_filled(line_right, DROP_ENDCAP_RADIUS_PX, ACCENT_PRIMARY);
}

fn paint_drag_ghost_preview(ui: &Ui, pointer_pos: Option<Pos2>, drag_state: &BlockDragState) {
    let Some(active_drag) = &drag_state.active_drag else {
        return;
    };
    let Some(pointer) = pointer_pos else {
        return;
    };

    Area::new(Id::unique("block_drag_ghost_area"))
        .order(Order::Tooltip)
        .interactable(false)
        .fixed_pos(pos2(
            pointer.x + GHOST_OFFSET_X_PX,
            pointer.y + GHOST_OFFSET_Y_PX,
        ))
        .show(ui.ctx(), |ui| {
            Frame::NONE
                .fill(SURFACE_ELEVATED)
                .stroke(Stroke::new(1.5, active_drag.accent_color))
                .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (dot_rect, _) =
                            ui.allocate_exact_size(vec2(6.0, 14.0), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(dot_rect, 2.0, active_drag.accent_color);
                        ui.label(
                            RichText::new(&active_drag.preview_label)
                                .size(FONT_SIZE_BODY_PX)
                                .strong()
                                .color(TEXT_PRIMARY),
                        );
                    });
                });
        });
}

fn paint_subtle_canvas_dot_grid(ui: &Ui, canvas_rect: Rect) {
    let painter = ui.painter_at(canvas_rect);
    painter.rect_filled(canvas_rect, 0.0, BG_CANVAS);

    let dot_color = Color32::from_rgba_unmultiplied(0x2A, 0x30, 0x3C, 150);
    let mut current_y_px = (canvas_rect.min.y / GRID_SPACING_PX).ceil() * GRID_SPACING_PX;
    let max_columns: usize = 120;
    let max_rows: usize = 80;
    let mut row_count: usize = 0;

    while current_y_px < canvas_rect.max.y && row_count < max_rows {
        let mut current_x_px = (canvas_rect.min.x / GRID_SPACING_PX).ceil() * GRID_SPACING_PX;
        let mut column_count: usize = 0;
        while current_x_px < canvas_rect.max.x && column_count < max_columns {
            painter.circle_filled(
                pos2(current_x_px, current_y_px),
                GRID_DOT_RADIUS_PX,
                dot_color,
            );
            current_x_px += GRID_SPACING_PX;
            column_count = column_count.saturating_add(1);
        }
        current_y_px += GRID_SPACING_PX;
        row_count = row_count.saturating_add(1);
    }
}
