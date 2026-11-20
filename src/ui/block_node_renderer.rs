use egui::{
    Align, Button, Color32, CornerRadius, Frame, Layout, Margin, Painter, Pos2, Rect, RichText,
    Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

use crate::models::{BlockId, BlockSlotTarget, StatementBlock, StatementKind, ValidationReport};
use crate::ui::block_field_editors::{render_container_header_fields, render_leaf_inline_fields};
use crate::ui::block_palette::{BlockDragPayload, BlockDragState};
use crate::ui::theme::{
    ACCENT_PRIMARY, BG_CANVAS, BORDER_STRONG, BORDER_SUBTLE, CORNER_RADIUS_SM_PX,
    FONT_SIZE_BODY_PX, FONT_SIZE_SMALL_PX, STATUS_DANGER, STATUS_DANGER_TINT, STATUS_RUNNING,
    STATUS_RUNNING_TINT, SURFACE_ELEVATED, SURFACE_HOVER, TEXT_MUTED, TEXT_PRIMARY, category_color,
};

const BLOCK_STRIPE_WIDTH_PX: f32 = 5.0;
const PUZZLE_NOTCH_OFFSET_X_PX: f32 = 24.0;
const PUZZLE_NOTCH_WIDTH_PX: f32 = 24.0;
const PUZZLE_NOTCH_DEPTH_PX: f32 = 4.0;
const C_BLOCK_SPINE_WIDTH_PX: f32 = 14.0;
const C_BLOCK_CHILD_INDENT_PX: f32 = 20.0;
const C_BLOCK_FOOTER_HEIGHT_PX: f32 = 16.0;
const BLOCK_VERTICAL_GAP_PX: f32 = 6.0;
const EMPTY_SLOT_HEIGHT_PX: f32 = 30.0;

/// Deferred structural tree mutation emitted while rendering statement blocks.
#[derive(Debug, Clone, Copy)]
pub enum BlockRenderMutation {
    MarkDirty,
    MoveBlock {
        block_id: BlockId,
        target_slot: BlockSlotTarget,
    },
    DeleteBlock(BlockId),
}

/// Shared read-only context passed down during recursive block rendering.
pub struct BlockRenderContext<'frame> {
    pub active_block_id: Option<BlockId>,
    pub validation_report: &'frame ValidationReport,
    pub pointer_pos: Option<Pos2>,
}

/// Renders a list of statement blocks and nested C-block branches with magnetic drop slots.
pub fn render_statement_stack(
    ui: &mut Ui,
    statements: &mut [StatementBlock],
    slot_for_index: impl Fn(usize) -> BlockSlotTarget + Copy,
    render_context: &BlockRenderContext<'_>,
    drag_state: &mut BlockDragState,
    mutations: &mut Vec<BlockRenderMutation>,
) {
    let stack_min_x_px = ui.cursor().min.x;
    let stack_max_x_px = stack_min_x_px + ui.available_width().max(260.0);

    if statements.is_empty() {
        render_empty_child_slot(
            ui,
            slot_for_index(0),
            stack_min_x_px,
            stack_max_x_px,
            render_context.pointer_pos,
            drag_state,
        );
        return;
    }

    if let Some(pointer_pos) = render_context.pointer_pos {
        let top_slot_y_px = ui.cursor().min.y;
        drag_state.offer_slot_candidate(
            pointer_pos,
            slot_for_index(0),
            top_slot_y_px,
            stack_min_x_px,
            stack_max_x_px,
        );
    }

    let statement_count = statements.len();
    for (block_index, block) in statements.iter_mut().enumerate() {
        let can_move_up = block_index > 0;
        let can_move_down = block_index.saturating_add(1) < statement_count;
        let move_up_slot = can_move_up.then(|| slot_for_index(block_index.saturating_sub(1)));
        let move_down_slot = can_move_down.then(|| slot_for_index(block_index.saturating_add(2)));

        render_single_block(
            ui,
            block,
            move_up_slot,
            move_down_slot,
            render_context,
            drag_state,
            mutations,
        );

        ui.add_space(BLOCK_VERTICAL_GAP_PX);

        if let Some(pointer_pos) = render_context.pointer_pos {
            let after_slot_y_px = ui.cursor().min.y - (BLOCK_VERTICAL_GAP_PX * 0.5);
            drag_state.offer_slot_candidate(
                pointer_pos,
                slot_for_index(block_index.saturating_add(1)),
                after_slot_y_px,
                stack_min_x_px,
                stack_max_x_px,
            );
        }
    }
}

