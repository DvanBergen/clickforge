mod geometry;

use egui::{
    Align2, Button, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Margin, PointerButton,
    Popup, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, UiBuilder, pos2, vec2,
};

use crate::models::{BlockId, BlockSlotTarget, StatementBlock, StatementKind, ValidationReport};
use crate::ui::block_field_editors::{render_container_block_sentence, render_leaf_block_sentence};
use crate::ui::block_palette::{BlockDragPayload, BlockDragState};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BORDER_SUBTLE, BlockVisualColors, CORNER_RADIUS_SM_PX,
    FONT_SIZE_SMALL_PX, STATUS_DANGER, STATUS_DANGER_INK, STATUS_DANGER_SOLID, STATUS_DANGER_TINT,
    STATUS_RUNNING, STATUS_RUNNING_TINT, TEXT_MUTED, TEXT_PRIMARY, block_visual_colors,
    font_family_semibold, rich_semibold,
};

pub(crate) use geometry::{
    FULL_BLOCK_GEOMETRY, MINI_BLOCK_GEOMETRY, build_leaf_block_shapes, build_trigger_hat_shapes,
};

use geometry::{
    C_BLOCK_SPINE_WIDTH_PX, build_c_block_shapes, paint_close_icon, paint_dashed_rect_border,
    paint_duplicate_icon, with_alpha,
};

pub(crate) const BLOCK_MIN_WIDTH_PX: f32 = 280.0;
pub(crate) const BLOCK_MAX_WIDTH_PX: f32 = 740.0;

const NESTED_BLOCK_MIN_WIDTH_PX: f32 = 248.0;
const C_BLOCK_FOOTER_HEIGHT_PX: f32 = 22.0;
const C_BLOCK_ELSE_MIN_HEIGHT_PX: f32 = 28.0;
const C_BLOCK_MIN_FOOTER_WIDTH_PX: f32 = 220.0;
const C_BLOCK_MAX_FOOTER_WIDTH_PX: f32 = 340.0;
const BLOCK_VERTICAL_GAP_PX: f32 = 0.0;
const EMPTY_POCKET_HEIGHT_PX: f32 = 34.0;
const EMPTY_ROOT_HEIGHT_PX: f32 = 46.0;
const DRAG_DIM_OPACITY: f32 = 0.30;
const HOVER_ACTION_SIZE_PX: f32 = 20.0;
const HOVER_ACTION_GAP_PX: f32 = 4.0;
const HOVER_RIGHT_RESERVE_PX: i8 = 54;

/// Deferred structural tree mutation emitted while rendering statement blocks.
#[derive(Debug, Clone, Copy)]
pub enum BlockRenderMutation {
    MarkDirty,
    MoveBlock {
        block_id: BlockId,
        target_slot: BlockSlotTarget,
    },
    DuplicateBlock {
        block_id: BlockId,
        insert_slot: BlockSlotTarget,
    },
    DeleteBlock(BlockId),
}

/// Shared read-only context passed down during recursive block rendering.
pub struct BlockRenderContext<'frame> {
    pub active_block_id: Option<BlockId>,
    pub validation_report: &'frame ValidationReport,
    pub pointer_pos: Option<Pos2>,
}

struct BlockNavigationSlots {
    move_up_slot: Option<BlockSlotTarget>,
    move_down_slot: Option<BlockSlotTarget>,
    duplicate_slot: BlockSlotTarget,
}

