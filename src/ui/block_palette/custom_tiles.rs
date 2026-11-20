use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Painter, PointerButton, Popup, Pos2,
    Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2,
};

use crate::models::{CustomBlockDefinition, MAX_CUSTOM_BLOCKS, StatementKind};
use crate::ui::block_node_renderer::{MINI_BLOCK_GEOMETRY, build_leaf_block_shapes};
use crate::ui::block_palette::{
    BlockDragPayload, BlockDragState, CATEGORY_DOT_RADIUS_PX, MINI_BLOCK_HEIGHT_PX, PaletteAction,
    matches_search_query,
};
use crate::ui::theme::{
    BORDER_STRONG, CATEGORY_CUSTOM, CORNER_RADIUS_SM_PX, FONT_SIZE_SMALL_PX, STATUS_DANGER_INK,
    STATUS_DANGER_SOLID, SURFACE_ELEVATED, TEXT_MUTED, TEXT_PRIMARY, build_styled_button,
    custom_block_visual_colors, font_family_mono_bold, font_family_semibold, rich_mono,
    rich_semibold, truncate_display_text,
};

const NEW_TILE_BUTTON_HEIGHT_PX: f32 = 28.0;
const TILE_ACTION_BUTTON_SIZE_PX: f32 = 18.0;
const TILE_ACTION_BUTTON_GAP_PX: f32 = 3.0;
const TILE_ACTION_RIGHT_INSET_PX: f32 = 13.0;
const MAX_TILE_NAME_IDLE_CHARS: usize = 19;
const MAX_TILE_NAME_HOVERED_CHARS: usize = 13;

