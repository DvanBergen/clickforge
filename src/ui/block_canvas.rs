mod tree_mutations;
mod trigger_hat;

use egui::{
    Align, Area, Button, Color32, CornerRadius, CursorIcon, Frame, Id, Layout, Margin, Order, Pos2,
    Rect, ScrollArea, Shape, Stroke, Ui, pos2,
};

use crate::models::{
    BlockCategory, BlockId, BlockSlotTarget, CustomBlockDefinition, ScriptDocument, StatementBlock,
    StatementKind, ValidationReport, lookup_custom_block,
};
use crate::ui::block_node_renderer::{
    BlockRenderContext, BlockRenderMutation, FULL_BLOCK_GEOMETRY, build_leaf_block_shapes,
    build_trigger_hat_shapes, render_statement_stack,
};
use crate::ui::block_palette::{BlockDragPayload, BlockDragState};
use crate::ui::theme::{
    ACCENT_PRIMARY, BG_CANVAS, BORDER_STRONG, BORDER_SUBTLE, CORNER_RADIUS_SM_PX,
    FONT_SIZE_BODY_PX, FONT_SIZE_SMALL_PX, STATUS_DANGER_INK, STATUS_DANGER_SOLID,
    STATUS_DANGER_TINT, SURFACE_ELEVATED, SURFACE_PANEL, TEXT_MUTED, TEXT_PRIMARY,
    block_visual_colors, custom_block_visual_colors, rich_mono, rich_semibold,
};

use tree_mutations::{
    clone_block_slice_with_fresh_ids, clone_block_with_fresh_ids, replace_block_with_slice_in_tree,
};
use trigger_hat::render_trigger_hat_block;

const GRID_SPACING_PX: f32 = 22.0;
const GRID_DOT_RADIUS_PX: f32 = 1.1;
const GRID_DOT_COLOR: Color32 = Color32::from_rgb(0x2D, 0x34, 0x42);
const MAX_GRID_COLUMNS: i32 = 140;
const MAX_GRID_ROWS: i32 = 100;
const DROP_GLOW_WIDTH_PX: f32 = 7.0;
const DROP_LINE_STROKE_WIDTH_PX: f32 = 3.0;
const DROP_ENDCAP_RADIUS_PX: f32 = 4.5;
const GHOST_OFFSET_X_PX: f32 = 14.0;
const GHOST_OFFSET_Y_PX: f32 = 10.0;
const CANVAS_SCROLL_PADDING_PX: f32 = 14.0;
const CANVAS_BOTTOM_BREATHING_PX: f32 = 64.0;
const MAX_GHOST_SUMMARY_CHARS: usize = 36;

/// High-level actions emitted by the visual block canvas toolbar and context menu.
#[derive(Debug, Clone)]
pub enum BlockCanvasAction {
    MarkDirty,
    ExportToRust,
    EditCustomBlock(String),
    UnpackCustomBlock {
        tile_name: String,
        unpacked_block_count: usize,
    },
    SaveBlockAsCustomTile {
        suggested_name: String,
        statements: Vec<StatementBlock>,
    },
}

/// Renders the Scratch-style visual block workspace, trigger hat, and drag-and-drop canvas.
pub fn render_block_canvas(
    ui: &mut Ui,
    script: &mut ScriptDocument,
    active_block_id: Option<BlockId>,
    validation_report: &ValidationReport,
    custom_blocks: &[CustomBlockDefinition],
    drag_state: &mut BlockDragState,
    emitted_actions: &mut Vec<BlockCanvasAction>,
) {
    drag_state.reset_frame_targets();

    let canvas_rect = ui.available_rect_before_wrap();
    ui.painter_at(canvas_rect)
        .rect_filled(canvas_rect, 0.0, BG_CANVAS);

    render_canvas_toolbar(ui, script, validation_report, emitted_actions);
    ui.add_space(6.0);

    let pointer_pos = ui.input(|input| input.pointer.hover_pos());
    let mut render_mutations = Vec::new();
    let mut trigger_did_change = false;

    ScrollArea::both()
        .id_salt(("block_canvas_scroll", script.id.as_str()))
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, _viewport_rect| {
            paint_scroll_synced_dot_grid(ui);

            Frame::NONE
                .inner_margin(Margin::same(CANVAS_SCROLL_PADDING_PX as i8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;

                    trigger_did_change = render_trigger_hat_block(
                        ui,
                        &script.id,
                        &mut script.visual_program.trigger,
                        pointer_pos,
                        drag_state,
                    );

                    let render_context = BlockRenderContext {
                        active_block_id,
                        validation_report,
                        custom_blocks,
                        pointer_pos,
                    };
                    render_statement_stack(
                        ui,
                        &mut script.visual_program.statements,
                        |slot_index| BlockSlotTarget::Root {
                            insert_index: slot_index,
                        },
                        0,
                        &render_context,
                        drag_state,
                        &mut render_mutations,
                    );

                    ui.add_space(CANVAS_BOTTOM_BREATHING_PX);
                });
        });

    if trigger_did_change {
        emitted_actions.push(BlockCanvasAction::MarkDirty);
    }

    if apply_render_mutations(script, custom_blocks, render_mutations, emitted_actions) {
        emitted_actions.push(BlockCanvasAction::MarkDirty);
    }

    if drag_state.is_dragging() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }

    paint_drop_indicator_overlay(ui, drag_state);
    paint_drag_ghost_preview(ui, pointer_pos, drag_state);

    let is_pointer_down = ui.input(|input| input.pointer.primary_down());
    let is_escape_pressed = ui.input(|input| input.key_pressed(egui::Key::Escape));

    if is_escape_pressed {
        drag_state.active_drag = None;
        drag_state.is_hovering_trash_zone = false;
        return;
    }

    if drag_state.is_dragging() && !is_pointer_down {
        let did_drop_mutate = finalize_active_drag(script, canvas_rect, pointer_pos, drag_state);
        if did_drop_mutate {
            emitted_actions.push(BlockCanvasAction::MarkDirty);
        }
    }
}