fn render_empty_child_slot(
    ui: &mut Ui,
    target_slot: BlockSlotTarget,
    min_x_px: f32,
    max_x_px: f32,
    pointer_pos: Option<Pos2>,
    drag_state: &mut BlockDragState,
) {
    let slot_width_px = (max_x_px - min_x_px).max(200.0);
    let (slot_rect, _) =
        ui.allocate_exact_size(vec2(slot_width_px, EMPTY_SLOT_HEIGHT_PX), Sense::hover());

    let border_color = if drag_state.is_dragging_statement() {
        ACCENT_PRIMARY
    } else {
        BORDER_SUBTLE
    };
    ui.painter().rect(
        slot_rect,
        CornerRadius::same(CORNER_RADIUS_SM_PX),
        BG_CANVAS,
        Stroke::new(1.0, border_color),
        StrokeKind::Inside,
    );
    ui.painter().text(
        slot_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Drop block here",
        egui::FontId::proportional(FONT_SIZE_SMALL_PX),
        TEXT_MUTED,
    );

    if let Some(pointer) = pointer_pos {
        drag_state.offer_slot_candidate(
            pointer,
            target_slot,
            slot_rect.center().y,
            slot_rect.min.x,
            slot_rect.max.x,
        );
    }
}

fn render_single_block(
    ui: &mut Ui,
    block: &mut StatementBlock,
    move_up_slot: Option<BlockSlotTarget>,
    move_down_slot: Option<BlockSlotTarget>,
    render_context: &BlockRenderContext<'_>,
    drag_state: &mut BlockDragState,
    mutations: &mut Vec<BlockRenderMutation>,
) {
    let is_container = matches!(
        block.kind,
        StatementKind::IfThen { .. }
            | StatementKind::IfThenElse { .. }
            | StatementKind::WhileCondition { .. }
            | StatementKind::RepeatCount { .. }
    );
    if is_container {
        render_c_container_block(
            ui,
            block,
            move_up_slot,
            move_down_slot,
            render_context,
            drag_state,
            mutations,
        );
    } else {
        render_leaf_statement_block(
            ui,
            block,
            move_up_slot,
            move_down_slot,
            render_context,
            drag_state,
            mutations,
        );
    }
}

fn render_leaf_statement_block(
    ui: &mut Ui,
    block: &mut StatementBlock,
    move_up_slot: Option<BlockSlotTarget>,
    move_down_slot: Option<BlockSlotTarget>,
    render_context: &BlockRenderContext<'_>,
    drag_state: &mut BlockDragState,
    mutations: &mut Vec<BlockRenderMutation>,
) {
    let block_id = block.id;
    let accent_color = category_color(block.kind.category());
    let is_active = render_context.active_block_id == Some(block_id);
    let validation_message =
        find_block_validation_message(render_context.validation_report, block_id);
    let is_dragged_source = drag_state.dragged_existing_block_id() == Some(block_id);

    let fill_color = resolve_block_fill(is_active, validation_message.is_some(), is_dragged_source);
    let stroke = resolve_block_stroke(is_active, validation_message.is_some());

    let frame_response = Frame::NONE
        .fill(fill_color)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin {
            left: 12,
            right: 8,
            top: 7,
            bottom: 7,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                render_block_drag_handle(
                    ui,
                    block_id,
                    block.kind.title(),
                    accent_color,
                    drag_state,
                );
                ui.label(
                    RichText::new(block.kind.title())
                        .size(FONT_SIZE_BODY_PX)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                if render_leaf_inline_fields(ui, block_id, &mut block.kind) {
                    mutations.push(BlockRenderMutation::MarkDirty);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    render_block_toolbar_buttons(
                        ui,
                        block_id,
                        move_up_slot,
                        move_down_slot,
                        mutations,
                    );
                });
            });
        });

    paint_notched_puzzle_chrome(
        ui.painter(),
        frame_response.response.rect,
        accent_color,
        true,
        true,
    );

    if let Some(error_text) = validation_message {
        frame_response.response.on_hover_text(error_text);
    }
}