/// Renders a snapped stack of statement blocks and nested C-block branches with magnetic drop slots.
pub fn render_statement_stack(
    ui: &mut Ui,
    statements: &mut [StatementBlock],
    slot_for_index: impl Fn(usize) -> BlockSlotTarget + Copy,
    nesting_depth: usize,
    render_context: &BlockRenderContext<'_>,
    drag_state: &mut BlockDragState,
    mutations: &mut Vec<BlockRenderMutation>,
) {
    ui.spacing_mut().item_spacing.y = BLOCK_VERTICAL_GAP_PX;
    let min_x_px = ui.cursor().min.x;
    let clamped_width_px = ui
        .available_width()
        .clamp(NESTED_BLOCK_MIN_WIDTH_PX, BLOCK_MAX_WIDTH_PX);
    let max_x_px = min_x_px + clamped_width_px;

    if statements.is_empty() {
        render_empty_drop_pocket(
            ui,
            slot_for_index(0),
            nesting_depth,
            render_context.pointer_pos,
            drag_state,
        );
        return;
    }

    if let Some(pointer) = render_context.pointer_pos {
        drag_state.offer_slot_candidate(
            pointer,
            slot_for_index(0),
            ui.cursor().min.y,
            min_x_px,
            max_x_px,
            nesting_depth,
        );
    }

    let statement_count = statements.len();
    for (block_index, block) in statements.iter_mut().enumerate() {
        let navigation_slots = BlockNavigationSlots {
            move_up_slot: (block_index > 0).then(|| slot_for_index(block_index.saturating_sub(1))),
            move_down_slot: (block_index.saturating_add(1) < statement_count)
                .then(|| slot_for_index(block_index.saturating_add(2))),
            duplicate_slot: slot_for_index(block_index.saturating_add(1)),
        };

        let is_dragged = drag_state.dragged_existing_block_id() == Some(block.id);
        ui.scope(|ui| {
            if is_dragged {
                ui.multiply_opacity(DRAG_DIM_OPACITY);
            }
            render_statement_node(
                ui,
                block,
                navigation_slots,
                nesting_depth,
                render_context,
                drag_state,
                mutations,
            );
        });

        if let Some(pointer) = render_context.pointer_pos {
            drag_state.offer_slot_candidate(
                pointer,
                slot_for_index(block_index.saturating_add(1)),
                ui.cursor().min.y,
                min_x_px,
                max_x_px,
                nesting_depth,
            );
        }
    }
}

fn render_empty_drop_pocket(
    ui: &mut Ui,
    target_slot: BlockSlotTarget,
    nesting_depth: usize,
    pointer_pos: Option<Pos2>,
    drag_state: &mut BlockDragState,
) {
    let is_root = nesting_depth == 0;
    let pocket_size = vec2(
        if is_root { 360.0 } else { 236.0 },
        if is_root {
            EMPTY_ROOT_HEIGHT_PX
        } else {
            EMPTY_POCKET_HEIGHT_PX
        },
    );
    ui.add_space(if is_root { 10.0 } else { 8.0 });
    let (pocket_rect, _) = ui.allocate_exact_size(pocket_size, Sense::hover());
    ui.add_space(6.0);

    if let Some(pointer) = pointer_pos {
        drag_state.offer_slot_candidate(
            pointer,
            target_slot,
            pocket_rect.center().y,
            pocket_rect.min.x,
            pocket_rect.max.x,
            nesting_depth,
        );
    }

    let is_target = drag_state
        .hovered_slot
        .is_some_and(|slot| slot.target_slot == target_slot);
    let (fill_color, border_color, text_color) = if is_target {
        (ACCENT_PRIMARY_TINT, ACCENT_PRIMARY, ACCENT_PRIMARY)
    } else if drag_state.is_dragging_statement() {
        (
            Color32::from_rgba_unmultiplied(0x28, 0x22, 0x16, 175),
            with_alpha(ACCENT_PRIMARY, 165),
            TEXT_PRIMARY,
        )
    } else {
        (
            Color32::from_rgba_unmultiplied(0x18, 0x1C, 0x24, 150),
            BORDER_SUBTLE,
            TEXT_MUTED,
        )
    };

    ui.painter().rect_filled(
        pocket_rect,
        CornerRadius::same(CORNER_RADIUS_SM_PX),
        fill_color,
    );
    paint_dashed_rect_border(
        ui.painter(),
        pocket_rect,
        Stroke::new(if is_target { 1.5 } else { 1.0 }, border_color),
    );
    let prompt = if is_root {
        "Drag blocks from the palette or click a block to add"
    } else {
        "Drop blocks here"
    };
    ui.painter().text(
        pocket_rect.center(),
        Align2::CENTER_CENTER,
        prompt,
        FontId::new(FONT_SIZE_SMALL_PX, font_family_semibold()),
        text_color,
    );
}

