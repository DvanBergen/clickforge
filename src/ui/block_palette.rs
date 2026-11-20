mod custom_tiles;
pub(crate) mod templates;
mod trash_zone;

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, PointerButton, Popup, Pos2, Rect,
    ScrollArea, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui, epaint::PathShape, pos2, vec2,
};

use crate::models::{
    BlockCategory, BlockId, BlockSlotTarget, CustomBlockColor, CustomBlockDefinition,
    StatementKind, TriggerBlock,
};
use crate::ui::block_node_renderer::{
    MINI_BLOCK_GEOMETRY, build_leaf_block_shapes, build_trigger_hat_shapes,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BORDER_STRONG, BORDER_SUBTLE, CORNER_RADIUS_PILL_PX,
    CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX, FONT_SIZE_SMALL_PX, SURFACE_ELEVATED, SURFACE_HOVER,
    TEXT_MUTED, TEXT_PRIMARY, block_visual_colors, category_color, custom_block_swatch_color,
    font_family_mono_bold, font_family_semibold, rich_semibold,
};

use custom_tiles::render_custom_tiles_section;
use templates::{PALETTE_TEMPLATES, PaletteTemplateSpec, build_template_action_and_payload};
use trash_zone::{
    paint_palette_trash_hint_banner, paint_palette_trash_overlay, render_empty_filter_state,
};

/// Default palette dock width in logical pixels.
pub const PALETTE_DEFAULT_WIDTH_PX: f32 = 236.0;

/// Minimum palette dock width in logical pixels.
pub const PALETTE_MIN_WIDTH_PX: f32 = 204.0;

/// Maximum palette dock width in logical pixels.
pub const PALETTE_MAX_WIDTH_PX: f32 = 310.0;

const PALETTE_DROP_SNAP_TOLERANCE_Y_PX: f32 = 32.0;
const PALETTE_DROP_MARGIN_X_PX: f32 = 28.0;
const NESTED_SLOT_PREFERENCE_WINDOW_PX: f32 = 16.0;
const STRICT_X_PREFERENCE_WINDOW_PX: f32 = 18.0;
pub(super) const MINI_BLOCK_HEIGHT_PX: f32 = 30.0;
const MINI_HAT_HEIGHT_PX: f32 = 34.0;
pub(super) const CATEGORY_DOT_RADIUS_PX: f32 = 3.5;

/// Filter selection for narrowing the visible block templates inside the palette.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PaletteCategoryFilter {
    #[default]
    All,
    Triggers,
    Control,
    Actions,
    Timing,
    Variables,
    Custom,
}

impl PaletteCategoryFilter {
    fn display_label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Triggers => "Triggers",
            Self::Control => "Control",
            Self::Actions => "Actions",
            Self::Timing => "Timing",
            Self::Variables => "Vars",
            Self::Custom => "My Blocks",
        }
    }

    fn category(self) -> Option<BlockCategory> {
        match self {
            Self::All => None,
            Self::Triggers => Some(BlockCategory::Trigger),
            Self::Control => Some(BlockCategory::Control),
            Self::Actions => Some(BlockCategory::Action),
            Self::Timing => Some(BlockCategory::Timing),
            Self::Variables => Some(BlockCategory::Variable),
            Self::Custom => Some(BlockCategory::Custom),
        }
    }

    fn dot_color(self) -> Color32 {
        self.category().map_or(TEXT_MUTED, category_color)
    }

    fn matches_category(self, target_category: BlockCategory) -> bool {
        self.category()
            .is_none_or(|filter_category| filter_category == target_category)
    }
}

/// Payload carried during a drag-and-drop gesture across the palette and block canvas.
#[derive(Debug, Clone)]
pub enum BlockDragPayload {
    ReplaceTrigger(TriggerBlock),
    NewStatement(StatementKind),
    MoveExistingBlock(BlockId),
}

/// Active pointer drag state with visual preview metadata.
#[derive(Debug, Clone)]
pub struct ActiveBlockDrag {
    pub payload: BlockDragPayload,
    pub preview_label: String,
    pub preview_summary: String,
    pub category: BlockCategory,
    pub accent_color: Color32,
    pub custom_color: Option<CustomBlockColor>,
}

/// Closest valid drop slot computed during a canvas layout frame.
#[derive(Debug, Clone, Copy)]
pub struct HoveredDropSlot {
    pub target_slot: BlockSlotTarget,
    pub indicator_line_y_px: f32,
    pub indicator_min_x_px: f32,
    pub indicator_max_x_px: f32,
    pub distance_y_px: f32,
    pub nesting_depth: usize,
    pub is_pointer_strictly_inside_x: bool,
}