/// Renders the `My Blocks` palette section containing `+ New Custom Tile` and draggable user tiles.
pub(super) fn render_custom_tiles_section(
    ui: &mut Ui,
    custom_blocks: &[CustomBlockDefinition],
    normalized_query: &str,
    is_dedicated_tab: bool,
    drag_state: &mut BlockDragState,
    emitted_actions: &mut Vec<PaletteAction>,
) -> bool {
    let matching_tiles: Vec<&CustomBlockDefinition> = custom_blocks
        .iter()
        .filter(|tile| {
            matches_search_query(normalized_query, "My Blocks", &tile.name, &tile.description)
        })
        .collect();

    if !is_dedicated_tab && !normalized_query.is_empty() && matching_tiles.is_empty() {
        return false;
    }

    ui.horizontal(|ui| {
        let (dot_rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
        ui.painter()
            .circle_filled(dot_rect.center(), CATEGORY_DOT_RADIUS_PX, CATEGORY_CUSTOM);
        ui.label(rich_semibold("My Blocks", FONT_SIZE_SMALL_PX, TEXT_MUTED));
        ui.label(rich_mono(
            custom_blocks.len().to_string(),
            FONT_SIZE_SMALL_PX,
            TEXT_MUTED,
        ));
    });
    ui.add_space(4.0);

    let can_create_tile = custom_blocks.len() < MAX_CUSTOM_BLOCKS;
    let new_tile_button = build_styled_button(
        rich_semibold(
            "+ New Custom Tile",
            FONT_SIZE_SMALL_PX,
            if can_create_tile {
                TEXT_PRIMARY
            } else {
                TEXT_MUTED
            },
        ),
        SURFACE_ELEVATED,
        BORDER_STRONG,
        CORNER_RADIUS_SM_PX,
        vec2(ui.available_width(), NEW_TILE_BUTTON_HEIGHT_PX),
    );
    let button_response = ui.add_enabled(can_create_tile, new_tile_button);
    if button_response.clicked() {
        emitted_actions.push(PaletteAction::OpenNewCustomBlockModal);
    }
    if !can_create_tile {
        button_response.on_disabled_hover_text(format!(
            "Custom tile limit reached ({MAX_CUSTOM_BLOCKS} maximum)."
        ));
    } else {
        button_response.on_hover_text("Create a reusable custom tile made of multiple block steps");
    }
    ui.add_space(6.0);

    if matching_tiles.is_empty() {
        let empty_hint = if custom_blocks.is_empty() {
            "No custom tiles yet. Build one above or right-click any block on the canvas."
        } else {
            "No custom tiles match the current filter."
        };
        ui.label(
            RichText::new(empty_hint)
                .size(FONT_SIZE_SMALL_PX)
                .color(TEXT_MUTED),
        );
        ui.add_space(8.0);
        return true;
    }

    for custom_tile in matching_tiles {
        render_palette_custom_tile_block(ui, custom_tile, drag_state, emitted_actions);
    }
    ui.add_space(8.0);
    true
}

fn render_palette_custom_tile_block(
    ui: &mut Ui,
    custom_tile: &CustomBlockDefinition,
    drag_state: &mut BlockDragState,
    emitted_actions: &mut Vec<PaletteAction>,
) {
    let visual_colors = custom_block_visual_colors(custom_tile.color);
    let width_px = ui.available_width().max(140.0);
    let (allocated_rect, block_response) = ui.allocate_exact_size(
        vec2(
            width_px,
            MINI_BLOCK_HEIGHT_PX + MINI_BLOCK_GEOMETRY.notch_depth_px,
        ),
        Sense::click_and_drag(),
    );

    let body_rect = Rect::from_min_max(
        allocated_rect.min,
        pos2(
            allocated_rect.max.x,
            allocated_rect.min.y + MINI_BLOCK_HEIGHT_PX,
        ),
    );
    let is_popup_open = Popup::is_any_open(ui.ctx());
    let is_tile_hovered =
        !is_popup_open && !drag_state.is_dragging() && ui.rect_contains_pointer(body_rect);

    let fill_color = if is_tile_hovered {
        visual_colors.fill_hover
    } else {
        visual_colors.fill
    };
    let shapes = build_leaf_block_shapes(
        body_rect,
        MINI_BLOCK_GEOMETRY,
        fill_color,
        visual_colors.border,
        visual_colors.top_highlight,
        MINI_BLOCK_GEOMETRY.stroke_width_px,
        true,
        true,
    );
    ui.painter().add(Shape::Vec(shapes));

    let text_center_y_px = body_rect.center().y + 1.0;
    let max_title_chars = if is_tile_hovered {
        MAX_TILE_NAME_HOVERED_CHARS
    } else {
        MAX_TILE_NAME_IDLE_CHARS
    };
    let display_title = format!(
        "★ {}",
        truncate_display_text(&custom_tile.name, max_title_chars)
    );
    ui.painter().text(
        pos2(body_rect.min.x + 11.0, text_center_y_px),
        Align2::LEFT_CENTER,
        display_title,
        FontId::new(12.5, font_family_semibold()),
        visual_colors.text_primary,
    );

    let step_count = custom_tile.total_block_count();
    let mut is_pointer_over_icon_button = false;

    if is_tile_hovered {
        is_pointer_over_icon_button = render_custom_tile_hover_icons(
            ui,
            body_rect,
            text_center_y_px,
            custom_tile,
            emitted_actions,
        );
    } else {
        ui.painter().text(
            pos2(body_rect.max.x - 10.0, text_center_y_px),
            Align2::RIGHT_CENTER,
            step_count.to_string(),
            FontId::new(11.0, font_family_mono_bold()),
            visual_colors.text_muted,
        );
    }

    if is_tile_hovered && !is_pointer_over_icon_button {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }

    let step_word = if step_count == 1 { "step" } else { "steps" };
    let tooltip_text = if custom_tile.description.trim().is_empty() {
        format!("{} ({step_count} {step_word})", custom_tile.name)
    } else {
        format!(
            "{} ({step_count} {step_word})\n{}",
            custom_tile.name,
            custom_tile.description.trim()
        )
    };
    let block_response = block_response.on_hover_text(tooltip_text);

    if !is_popup_open && !is_pointer_over_icon_button {
        let statement_kind = StatementKind::RunCustomBlock {
            custom_block_id: custom_tile.id.clone(),
            cached_name: custom_tile.name.clone(),
        };
        if block_response.drag_started_by(PointerButton::Primary) {
            drag_state.start_custom_tile_drag(
                BlockDragPayload::NewStatement(statement_kind),
                format!("★ {}", custom_tile.name),
                format!("{step_count} {step_word}"),
                custom_tile.color,
            );
        } else if block_response.clicked() {
            emitted_actions.push(PaletteAction::AppendStatement(statement_kind));
        }
    }

    ui.add_space(4.0);
}

fn render_custom_tile_hover_icons(
    ui: &mut Ui,
    body_rect: Rect,
    center_y_px: f32,
    custom_tile: &CustomBlockDefinition,
    emitted_actions: &mut Vec<PaletteAction>,
) -> bool {
    let icon_size = vec2(TILE_ACTION_BUTTON_SIZE_PX, TILE_ACTION_BUTTON_SIZE_PX);
    let delete_rect = Rect::from_center_size(
        pos2(body_rect.max.x - TILE_ACTION_RIGHT_INSET_PX, center_y_px),
        icon_size,
    );
    let edit_center_x_px =
        delete_rect.min.x - TILE_ACTION_BUTTON_GAP_PX - (TILE_ACTION_BUTTON_SIZE_PX * 0.5);
    let edit_rect = Rect::from_center_size(pos2(edit_center_x_px, center_y_px), icon_size);
    let idle_fill = Color32::from_rgba_unmultiplied(0x00, 0x00, 0x00, 86);

    let edit_response = ui
        .interact(
            edit_rect,
            Id::unique(("palette_custom_edit", custom_tile.id.as_str())),
            Sense::click_and_drag(),
        )
        .on_hover_text("Edit custom tile");
    let edit_fill = if edit_response.hovered() {
        Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 48)
    } else {
        idle_fill
    };
    ui.painter().rect_filled(
        edit_rect,
        CornerRadius::same(CORNER_RADIUS_SM_PX),
        edit_fill,
    );
    paint_pencil_icon(ui.painter(), edit_rect.center(), TEXT_PRIMARY);
    if edit_response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if edit_response.clicked() {
        emitted_actions.push(PaletteAction::EditCustomBlock(custom_tile.id.clone()));
    }

    let delete_response = ui
        .interact(
            delete_rect,
            Id::unique(("palette_custom_delete", custom_tile.id.as_str())),
            Sense::click_and_drag(),
        )
        .on_hover_text("Delete custom tile");
    let (delete_fill, delete_ink) = if delete_response.hovered() {
        (STATUS_DANGER_SOLID, Color32::WHITE)
    } else {
        (idle_fill, STATUS_DANGER_INK)
    };
    ui.painter().rect_filled(
        delete_rect,
        CornerRadius::same(CORNER_RADIUS_SM_PX),
        delete_fill,
    );
    paint_compact_cross_icon(ui.painter(), delete_rect.center(), delete_ink);
    if delete_response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if delete_response.clicked() {
        emitted_actions.push(PaletteAction::DeleteCustomBlock(custom_tile.id.clone()));
    }

    let is_pointer_inside =
        ui.rect_contains_pointer(edit_rect) || ui.rect_contains_pointer(delete_rect);
    is_pointer_inside
        || edit_response.hovered()
        || delete_response.hovered()
        || edit_response.clicked()
        || delete_response.clicked()
}

fn paint_pencil_icon(painter: &Painter, center: Pos2, icon_color: Color32) {
    let stroke = Stroke::new(1.4, icon_color);
    painter.line_segment(
        [
            pos2(center.x - 3.5, center.y + 3.5),
            pos2(center.x + 3.5, center.y - 3.5),
        ],
        stroke,
    );
    painter.line_segment(
        [
            pos2(center.x + 1.5, center.y - 4.5),
            pos2(center.x + 4.5, center.y - 1.5),
        ],
        stroke,
    );
    painter.line_segment(
        [
            pos2(center.x - 4.2, center.y + 4.2),
            pos2(center.x - 2.0, center.y + 3.6),
        ],
        stroke,
    );
}

fn paint_compact_cross_icon(painter: &Painter, center: Pos2, icon_color: Color32) {
    let stroke = Stroke::new(1.5, icon_color);
    let half_px = 3.2;
    painter.line_segment(
        [
            pos2(center.x - half_px, center.y - half_px),
            pos2(center.x + half_px, center.y + half_px),
        ],
        stroke,
    );
    painter.line_segment(
        [
            pos2(center.x + half_px, center.y - half_px),
            pos2(center.x - half_px, center.y + half_px),
        ],
        stroke,
    );
}