fn render_statement_node(
    ui: &mut Ui,
    block: &mut StatementBlock,
    navigation_slots: BlockNavigationSlots,
    nesting_depth: usize,
    render_context: &BlockRenderContext<'_>,
    drag_state: &mut BlockDragState,
    mutations: &mut Vec<BlockRenderMutation>,
) {
    let block_id = block.id;
    let is_container = is_container_kind(&block.kind);
    let visual_colors = block_visual_colors(block.kind.category());
    let is_active = render_context.active_block_id == Some(block_id);
    let validation_message =
        find_block_validation_message(render_context.validation_report, block_id);
    let min_width_px = if nesting_depth == 0 {
        BLOCK_MIN_WIDTH_PX
    } else {
        NESTED_BLOCK_MIN_WIDTH_PX
    };
    let background_shape_index = ui.painter().add(Shape::Noop);

    let header_response = ui
        .scope_builder(
            UiBuilder::new()
                .id_salt(("block_body_drag", block_id.0))
                .sense(Sense::click_and_drag()),
            |ui| {
                Frame::NONE
                    .inner_margin(Margin {
                        left: 10,
                        right: HOVER_RIGHT_RESERVE_PX,
                        top: (FULL_BLOCK_GEOMETRY.notch_depth_px as i8) + 5,
                        bottom: 7,
                    })
                    .show(ui, |ui| {
                        ui.set_min_width(min_width_px);
                        ui.set_max_width(BLOCK_MAX_WIDTH_PX);
                        ui.horizontal_wrapped(|ui| {
                            paint_subtle_grip_dots(ui, visual_colors.text_muted);
                            let did_edit = if is_container {
                                render_container_block_sentence(
                                    ui,
                                    block_id,
                                    &mut block.kind,
                                    visual_colors,
                                )
                            } else {
                                render_leaf_block_sentence(
                                    ui,
                                    block_id,
                                    &mut block.kind,
                                    visual_colors,
                                )
                            };
                            if did_edit {
                                mutations.push(BlockRenderMutation::MarkDirty);
                            }
                        });
                    });
            },
        )
        .response;

    let header_rect = header_response.rect;
    let is_hovered = !drag_state.is_dragging() && header_response.contains_pointer();
    let (fill_color, border_color, stroke_width_px) = resolve_block_surface_colors(
        visual_colors,
        is_active,
        validation_message.is_some(),
        is_hovered,
    );

    let shapes = if is_container {
        let footer_width_px = header_rect
            .width()
            .clamp(C_BLOCK_MIN_FOOTER_WIDTH_PX, C_BLOCK_MAX_FOOTER_WIDTH_PX);
        let else_rect = render_container_branches(
            ui,
            block_id,
            &mut block.kind,
            footer_width_px,
            visual_colors,
            nesting_depth.saturating_add(1),
            render_context,
            drag_state,
            mutations,
        );
        let (footer_rect, _) = ui.allocate_exact_size(
            vec2(footer_width_px, C_BLOCK_FOOTER_HEIGHT_PX),
            Sense::hover(),
        );
        build_c_block_shapes(
            header_rect,
            else_rect,
            footer_rect,
            fill_color,
            border_color,
            visual_colors.top_highlight,
            stroke_width_px,
        )
    } else {
        build_leaf_block_shapes(
            header_rect,
            FULL_BLOCK_GEOMETRY,
            fill_color,
            border_color,
            visual_colors.top_highlight,
            stroke_width_px,
            true,
            true,
        )
    };
    ui.painter().set(background_shape_index, Shape::Vec(shapes));

    handle_block_drag_and_menu(
        ui,
        &header_response,
        header_rect,
        block,
        &navigation_slots,
        validation_message,
        drag_state,
        mutations,
    );
}

fn is_container_kind(statement_kind: &StatementKind) -> bool {
    matches!(
        statement_kind,
        StatementKind::IfThen { .. }
            | StatementKind::IfThenElse { .. }
            | StatementKind::WhileCondition { .. }
            | StatementKind::RepeatCount { .. }
    )
}

