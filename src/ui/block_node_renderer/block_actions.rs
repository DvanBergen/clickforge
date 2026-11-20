use egui::{Button, Color32, CornerRadius, Id, Rect, Sense, Ui, pos2, vec2};

use crate::models::{
    BlockId, BlockSlotTarget, CustomBlockDefinition, StatementBlock, StatementKind,
    lookup_custom_block,
};
use crate::ui::block_node_renderer::geometry::{
    FULL_BLOCK_GEOMETRY, paint_close_icon, paint_duplicate_icon,
};
use crate::ui::block_node_renderer::{BlockNavigationSlots, BlockRenderMutation};
use crate::ui::theme::{
    CORNER_RADIUS_SM_PX, FONT_SIZE_SMALL_PX, STATUS_DANGER_INK, STATUS_DANGER_SOLID, TEXT_PRIMARY,
    rich_semibold,
};

const HOVER_ACTION_SIZE_PX: f32 = 20.0;
const HOVER_ACTION_GAP_PX: f32 = 4.0;

/// Renders the trailing hover-revealed Duplicate and Delete buttons on a statement block.
pub(super) fn render_hover_trailing_actions(
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

/// Renders the right-click context menu for any canvas block, including Custom Tile actions.
pub(super) fn render_block_context_menu(
    ui: &mut Ui,
    block: &StatementBlock,
    navigation_slots: &BlockNavigationSlots,
    custom_blocks: &[CustomBlockDefinition],
    mutations: &mut Vec<BlockRenderMutation>,
) {
    ui.set_min_width(168.0);
    if add_menu_item(ui, "Duplicate Block", TEXT_PRIMARY, true) {
        mutations.push(BlockRenderMutation::DuplicateBlock {
            block_id: block.id,
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
            block_id: block.id,
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
            block_id: block.id,
            target_slot,
        });
        ui.close();
    }

    ui.separator();
    if let StatementKind::RunCustomBlock {
        custom_block_id, ..
    } = &block.kind
    {
        let tile_exists = lookup_custom_block(custom_blocks, custom_block_id).is_some();
        if add_menu_item(ui, "Edit Custom Tile", TEXT_PRIMARY, tile_exists) {
            mutations.push(BlockRenderMutation::EditCustomBlock(
                custom_block_id.clone(),
            ));
            ui.close();
        }
        if add_menu_item(ui, "Unpack Inline", TEXT_PRIMARY, tile_exists) {
            mutations.push(BlockRenderMutation::UnpackCustomBlock {
                block_id: block.id,
                custom_block_id: custom_block_id.clone(),
            });
            ui.close();
        }
    }
    if add_menu_item(ui, "Save as Custom Tile...", TEXT_PRIMARY, true) {
        mutations.push(BlockRenderMutation::SaveBlockAsCustomTile(block.id));
        ui.close();
    }

    ui.separator();
    if add_menu_item(ui, "Delete Block", STATUS_DANGER_INK, true) {
        mutations.push(BlockRenderMutation::DeleteBlock(block.id));
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