/// Shared drag-and-drop and palette filter state synchronized between palette and canvas.
#[derive(Debug, Default)]
pub struct BlockDragState {
    pub active_drag: Option<ActiveBlockDrag>,
    pub hovered_slot: Option<HoveredDropSlot>,
    pub is_hovering_trigger_hat: bool,
    pub is_hovering_trash_zone: bool,
    pub palette_rect: Option<Rect>,
    pub palette_search_query: String,
    pub selected_category_filter: PaletteCategoryFilter,
}

impl BlockDragState {
    /// Clears per-frame hover candidates before painting the block canvas.
    pub fn reset_frame_targets(&mut self) {
        self.hovered_slot = None;
        self.is_hovering_trigger_hat = false;
    }

    /// Returns true while any block drag gesture is active.
    pub fn is_dragging(&self) -> bool {
        self.active_drag.is_some()
    }

    /// Starts dragging a new or existing block with category colors and preview summary.
    pub fn start_drag(
        &mut self,
        payload: BlockDragPayload,
        preview_label: impl Into<String>,
        preview_summary: impl Into<String>,
        category: BlockCategory,
    ) {
        self.active_drag = Some(ActiveBlockDrag {
            payload,
            preview_label: preview_label.into(),
            preview_summary: preview_summary.into(),
            category,
            accent_color: category_color(category),
            custom_color: None,
        });
    }

    /// Starts dragging a reusable custom tile block tinted with its chosen `CustomBlockColor`.
    pub fn start_custom_tile_drag(
        &mut self,
        payload: BlockDragPayload,
        preview_label: impl Into<String>,
        preview_summary: impl Into<String>,
        custom_color: CustomBlockColor,
    ) {
        self.active_drag = Some(ActiveBlockDrag {
            payload,
            preview_label: preview_label.into(),
            preview_summary: preview_summary.into(),
            category: BlockCategory::Custom,
            accent_color: custom_block_swatch_color(custom_color),
            custom_color: Some(custom_color),
        });
    }

    /// Returns true while dragging a statement block from the palette or canvas.
    pub fn is_dragging_statement(&self) -> bool {
        matches!(
            self.active_drag.as_ref().map(|drag| &drag.payload),
            Some(BlockDragPayload::NewStatement(_) | BlockDragPayload::MoveExistingBlock(_))
        )
    }

    /// Returns true while dragging an existing canvas block that can be deleted via the trash zone.
    pub fn is_dragging_existing_block(&self) -> bool {
        self.dragged_existing_block_id().is_some()
    }

    /// Returns true while dragging a replacement trigger hat block.
    pub fn is_dragging_trigger(&self) -> bool {
        matches!(
            self.active_drag.as_ref().map(|drag| &drag.payload),
            Some(BlockDragPayload::ReplaceTrigger(_))
        )
    }

    /// Returns the ID of the existing canvas block currently being dragged, if any.
    pub fn dragged_existing_block_id(&self) -> Option<BlockId> {
        match self.active_drag.as_ref().map(|drag| &drag.payload) {
            Some(BlockDragPayload::MoveExistingBlock(block_id)) => Some(*block_id),
            _ => None,
        }
    }

    /// Evaluates a candidate insertion slot against pointer position, favoring nested branch slots.
    pub fn offer_slot_candidate(
        &mut self,
        pointer_pos: Pos2,
        target_slot: BlockSlotTarget,
        line_y_px: f32,
        min_x_px: f32,
        max_x_px: f32,
        nesting_depth: usize,
    ) {
        if !self.is_dragging_statement() || self.is_hovering_trash_zone {
            return;
        }
        let is_within_x = pointer_pos.x >= min_x_px - PALETTE_DROP_MARGIN_X_PX
            && pointer_pos.x <= max_x_px + PALETTE_DROP_MARGIN_X_PX;
        let distance_y_px = (pointer_pos.y - line_y_px).abs();
        if !is_within_x || distance_y_px > PALETTE_DROP_SNAP_TOLERANCE_Y_PX {
            return;
        }

        let candidate = HoveredDropSlot {
            target_slot,
            indicator_line_y_px: line_y_px,
            indicator_min_x_px: min_x_px,
            indicator_max_x_px: max_x_px,
            distance_y_px,
            nesting_depth,
            is_pointer_strictly_inside_x: pointer_pos.x >= min_x_px && pointer_pos.x <= max_x_px,
        };
        if self
            .hovered_slot
            .is_none_or(|existing| should_prefer_slot_candidate(existing, candidate))
        {
            self.hovered_slot = Some(candidate);
        }
    }
}