#[allow(clippy::too_many_arguments)]
fn render_container_branches(
    ui: &mut Ui,
    parent_block_id: BlockId,
    statement_kind: &mut StatementKind,
    bridge_width_px: f32,
    visual_colors: BlockVisualColors,
    child_depth: usize,
    render_context: &BlockRenderContext<'_>,
    drag_state: &mut BlockDragState,
    mutations: &mut Vec<BlockRenderMutation>,
) -> Option<Rect> {
    match statement_kind {
        StatementKind::IfThen { then_blocks, .. } => {
            render_indented_branch(
                ui,
                then_blocks,
                |insert_index| BlockSlotTarget::ThenBranch {
                    parent_block_id,
                    insert_index,
                },
                child_depth,
                render_context,
                drag_state,
                mutations,
            );
            None
        }
        StatementKind::IfThenElse {
            then_blocks,
            else_blocks,
            ..
        } => {
            render_indented_branch(
                ui,
                then_blocks,
                |insert_index| BlockSlotTarget::ThenBranch {
                    parent_block_id,
                    insert_index,
                },
                child_depth,
                render_context,
                drag_state,
                mutations,
            );
            let else_rect = render_else_bridge_label(ui, bridge_width_px, visual_colors);
            render_indented_branch(
                ui,
                else_blocks,
                |insert_index| BlockSlotTarget::ElseBranch {
                    parent_block_id,
                    insert_index,
                },
                child_depth,
                render_context,
                drag_state,
                mutations,
            );
            Some(else_rect)
        }
        StatementKind::WhileCondition { body_blocks, .. }
        | StatementKind::RepeatCount { body_blocks, .. } => {
            render_indented_branch(
                ui,
                body_blocks,
                |insert_index| BlockSlotTarget::LoopBody {
                    parent_block_id,
                    insert_index,
                },
                child_depth,
                render_context,
                drag_state,
                mutations,
            );
            None
        }
        _ => None,
    }
}

fn render_indented_branch(
    ui: &mut Ui,
    child_blocks: &mut [StatementBlock],
    slot_for_index: impl Fn(usize) -> BlockSlotTarget + Copy,
    child_depth: usize,
    render_context: &BlockRenderContext<'_>,
    drag_state: &mut BlockDragState,
    mutations: &mut Vec<BlockRenderMutation>,
) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(C_BLOCK_SPINE_WIDTH_PX);
        ui.vertical(|ui| {
            render_statement_stack(
                ui,
                child_blocks,
                slot_for_index,
                child_depth,
                render_context,
                drag_state,
                mutations,
            );
        });
    });
}

fn render_else_bridge_label(
    ui: &mut Ui,
    bridge_width_px: f32,
    visual_colors: BlockVisualColors,
) -> Rect {
    let (bridge_rect, _) = ui.allocate_exact_size(
        vec2(bridge_width_px, C_BLOCK_ELSE_MIN_HEIGHT_PX),
        Sense::hover(),
    );
    let notch_px = FULL_BLOCK_GEOMETRY.notch_depth_px;
    let label_y_px = bridge_rect.min.y + notch_px + (C_BLOCK_ELSE_MIN_HEIGHT_PX - notch_px) * 0.5;
    ui.painter().text(
        pos2(
            bridge_rect.min.x + C_BLOCK_SPINE_WIDTH_PX + 10.0,
            label_y_px,
        ),
        Align2::LEFT_CENTER,
        "Else",
        FontId::new(FONT_SIZE_SMALL_PX, font_family_semibold()),
        visual_colors.text_primary,
    );
    bridge_rect
}

#[allow(clippy::too_many_arguments)]
fn handle_block_drag_and_menu(
    ui: &mut Ui,
    header_response: &Response,
    header_rect: Rect,
    block: &StatementBlock,
    navigation_slots: &BlockNavigationSlots,
    validation_message: Option<&str>,
    drag_state: &mut BlockDragState,
    mutations: &mut Vec<BlockRenderMutation>,
) {
    let is_popup_open = Popup::is_any_open(ui.ctx());
    let is_hovered = !drag_state.is_dragging() && header_response.contains_pointer();
    if !is_popup_open && header_response.hovered() && !drag_state.is_dragging() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    if !is_popup_open && header_response.drag_started_by(PointerButton::Primary) {
        drag_state.start_drag(
            BlockDragPayload::MoveExistingBlock(block.id),
            block.kind.title(),
            summarize_statement_for_ghost(&block.kind),
            block.kind.category(),
        );
    }

    if is_hovered {
        render_hover_trailing_actions(
            ui,
            header_rect,
            block.id,
            navigation_slots.duplicate_slot,
            mutations,
        );
    } else if let Some(error_text) = validation_message {
        paint_validation_warning_dot(ui, header_rect, error_text);
    }

    header_response.container_context_menu(|ui| {
        render_block_context_menu(ui, block.id, navigation_slots, mutations);
    });
}