fn render_canvas_toolbar(
    ui: &mut Ui,
    script: &ScriptDocument,
    validation_report: &ValidationReport,
    emitted_actions: &mut Vec<BlockCanvasAction>,
) {
    let block_count = script.visual_program.total_block_count();
    let nesting_depth = script.visual_program.max_nesting_depth();
    let block_issue_count = validation_report
        .issues
        .iter()
        .filter(|issue| issue.block_id.is_some())
        .count();

    Frame::NONE
        .fill(Color32::from_rgba_unmultiplied(
            SURFACE_PANEL.r(),
            SURFACE_PANEL.g(),
            SURFACE_PANEL.b(),
            235,
        ))
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(rich_mono(
                    format!("{block_count} blocks  |  Depth {nesting_depth}"),
                    FONT_SIZE_SMALL_PX,
                    TEXT_MUTED,
                ));

                if block_issue_count > 0 {
                    Frame::NONE
                        .fill(STATUS_DANGER_TINT)
                        .stroke(Stroke::new(1.0, STATUS_DANGER_SOLID))
                        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
                        .inner_margin(Margin::symmetric(6, 1))
                        .show(ui, |ui| {
                            let issue_label = if block_issue_count == 1 {
                                "1 block issue".to_string()
                            } else {
                                format!("{block_issue_count} block issues")
                            };
                            ui.label(rich_semibold(
                                issue_label,
                                FONT_SIZE_SMALL_PX,
                                STATUS_DANGER_INK,
                            ));
                        });
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let export_button = Button::new(rich_semibold(
                        "Export to Rust",
                        FONT_SIZE_SMALL_PX,
                        TEXT_PRIMARY,
                    ))
                    .fill(SURFACE_ELEVATED)
                    .stroke(Stroke::new(1.0, BORDER_STRONG))
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

                    let has_statements = !script.visual_program.statements.is_empty();
                    let save_tile_button = Button::new(rich_semibold(
                        "Save Stack as Tile",
                        FONT_SIZE_SMALL_PX,
                        if has_statements {
                            TEXT_PRIMARY
                        } else {
                            TEXT_MUTED
                        },
                    ))
                    .fill(SURFACE_ELEVATED)
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                    .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX));

                    let save_tile_response = ui.add_enabled(has_statements, save_tile_button);
                    if save_tile_response.clicked() {
                        emitted_actions.push(BlockCanvasAction::SaveBlockAsCustomTile {
                            suggested_name: script.name.clone(),
                            statements: script.visual_program.statements.clone(),
                        });
                    }
                    if has_statements {
                        save_tile_response.on_hover_text(
                            "Save this script's statement stack as a reusable Custom Tile in My Blocks",
                        );
                    } else {
                        save_tile_response
                            .on_disabled_hover_text("Add at least one block before saving as a tile");
                    }
                });
            });
        });
}