fn should_prefer_slot_candidate(existing: HoveredDropSlot, candidate: HoveredDropSlot) -> bool {
    if candidate.is_pointer_strictly_inside_x && !existing.is_pointer_strictly_inside_x {
        return candidate.distance_y_px <= existing.distance_y_px + STRICT_X_PREFERENCE_WINDOW_PX;
    }
    if !candidate.is_pointer_strictly_inside_x && existing.is_pointer_strictly_inside_x {
        return candidate.distance_y_px + STRICT_X_PREFERENCE_WINDOW_PX < existing.distance_y_px;
    }
    if candidate.is_pointer_strictly_inside_x && existing.is_pointer_strictly_inside_x {
        if candidate.nesting_depth > existing.nesting_depth {
            return candidate.distance_y_px
                <= existing.distance_y_px + NESTED_SLOT_PREFERENCE_WINDOW_PX;
        }
        if candidate.nesting_depth < existing.nesting_depth {
            return candidate.distance_y_px + NESTED_SLOT_PREFERENCE_WINDOW_PX
                < existing.distance_y_px;
        }
    }
    candidate.distance_y_px < existing.distance_y_px
}

/// Immediate click-to-insert and custom-tile management actions emitted by the block palette.
#[derive(Debug, Clone)]
pub enum PaletteAction {
    ApplyTrigger(TriggerBlock),
    AppendStatement(StatementKind),
    OpenNewCustomBlockModal,
    EditCustomBlock(String),
    DeleteCustomBlock(String),
}

/// Renders the filtered Scratch-style mini-block palette with click-to-add, drag-to-place, and trash drop.
pub fn render_block_palette(
    ui: &mut Ui,
    custom_blocks: &[CustomBlockDefinition],
    drag_state: &mut BlockDragState,
    emitted_actions: &mut Vec<PaletteAction>,
) {
    let palette_rect = ui.available_rect_before_wrap();
    drag_state.palette_rect = Some(palette_rect);

    let pointer_pos = ui.input(|input| input.pointer.hover_pos());
    let is_dragging_existing = drag_state.is_dragging_existing_block();
    drag_state.is_hovering_trash_zone = is_dragging_existing
        && pointer_pos.is_some_and(|pointer| palette_rect.expand(4.0).contains(pointer));

    ui.label(rich_semibold(
        "Block Palette",
        FONT_SIZE_BODY_PX,
        TEXT_PRIMARY,
    ));
    ui.label(
        egui::RichText::new("Click to append or drag to snap")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    );
    ui.add_space(6.0);

    render_palette_search_bar(ui, &mut drag_state.palette_search_query);
    ui.add_space(5.0);
    render_category_filter_strip(
        ui,
        &mut drag_state.selected_category_filter,
        custom_blocks.len(),
    );
    ui.add_space(8.0);

    let normalized_query = drag_state.palette_search_query.trim().to_ascii_lowercase();
    let active_filter = drag_state.selected_category_filter;
    let sections: [(&str, BlockCategory); 5] = [
        ("Triggers", BlockCategory::Trigger),
        ("Control", BlockCategory::Control),
        ("Actions", BlockCategory::Action),
        ("Timing", BlockCategory::Timing),
        ("Variables", BlockCategory::Variable),
    ];

    let mut rendered_any_item = false;
    ScrollArea::vertical()
        .id_salt("block_palette_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if active_filter == PaletteCategoryFilter::Custom {
                rendered_any_item |= render_custom_tiles_section(
                    ui,
                    custom_blocks,
                    &normalized_query,
                    true,
                    drag_state,
                    emitted_actions,
                );
            }
            for (section_title, category) in sections {
                if !active_filter.matches_category(category) {
                    continue;
                }
                rendered_any_item |= render_palette_section(
                    ui,
                    section_title,
                    category,
                    &normalized_query,
                    drag_state,
                    emitted_actions,
                );
            }
            if active_filter == PaletteCategoryFilter::All {
                rendered_any_item |= render_custom_tiles_section(
                    ui,
                    custom_blocks,
                    &normalized_query,
                    false,
                    drag_state,
                    emitted_actions,
                );
            }
            if !rendered_any_item {
                render_empty_filter_state(ui, drag_state);
            }
        });

    if drag_state.is_hovering_trash_zone {
        paint_palette_trash_overlay(ui, palette_rect);
    } else if is_dragging_existing {
        paint_palette_trash_hint_banner(ui, palette_rect);
    }
}