fn render_hover_trailing_actions(
    ui: &mut Ui,
    header_rect: Rect,
    block_id: BlockId,
    duplicate_slot: BlockSlotTarget,
    mutations: &mut Vec<BlockRenderMutation>,
) {
    let notch_px = FULL_BLOCK_GEOMETRY.notch_depth_px;
    let center_y_px = header_rect.min.y + notch_px + (header_rect.height() - notch_px) * 0.5;
    let button_size = vec2(HOVER_ACTION_SIZE_PX, HOVER_ACTION_SIZE_PX);
    let delete_rect =
        Rect::from_center_size(pos2(header_rect.max.x - 16.0, center_y_px), button_size);
    let duplicate_center_x = delete_rect.min.x - HOVER_ACTION_GAP_PX - (HOVER_ACTION_SIZE_PX * 0.5);
    let duplicate_rect = Rect::from_center_size(pos2(duplicate_center_x, center_y_px), button_size);

    let idle_fill = Color32::from_rgba_unmultiplied(0x00, 0x00, 0x00, 76);
    let duplicate_response = ui
        .interact(
            duplicate_rect,
            Id::unique(("blk_dup", block_id.0)),
            Sense::click_and_drag(),
        )
        .on_hover_text("Duplicate block");
    let duplicate_fill = if duplicate_response.hovered() {
        Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 44)
    } else {
        idle_fill
    };
    ui.painter().rect_filled(
        duplicate_rect,
        CornerRadius::same(CORNER_RADIUS_SM_PX),
        duplicate_fill,
    );
    paint_duplicate_icon(ui.painter(), duplicate_rect.center(), TEXT_PRIMARY);
    if duplicate_response.clicked() {
        mutations.push(BlockRenderMutation::DuplicateBlock {
            block_id,
            insert_slot: duplicate_slot,
        });
    }

    let delete_response = ui
        .interact(
            delete_rect,
            Id::unique(("blk_del", block_id.0)),
            Sense::click_and_drag(),
        )
        .on_hover_text("Delete block");
    let (delete_fill, delete_ink) = if delete_response.hovered() {
        (STATUS_DANGER_SOLID, TEXT_PRIMARY)
    } else {
        (idle_fill, STATUS_DANGER_INK)
    };
    ui.painter().rect_filled(
        delete_rect,
        CornerRadius::same(CORNER_RADIUS_SM_PX),
        delete_fill,
    );
    paint_close_icon(ui.painter(), delete_rect.center(), delete_ink);
    if delete_response.clicked() {
        mutations.push(BlockRenderMutation::DeleteBlock(block_id));
    }
}

fn render_block_context_menu(
    ui: &mut Ui,
    block_id: BlockId,
    navigation_slots: &BlockNavigationSlots,
    mutations: &mut Vec<BlockRenderMutation>,
) {
    ui.set_min_width(148.0);
    if add_menu_item(ui, "Duplicate Block", TEXT_PRIMARY, true) {
        mutations.push(BlockRenderMutation::DuplicateBlock {
            block_id,
            insert_slot: navigation_slots.duplicate_slot,
        });
        ui.close();
    }
    if add_menu_item(
        ui,
        "Move Up",
        TEXT_PRIMARY,
        navigation_slots.move_up_slot.is_some(),
    ) && let Some(target_slot) = navigation_slots.move_up_slot
    {
        mutations.push(BlockRenderMutation::MoveBlock {
            block_id,
            target_slot,
        });
        ui.close();
    }
    if add_menu_item(
        ui,
        "Move Down",
        TEXT_PRIMARY,
        navigation_slots.move_down_slot.is_some(),
    ) && let Some(target_slot) = navigation_slots.move_down_slot
    {
        mutations.push(BlockRenderMutation::MoveBlock {
            block_id,
            target_slot,
        });
        ui.close();
    }
    ui.separator();
    if add_menu_item(ui, "Delete Block", STATUS_DANGER_INK, true) {
        mutations.push(BlockRenderMutation::DeleteBlock(block_id));
        ui.close();
    }
}

fn add_menu_item(ui: &mut Ui, label: &str, color: Color32, is_enabled: bool) -> bool {
    ui.add_enabled(
        is_enabled,
        Button::new(rich_semibold(label, FONT_SIZE_SMALL_PX, color)).frame(false),
    )
    .clicked()
}