fn render_c_container_block(
    ui: &mut Ui,
    block: &mut StatementBlock,
    move_up_slot: Option<BlockSlotTarget>,
    move_down_slot: Option<BlockSlotTarget>,
    render_context: &BlockRenderContext<'_>,
    drag_state: &mut BlockDragState,
    mutations: &mut Vec<BlockRenderMutation>,
) {
    let block_id = block.id;
    let accent_color = category_color(block.kind.category());
    let is_active = render_context.active_block_id == Some(block_id);
    let validation_message =
        find_block_validation_message(render_context.validation_report, block_id);
    let is_dragged_source = drag_state.dragged_existing_block_id() == Some(block_id);

    let fill_color = resolve_block_fill(is_active, validation_message.is_some(), is_dragged_source);
    let stroke = resolve_block_stroke(is_active, validation_message.is_some());
    let container_left_x_px = ui.cursor().min.x;

    let header_response = Frame::NONE
        .fill(fill_color)
        .stroke(stroke)
        .corner_radius(CornerRadius {
            nw: CORNER_RADIUS_SM_PX,
            ne: CORNER_RADIUS_SM_PX,
            sw: 0,
            se: CORNER_RADIUS_SM_PX,
        })
        .inner_margin(Margin {
            left: 12,
            right: 8,
            top: 7,
            bottom: 7,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                render_block_drag_handle(
                    ui,
                    block_id,
                    block.kind.title(),
                    accent_color,
                    drag_state,
                );
                ui.label(
                    RichText::new(block.kind.title())
                        .size(FONT_SIZE_BODY_PX)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                if render_container_header_fields(ui, block_id, &mut block.kind) {
                    mutations.push(BlockRenderMutation::MarkDirty);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    render_block_toolbar_buttons(
                        ui,
                        block_id,
                        move_up_slot,
                        move_down_slot,
                        mutations,
                    );
                });
            });
        });

    let header_rect = header_response.response.rect;
    paint_notched_puzzle_chrome(ui.painter(), header_rect, accent_color, true, false);

    render_container_branches(
        ui,
        block_id,
        &mut block.kind,
        (accent_color, stroke),
        render_context,
        drag_state,
        mutations,
    );

    let footer_width_px = ui.available_width().max(220.0);
    let (footer_rect, _) = ui.allocate_exact_size(
        vec2(footer_width_px, C_BLOCK_FOOTER_HEIGHT_PX),
        Sense::hover(),
    );
    ui.painter().rect(
        footer_rect,
        CornerRadius {
            nw: 0,
            ne: CORNER_RADIUS_SM_PX,
            sw: CORNER_RADIUS_SM_PX,
            se: CORNER_RADIUS_SM_PX,
        },
        fill_color,
        stroke,
        StrokeKind::Inside,
    );

    let spine_rect = Rect::from_min_max(
        pos2(container_left_x_px, header_rect.max.y - 1.0),
        pos2(
            container_left_x_px + C_BLOCK_SPINE_WIDTH_PX,
            footer_rect.max.y,
        ),
    );
    ui.painter().rect_filled(
        spine_rect,
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: CORNER_RADIUS_SM_PX,
            se: 0,
        },
        accent_color,
    );

    paint_bottom_puzzle_tab(ui.painter(), footer_rect, accent_color);

    if let Some(error_text) = validation_message {
        header_response.response.on_hover_text(error_text);
    }
}