fn render_palette_search_bar(ui: &mut Ui, search_query: &mut String) {
    ui.horizontal(|ui| {
        let has_query = !search_query.is_empty();
        let input_width_px = (ui.available_width() - if has_query { 28.0 } else { 0.0 }).max(80.0);
        ui.add(
            TextEdit::singleline(search_query)
                .desired_width(input_width_px)
                .hint_text("Filter blocks..."),
        );
        if has_query {
            let clear_button = egui::Button::new(rich_semibold("×", FONT_SIZE_BODY_PX, TEXT_MUTED))
                .fill(SURFACE_ELEVATED)
                .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
                .min_size(vec2(22.0, 24.0));
            if ui
                .add(clear_button)
                .on_hover_text("Clear search filter")
                .clicked()
            {
                search_query.clear();
            }
        }
    });
}

fn render_category_filter_strip(
    ui: &mut Ui,
    active_filter: &mut PaletteCategoryFilter,
    custom_tile_count: usize,
) {
    const FILTER_ROWS: [[PaletteCategoryFilter; 3]; 2] = [
        [
            PaletteCategoryFilter::All,
            PaletteCategoryFilter::Triggers,
            PaletteCategoryFilter::Control,
        ],
        [
            PaletteCategoryFilter::Actions,
            PaletteCategoryFilter::Timing,
            PaletteCategoryFilter::Variables,
        ],
    ];
    let pill_gap_px = 4.0;
    let full_width_px = ui.available_width();
    let pill_width_px = ((full_width_px - (pill_gap_px * 2.0)) / 3.0).max(56.0);

    for (row_index, row_filters) in FILTER_ROWS.iter().enumerate() {
        if row_index > 0 {
            ui.add_space(4.0);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = pill_gap_px;
            for candidate_filter in row_filters {
                render_category_filter_pill(
                    ui,
                    *candidate_filter,
                    active_filter,
                    pill_width_px,
                    None,
                );
            }
        });
    }

    ui.add_space(4.0);
    render_category_filter_pill(
        ui,
        PaletteCategoryFilter::Custom,
        active_filter,
        full_width_px.max(120.0),
        Some(custom_tile_count),
    );
}

fn render_category_filter_pill(
    ui: &mut Ui,
    candidate_filter: PaletteCategoryFilter,
    active_filter: &mut PaletteCategoryFilter,
    pill_width_px: f32,
    trailing_count: Option<usize>,
) {
    let is_selected = *active_filter == candidate_filter;
    let (pill_rect, response) = ui.allocate_exact_size(vec2(pill_width_px, 24.0), Sense::click());
    let (fill_color, border_color, text_color) = if is_selected {
        (ACCENT_PRIMARY_TINT, ACCENT_PRIMARY, TEXT_PRIMARY)
    } else if response.hovered() {
        (SURFACE_HOVER, BORDER_STRONG, TEXT_MUTED)
    } else {
        (SURFACE_ELEVATED, BORDER_SUBTLE, TEXT_MUTED)
    };
    ui.painter().rect(
        pill_rect,
        CornerRadius::same(CORNER_RADIUS_PILL_PX),
        fill_color,
        Stroke::new(1.0, border_color),
        StrokeKind::Inside,
    );
    ui.painter().circle_filled(
        pos2(pill_rect.min.x + 10.0, pill_rect.center().y),
        CATEGORY_DOT_RADIUS_PX,
        candidate_filter.dot_color(),
    );
    ui.painter().text(
        pos2(pill_rect.min.x + 18.0, pill_rect.center().y),
        Align2::LEFT_CENTER,
        candidate_filter.display_label(),
        FontId::new(11.5, font_family_semibold()),
        text_color,
    );
    if let Some(count_value) = trailing_count {
        ui.painter().text(
            pos2(pill_rect.max.x - 10.0, pill_rect.center().y),
            Align2::RIGHT_CENTER,
            count_value.to_string(),
            FontId::new(11.0, font_family_mono_bold()),
            if is_selected {
                ACCENT_PRIMARY
            } else {
                TEXT_MUTED
            },
        );
    }
    if response.clicked() {
        *active_filter = candidate_filter;
    }
}