fn apply_render_mutations(
    script: &mut ScriptDocument,
    custom_blocks: &[CustomBlockDefinition],
    mutations: Vec<BlockRenderMutation>,
    emitted_actions: &mut Vec<BlockCanvasAction>,
) -> bool {
    let mut did_modify = false;
    for mutation in mutations {
        match mutation {
            BlockRenderMutation::MarkDirty => did_modify = true,
            BlockRenderMutation::MoveBlock {
                block_id,
                target_slot,
            } => {
                did_modify |= script.visual_program.move_block(block_id, target_slot);
            }
            BlockRenderMutation::DuplicateBlock {
                block_id,
                insert_slot,
            } => {
                let Some(source_clone) = script.visual_program.find_block(block_id).cloned() else {
                    continue;
                };
                let duplicated_block =
                    clone_block_with_fresh_ids(&mut script.visual_program, &source_clone);
                did_modify |= script
                    .visual_program
                    .insert_block(insert_slot, duplicated_block);
            }
            BlockRenderMutation::DeleteBlock(block_id) => {
                did_modify |= script.visual_program.remove_block(block_id).is_some();
            }
            BlockRenderMutation::SaveBlockAsCustomTile(block_id) => {
                let Some(source_clone) = script.visual_program.find_block(block_id).cloned() else {
                    continue;
                };
                let suggested_name = match &source_clone.kind {
                    StatementKind::RunCustomBlock { cached_name, .. }
                        if !cached_name.trim().is_empty() =>
                    {
                        format!("{} Copy", cached_name.trim())
                    }
                    other_kind => other_kind.title().to_string(),
                };
                emitted_actions.push(BlockCanvasAction::SaveBlockAsCustomTile {
                    suggested_name,
                    statements: vec![source_clone],
                });
            }
            BlockRenderMutation::EditCustomBlock(custom_block_id) => {
                emitted_actions.push(BlockCanvasAction::EditCustomBlock(custom_block_id));
            }
            BlockRenderMutation::UnpackCustomBlock {
                block_id,
                custom_block_id,
            } => {
                let Some(definition) = lookup_custom_block(custom_blocks, &custom_block_id) else {
                    continue;
                };
                let unpacked_blocks = clone_block_slice_with_fresh_ids(
                    &mut script.visual_program,
                    &definition.statements,
                );
                let unpacked_block_count = definition.total_block_count();
                if replace_block_with_slice_in_tree(
                    &mut script.visual_program.statements,
                    block_id,
                    unpacked_blocks,
                ) {
                    did_modify = true;
                    emitted_actions.push(BlockCanvasAction::UnpackCustomBlock {
                        tile_name: definition.name.clone(),
                        unpacked_block_count,
                    });
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
    let dropped_on_trash = drag_state.is_hovering_trash_zone
        || pointer_pos.is_some_and(|pointer| {
            drag_state
                .palette_rect
                .is_some_and(|palette_rect| palette_rect.expand(4.0).contains(pointer))
        });
    drag_state.is_hovering_trash_zone = false;

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
            if dropped_on_trash {
                return false;
            }
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
            if dropped_on_trash {
                return script
                    .visual_program
                    .remove_block(source_block_id)
                    .is_some();
            }
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
    if drag_state.is_hovering_trash_zone {
        return;
    }
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

    let glow_color = Color32::from_rgba_unmultiplied(
        ACCENT_PRIMARY.r(),
        ACCENT_PRIMARY.g(),
        ACCENT_PRIMARY.b(),
        72,
    );
    overlay_painter.line_segment(
        [line_left, line_right],
        Stroke::new(DROP_GLOW_WIDTH_PX, glow_color),
    );
    overlay_painter.line_segment(
        [line_left, line_right],
        Stroke::new(DROP_LINE_STROKE_WIDTH_PX, ACCENT_PRIMARY),
    );

    let notch_left_x_px = line_left.x + FULL_BLOCK_GEOMETRY.notch_offset_x_px;
    let notch_width_px = FULL_BLOCK_GEOMETRY.notch_outer_width_px;
    let notch_slope_px = FULL_BLOCK_GEOMETRY.notch_slope_x_px;
    let notch_depth_px = FULL_BLOCK_GEOMETRY.notch_depth_px;
    if notch_left_x_px + notch_width_px < line_right.x - 10.0 {
        let notch_bump = vec![
            pos2(notch_left_x_px, line_left.y),
            pos2(notch_left_x_px + notch_width_px, line_left.y),
            pos2(
                notch_left_x_px + notch_width_px - notch_slope_px,
                line_left.y + notch_depth_px,
            ),
            pos2(
                notch_left_x_px + notch_slope_px,
                line_left.y + notch_depth_px,
            ),
        ];
        overlay_painter.add(Shape::convex_polygon(
            notch_bump,
            ACCENT_PRIMARY,
            Stroke::NONE,
        ));
    }

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

    let is_deleting = drag_state.is_hovering_trash_zone && drag_state.is_dragging_existing_block();
    let visual_colors = active_drag
        .custom_color
        .map(custom_block_visual_colors)
        .unwrap_or_else(|| block_visual_colors(active_drag.category));
    let summary_text = truncate_summary(&active_drag.preview_summary, MAX_GHOST_SUMMARY_CHARS);

    Area::new(Id::unique("block_drag_ghost_area"))
        .order(Order::Tooltip)
        .interactable(false)
        .fixed_pos(pos2(
            pointer.x + GHOST_OFFSET_X_PX,
            pointer.y + GHOST_OFFSET_Y_PX,
        ))
        .show(ui.ctx(), |ui| {
            let background_shape_index = ui.painter().add(Shape::Noop);
            let is_trigger = active_drag.category == BlockCategory::Trigger;
            let top_pad_px = if is_trigger {
                (FULL_BLOCK_GEOMETRY.hat_dome_height_px as i8) + 6
            } else {
                (FULL_BLOCK_GEOMETRY.notch_depth_px as i8) + 5
            };

            let ghost_frame = Frame::NONE
                .inner_margin(Margin {
                    left: 12,
                    right: 14,
                    top: top_pad_px,
                    bottom: 7,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let title_color = if is_deleting {
                            STATUS_DANGER_INK
                        } else {
                            visual_colors.text_primary
                        };
                        ui.label(rich_semibold(
                            &active_drag.preview_label,
                            FONT_SIZE_BODY_PX,
                            title_color,
                        ));
                        if is_deleting {
                            ui.label(rich_semibold(
                                "Release to delete",
                                FONT_SIZE_SMALL_PX,
                                TEXT_PRIMARY,
                            ));
                        } else if !summary_text.is_empty() {
                            ui.label(rich_mono(
                                summary_text,
                                FONT_SIZE_SMALL_PX,
                                visual_colors.text_muted,
                            ));
                        }
                    });
                });

            let ghost_rect = ghost_frame.response.rect;
            let (fill_color, border_color) = if is_deleting {
                (STATUS_DANGER_TINT, STATUS_DANGER_SOLID)
            } else {
                (visual_colors.fill_hover, visual_colors.border)
            };
            let shapes = if is_trigger {
                build_trigger_hat_shapes(
                    ghost_rect,
                    FULL_BLOCK_GEOMETRY,
                    fill_color,
                    border_color,
                    visual_colors.top_highlight,
                    FULL_BLOCK_GEOMETRY.stroke_width_px,
                )
            } else {
                build_leaf_block_shapes(
                    ghost_rect,
                    FULL_BLOCK_GEOMETRY,
                    fill_color,
                    border_color,
                    visual_colors.top_highlight,
                    FULL_BLOCK_GEOMETRY.stroke_width_px,
                    true,
                    true,
                )
            };
            ui.painter().set(background_shape_index, Shape::Vec(shapes));
        });
}

fn paint_scroll_synced_dot_grid(ui: &Ui) {
    let visible_rect = ui.clip_rect();
    let painter = ui.painter_at(visible_rect);
    painter.rect_filled(visible_rect, 0.0, BG_CANVAS);

    let content_origin = ui.cursor().min;
    let start_column = ((visible_rect.min.x - content_origin.x) / GRID_SPACING_PX).floor() as i32;
    let end_column = (((visible_rect.max.x - content_origin.x) / GRID_SPACING_PX).ceil() as i32)
        .min(start_column.saturating_add(MAX_GRID_COLUMNS));

    let start_row = ((visible_rect.min.y - content_origin.y) / GRID_SPACING_PX).floor() as i32;
    let end_row = (((visible_rect.max.y - content_origin.y) / GRID_SPACING_PX).ceil() as i32)
        .min(start_row.saturating_add(MAX_GRID_ROWS));

    for row_index in start_row..=end_row {
        let dot_y_px = content_origin.y + (row_index as f32) * GRID_SPACING_PX;
        if dot_y_px < visible_rect.min.y || dot_y_px > visible_rect.max.y {
            continue;
        }
        for column_index in start_column..=end_column {
            let dot_x_px = content_origin.x + (column_index as f32) * GRID_SPACING_PX;
            if dot_x_px < visible_rect.min.x || dot_x_px > visible_rect.max.x {
                continue;
            }
            painter.circle_filled(pos2(dot_x_px, dot_y_px), GRID_DOT_RADIUS_PX, GRID_DOT_COLOR);
        }
    }
}

fn truncate_summary(raw_summary: &str, max_chars: usize) -> String {
    if raw_summary.chars().count() <= max_chars {
        return raw_summary.to_string();
    }
    let mut trimmed: String = raw_summary
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect();
    trimmed.push('…');
    trimmed
}