fn render_container_branches(
    ui: &mut Ui,
    parent_block_id: BlockId,
    statement_kind: &mut StatementKind,
    divider_style: (Color32, Stroke),
    render_context: &BlockRenderContext<'_>,
    drag_state: &mut BlockDragState,
    mutations: &mut Vec<BlockRenderMutation>,
) {
    let (accent_color, stroke) = divider_style;
    match statement_kind {
        StatementKind::IfThen { then_blocks, .. } => {
            render_indented_branch(
                ui,
                then_blocks,
                |slot_index| BlockSlotTarget::ThenBranch {
                    parent_block_id,
                    insert_index: slot_index,
                },
                render_context,
                drag_state,
                mutations,
            );
        }
        StatementKind::IfThenElse {
            then_blocks,
            else_blocks,
            ..
        } => {
            render_indented_branch(
                ui,
                then_blocks,
                |slot_index| BlockSlotTarget::ThenBranch {
                    parent_block_id,
                    insert_index: slot_index,
                },
                render_context,
                drag_state,
                mutations,
            );
            render_else_divider_bar(ui, accent_color, stroke);
            render_indented_branch(
                ui,
                else_blocks,
                |slot_index| BlockSlotTarget::ElseBranch {
                    parent_block_id,
                    insert_index: slot_index,
                },
                render_context,
                drag_state,
                mutations,
            );
        }
        StatementKind::WhileCondition { body_blocks, .. }
        | StatementKind::RepeatCount { body_blocks, .. } => {
            render_indented_branch(
                ui,
                body_blocks,
                |slot_index| BlockSlotTarget::LoopBody {
                    parent_block_id,
                    insert_index: slot_index,
                },
                render_context,
                drag_state,
                mutations,
            );
        }
        _ => {}
    }
}

fn render_indented_branch(
    ui: &mut Ui,
    child_blocks: &mut [StatementBlock],
    slot_for_index: impl Fn(usize) -> BlockSlotTarget + Copy,
    render_context: &BlockRenderContext<'_>,
    drag_state: &mut BlockDragState,
    mutations: &mut Vec<BlockRenderMutation>,
) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(C_BLOCK_CHILD_INDENT_PX);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            render_statement_stack(
                ui,
                child_blocks,
                slot_for_index,
                render_context,
                drag_state,
                mutations,
            );
        });
    });
    ui.add_space(4.0);
}

fn render_else_divider_bar(ui: &mut Ui, accent_color: Color32, stroke: Stroke) {
    Frame::NONE
        .fill(SURFACE_ELEVATED)
        .stroke(stroke)
        .corner_radius(CornerRadius {
            nw: 0,
            ne: CORNER_RADIUS_SM_PX,
            sw: 0,
            se: CORNER_RADIUS_SM_PX,
        })
        .inner_margin(Margin {
            left: 20,
            right: 8,
            top: 3,
            bottom: 3,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("Else")
                    .size(FONT_SIZE_SMALL_PX)
                    .strong()
                    .color(accent_color),
            );
        });
}

fn render_block_drag_handle(
    ui: &mut Ui,
    block_id: BlockId,
    block_title: &str,
    accent_color: Color32,
    drag_state: &mut BlockDragState,
) {
    let (grip_rect, grip_response) =
        ui.allocate_exact_size(vec2(16.0, 20.0), Sense::click_and_drag());
    let dot_color = if grip_response.hovered() {
        ACCENT_PRIMARY
    } else {
        TEXT_MUTED
    };
    let center_x_px = grip_rect.center().x;
    let center_y_px = grip_rect.center().y;
    for row_offset_px in [-4.0_f32, 0.0, 4.0] {
        ui.painter().circle_filled(
            pos2(center_x_px - 2.5, center_y_px + row_offset_px),
            1.2,
            dot_color,
        );
        ui.painter().circle_filled(
            pos2(center_x_px + 2.5, center_y_px + row_offset_px),
            1.2,
            dot_color,
        );
    }

    if grip_response
        .on_hover_text("Drag to reorder or nest block")
        .drag_started()
    {
        drag_state.start_drag(
            BlockDragPayload::MoveExistingBlock(block_id),
            block_title,
            accent_color,
        );
    }
}