fn render_palette_section(
    ui: &mut Ui,
    section_title: &str,
    category: BlockCategory,
    normalized_query: &str,
    drag_state: &mut BlockDragState,
    emitted_actions: &mut Vec<PaletteAction>,
) -> bool {
    let matches: Vec<&PaletteTemplateSpec> = PALETTE_TEMPLATES
        .iter()
        .filter(|template| {
            template.category == category
                && matches_search_query(
                    normalized_query,
                    section_title,
                    template.title,
                    template.summary,
                )
        })
        .collect();
    if matches.is_empty() {
        return false;
    }

    ui.horizontal(|ui| {
        let (dot_rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
        ui.painter().circle_filled(
            dot_rect.center(),
            CATEGORY_DOT_RADIUS_PX,
            category_color(category),
        );
        ui.label(rich_semibold(section_title, FONT_SIZE_SMALL_PX, TEXT_MUTED));
    });
    ui.add_space(4.0);

    for template in matches {
        render_palette_mini_block(ui, template, drag_state, emitted_actions);
    }
    ui.add_space(8.0);
    true
}

pub(super) fn matches_search_query(
    normalized_query: &str,
    section_title: &str,
    title: &str,
    summary: &str,
) -> bool {
    if normalized_query.is_empty() {
        return true;
    }
    title.to_ascii_lowercase().contains(normalized_query)
        || summary.to_ascii_lowercase().contains(normalized_query)
        || section_title
            .to_ascii_lowercase()
            .contains(normalized_query)
}

fn render_palette_mini_block(
    ui: &mut Ui,
    template: &PaletteTemplateSpec,
    drag_state: &mut BlockDragState,
    emitted_actions: &mut Vec<PaletteAction>,
) {
    let is_trigger_hat = template.category == BlockCategory::Trigger;
    let visual_colors = block_visual_colors(template.category);
    let block_height_px = if is_trigger_hat {
        MINI_HAT_HEIGHT_PX
    } else {
        MINI_BLOCK_HEIGHT_PX
    };
    let width_px = ui.available_width().max(140.0);
    let (allocated_rect, response) = ui.allocate_exact_size(
        vec2(
            width_px,
            block_height_px + MINI_BLOCK_GEOMETRY.notch_depth_px,
        ),
        Sense::click_and_drag(),
    );

    let body_rect = Rect::from_min_max(
        allocated_rect.min,
        pos2(allocated_rect.max.x, allocated_rect.min.y + block_height_px),
    );
    let fill_color = if response.hovered() {
        visual_colors.fill_hover
    } else {
        visual_colors.fill
    };
    let shapes = if is_trigger_hat {
        build_trigger_hat_shapes(
            body_rect,
            MINI_BLOCK_GEOMETRY,
            fill_color,
            visual_colors.border,
            visual_colors.top_highlight,
            MINI_BLOCK_GEOMETRY.stroke_width_px,
        )
    } else {
        build_leaf_block_shapes(
            body_rect,
            MINI_BLOCK_GEOMETRY,
            fill_color,
            visual_colors.border,
            visual_colors.top_highlight,
            MINI_BLOCK_GEOMETRY.stroke_width_px,
            true,
            true,
        )
    };
    ui.painter().add(Shape::Vec(shapes));

    let text_center_y_px = if is_trigger_hat {
        let dome_px = MINI_BLOCK_GEOMETRY.hat_dome_height_px;
        body_rect.min.y + dome_px + (block_height_px - dome_px) * 0.5
    } else {
        body_rect.center().y + 1.0
    };
    ui.painter().text(
        pos2(body_rect.min.x + 12.0, text_center_y_px),
        Align2::LEFT_CENTER,
        template.title,
        FontId::new(12.5, font_family_semibold()),
        visual_colors.text_primary,
    );

    if template.is_container {
        let mouth_rect = Rect::from_min_max(
            pos2(body_rect.max.x - 32.0, text_center_y_px - 5.0),
            pos2(body_rect.max.x - 22.0, text_center_y_px + 5.0),
        );
        ui.painter().add(Shape::Path(PathShape::line(
            vec![
                mouth_rect.right_top(),
                mouth_rect.left_top(),
                mouth_rect.left_bottom(),
                mouth_rect.right_bottom(),
            ],
            Stroke::new(1.4, visual_colors.text_muted),
        )));
    }

    let is_popup_open = Popup::is_any_open(ui.ctx());
    if response.hovered() && !is_popup_open {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
        ui.painter().text(
            pos2(body_rect.max.x - 10.0, text_center_y_px),
            Align2::RIGHT_CENTER,
            if is_trigger_hat { "Set" } else { "+" },
            FontId::new(11.5, font_family_semibold()),
            visual_colors.text_muted,
        );
    }

    let response = response.on_hover_text(template.summary);
    if !is_popup_open && response.drag_started_by(PointerButton::Primary) {
        let (_, drag_payload) = build_template_action_and_payload(template.id);
        drag_state.start_drag(
            drag_payload,
            template.title,
            template.summary,
            template.category,
        );
    } else if response.clicked() {
        let (palette_action, _) = build_template_action_and_payload(template.id);
        emitted_actions.push(palette_action);
    }
    ui.add_space(4.0);
}