fn paint_subtle_grip_dots(ui: &mut Ui, dot_base_color: Color32) {
    let (grip_rect, _) = ui.allocate_exact_size(vec2(10.0, 18.0), Sense::hover());
    let dot_color = with_alpha(dot_base_color, 130);
    let center = grip_rect.center();
    for offset_y_px in [-4.0_f32, 0.0, 4.0] {
        ui.painter()
            .circle_filled(pos2(center.x - 2.0, center.y + offset_y_px), 1.1, dot_color);
        ui.painter()
            .circle_filled(pos2(center.x + 2.0, center.y + offset_y_px), 1.1, dot_color);
    }
}

fn paint_validation_warning_dot(ui: &mut Ui, header_rect: Rect, error_text: &str) {
    let notch_px = FULL_BLOCK_GEOMETRY.notch_depth_px;
    let center_y_px = header_rect.min.y + notch_px + (header_rect.height() - notch_px) * 0.5;
    let badge_rect = Rect::from_center_size(
        pos2(header_rect.max.x - 16.0, center_y_px),
        vec2(18.0, 18.0),
    );
    let response = ui.interact(badge_rect, ui.next_auto_id(), Sense::hover());
    ui.painter()
        .circle_filled(badge_rect.center(), 8.0, STATUS_DANGER_SOLID);
    ui.painter().text(
        badge_rect.center(),
        Align2::CENTER_CENTER,
        "!",
        FontId::new(11.5, font_family_semibold()),
        TEXT_PRIMARY,
    );
    response.on_hover_text(error_text);
}

fn resolve_block_surface_colors(
    visual_colors: BlockVisualColors,
    is_active: bool,
    has_validation_error: bool,
    is_hovered: bool,
) -> (Color32, Color32, f32) {
    if is_active {
        return (STATUS_RUNNING_TINT, STATUS_RUNNING, 2.0);
    }
    if has_validation_error {
        return (STATUS_DANGER_TINT, STATUS_DANGER, 1.75);
    }
    let fill = if is_hovered {
        visual_colors.fill_hover
    } else {
        visual_colors.fill
    };
    (
        fill,
        visual_colors.border,
        FULL_BLOCK_GEOMETRY.stroke_width_px,
    )
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

/// Formats a concise one-line parameter summary for floating drag ghost cards.
fn summarize_statement_for_ghost(statement_kind: &StatementKind) -> String {
    match statement_kind {
        StatementKind::IfThen { condition, .. }
        | StatementKind::IfThenElse { condition, .. }
        | StatementKind::WhileCondition { condition, .. } => condition.display_label(),
        StatementKind::RepeatCount { repeat_count, .. } => format!("{repeat_count} times"),
        StatementKind::BreakLoop => "Exit current loop".to_string(),
        StatementKind::StopScript => "End script pass".to_string(),
        StatementKind::DoNothing => "No action".to_string(),
        StatementKind::MouseClick {
            mouse_button,
            hold_duration_ms,
        } => format!("{} · {hold_duration_ms} ms", mouse_button.short_label()),
        StatementKind::MouseDown { mouse_button } | StatementKind::MouseUp { mouse_button } => {
            mouse_button.display_label().to_string()
        }
        StatementKind::MouseMoveRelative {
            delta_x_px,
            delta_y_px,
        } => format!("X {delta_x_px}, Y {delta_y_px} px"),
        StatementKind::MouseScroll { vertical_steps } => format!("{vertical_steps} steps"),
        StatementKind::KeyTap {
            key_code,
            hold_duration_ms,
        } => format!("{} · {hold_duration_ms} ms", key_code.display_label()),
        StatementKind::KeyDown { key_code } | StatementKind::KeyUp { key_code } => {
            key_code.display_label().to_string()
        }
        StatementKind::TypeText { text, .. } => format!("\"{text}\""),
        StatementKind::WaitFixed { delay_ms } => format!("{delay_ms} ms"),
        StatementKind::WaitRandomUniform {
            min_delay_ms,
            max_delay_ms,
        } => format!("{min_delay_ms}..{max_delay_ms} ms"),
        StatementKind::WaitRandomGaussian {
            base_delay_ms,
            min_offset_ms,
            max_offset_ms,
            ..
        } => format!("{base_delay_ms} + G[{min_offset_ms:.0}..{max_offset_ms:.0}] ms"),
        StatementKind::SetVariable {
            variable_name,
            value,
        } => format!("{variable_name} = {value}"),
        StatementKind::ChangeVariable {
            variable_name,
            delta_value,
        } => format!("{variable_name} += {delta_value}"),
    }
}