fn render_block_toolbar_buttons(
    ui: &mut Ui,
    block_id: BlockId,
    move_up_slot: Option<BlockSlotTarget>,
    move_down_slot: Option<BlockSlotTarget>,
    mutations: &mut Vec<BlockRenderMutation>,
) {
    let delete_button = Button::new(
        RichText::new("x")
            .size(FONT_SIZE_SMALL_PX)
            .color(STATUS_DANGER),
    )
    .fill(Color32::TRANSPARENT)
    .stroke(Stroke::new(1.0, BORDER_SUBTLE));
    if ui
        .add(delete_button)
        .on_hover_text("Delete block")
        .clicked()
    {
        mutations.push(BlockRenderMutation::DeleteBlock(block_id));
    }

    let down_button = Button::new(
        RichText::new("v")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    )
    .fill(Color32::TRANSPARENT)
    .stroke(Stroke::new(1.0, BORDER_SUBTLE));
    if ui
        .add_enabled(move_down_slot.is_some(), down_button)
        .on_hover_text("Move block down")
        .clicked()
        && let Some(target_slot) = move_down_slot
    {
        mutations.push(BlockRenderMutation::MoveBlock {
            block_id,
            target_slot,
        });
    }

    let up_button = Button::new(
        RichText::new("^")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    )
    .fill(Color32::TRANSPARENT)
    .stroke(Stroke::new(1.0, BORDER_SUBTLE));
    if ui
        .add_enabled(move_up_slot.is_some(), up_button)
        .on_hover_text("Move block up")
        .clicked()
        && let Some(target_slot) = move_up_slot
    {
        mutations.push(BlockRenderMutation::MoveBlock {
            block_id,
            target_slot,
        });
    }
}

fn resolve_block_fill(
    is_active: bool,
    has_validation_error: bool,
    is_dragged_source: bool,
) -> Color32 {
    if is_active {
        return STATUS_RUNNING_TINT;
    }
    if has_validation_error {
        return STATUS_DANGER_TINT;
    }
    if is_dragged_source {
        return SURFACE_HOVER;
    }
    SURFACE_ELEVATED
}

fn resolve_block_stroke(is_active: bool, has_validation_error: bool) -> Stroke {
    if is_active {
        return Stroke::new(2.0, STATUS_RUNNING);
    }
    if has_validation_error {
        return Stroke::new(1.5, STATUS_DANGER);
    }
    Stroke::new(1.0, BORDER_STRONG)
}

fn find_block_validation_message(
    validation_report: &ValidationReport,
    block_id: BlockId,
) -> Option<&str> {
    validation_report
        .issues
        .iter()
        .find(|issue| issue.block_id == Some(block_id))
        .map(|issue| issue.message.as_str())
}

fn paint_notched_puzzle_chrome(
    painter: &Painter,
    block_rect: Rect,
    accent_color: Color32,
    draw_top_notch: bool,
    draw_bottom_tab: bool,
) {
    let stripe_rect = Rect::from_min_max(
        block_rect.min,
        pos2(block_rect.min.x + BLOCK_STRIPE_WIDTH_PX, block_rect.max.y),
    );
    painter.rect_filled(
        stripe_rect,
        CornerRadius {
            nw: CORNER_RADIUS_SM_PX,
            sw: CORNER_RADIUS_SM_PX,
            ne: 0,
            se: 0,
        },
        accent_color,
    );

    if draw_top_notch {
        let notch_rect = Rect::from_min_max(
            pos2(
                block_rect.min.x + PUZZLE_NOTCH_OFFSET_X_PX,
                block_rect.min.y,
            ),
            pos2(
                block_rect.min.x + PUZZLE_NOTCH_OFFSET_X_PX + PUZZLE_NOTCH_WIDTH_PX,
                block_rect.min.y + PUZZLE_NOTCH_DEPTH_PX,
            ),
        );
        painter.rect_filled(notch_rect, 1.0, BG_CANVAS);
    }

    if draw_bottom_tab {
        paint_bottom_puzzle_tab(painter, block_rect, accent_color);
    }
}

/// Paints the protruding bottom connector tab shared by trigger hats, leaf blocks, and C-footers.
pub(crate) fn paint_bottom_puzzle_tab(painter: &Painter, block_rect: Rect, accent_color: Color32) {
    let tab_rect = Rect::from_min_max(
        pos2(
            block_rect.min.x + PUZZLE_NOTCH_OFFSET_X_PX,
            block_rect.max.y - 1.0,
        ),
        pos2(
            block_rect.min.x + PUZZLE_NOTCH_OFFSET_X_PX + PUZZLE_NOTCH_WIDTH_PX,
            block_rect.max.y + PUZZLE_NOTCH_DEPTH_PX,
        ),
    );
    painter.rect_filled(tab_rect, 1.5, accent_color);
}
