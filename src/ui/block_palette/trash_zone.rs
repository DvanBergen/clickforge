use egui::{
    Align2, Color32, CornerRadius, FontId, Frame, Margin, Painter, Pos2, Rect, Shape, Stroke,
    StrokeKind, Ui, epaint::PathShape, pos2,
};

use crate::ui::block_palette::{BlockDragState, PaletteCategoryFilter};
use crate::ui::theme::{
    BORDER_STRONG, BORDER_SUBTLE, CORNER_RADIUS_MD_PX, CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX,
    FONT_SIZE_SMALL_PX, STATUS_DANGER_INK, STATUS_DANGER_SOLID, STATUS_DANGER_TINT,
    SURFACE_ELEVATED, SURFACE_HOVER, TEXT_PRIMARY, font_family_semibold, rich_semibold,
};

const TRASH_OVERLAY_ALPHA: u8 = 228;
const TRASH_BANNER_HEIGHT_PX: f32 = 30.0;

pub(super) fn render_empty_filter_state(ui: &mut Ui, drag_state: &mut BlockDragState) {
    ui.add_space(12.0);
    Frame::NONE
        .fill(SURFACE_ELEVATED)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(rich_semibold(
                    "No matching blocks",
                    FONT_SIZE_SMALL_PX,
                    TEXT_PRIMARY,
                ));
                ui.add_space(6.0);
                let reset_button = egui::Button::new(rich_semibold(
                    "Reset Filters",
                    FONT_SIZE_SMALL_PX,
                    TEXT_PRIMARY,
                ))
                .fill(SURFACE_HOVER)
                .stroke(Stroke::new(1.0, BORDER_STRONG))
                .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX));
                if ui.add(reset_button).clicked() {
                    drag_state.palette_search_query.clear();
                    drag_state.selected_category_filter = PaletteCategoryFilter::All;
                }
            });
        });
}

pub(super) fn paint_palette_trash_overlay(ui: &Ui, palette_rect: Rect) {
    let painter = ui.painter_at(palette_rect);
    let overlay_fill = Color32::from_rgba_unmultiplied(
        STATUS_DANGER_TINT.r(),
        STATUS_DANGER_TINT.g(),
        STATUS_DANGER_TINT.b(),
        TRASH_OVERLAY_ALPHA,
    );
    painter.rect(
        palette_rect,
        CornerRadius::same(CORNER_RADIUS_MD_PX),
        overlay_fill,
        Stroke::new(2.0, STATUS_DANGER_SOLID),
        StrokeKind::Inside,
    );
    let center = palette_rect.center();
    paint_trash_bin_icon(&painter, pos2(center.x, center.y - 26.0), STATUS_DANGER_INK);
    painter.text(
        pos2(center.x, center.y + 8.0),
        Align2::CENTER_CENTER,
        "Drop here to delete block",
        FontId::new(FONT_SIZE_BODY_PX, font_family_semibold()),
        TEXT_PRIMARY,
    );
    painter.text(
        pos2(center.x, center.y + 28.0),
        Align2::CENTER_CENTER,
        "Release mouse to remove",
        FontId::new(FONT_SIZE_SMALL_PX, egui::FontFamily::Proportional),
        STATUS_DANGER_INK,
    );
}

pub(super) fn paint_palette_trash_hint_banner(ui: &Ui, palette_rect: Rect) {
    let banner_rect = Rect::from_min_max(
        pos2(
            palette_rect.min.x + 2.0,
            palette_rect.max.y - TRASH_BANNER_HEIGHT_PX - 2.0,
        ),
        pos2(palette_rect.max.x - 2.0, palette_rect.max.y - 2.0),
    );
    let painter = ui.painter_at(palette_rect);
    painter.rect(
        banner_rect,
        CornerRadius::same(CORNER_RADIUS_SM_PX),
        STATUS_DANGER_TINT,
        Stroke::new(1.0, STATUS_DANGER_SOLID),
        StrokeKind::Inside,
    );
    painter.text(
        banner_rect.center(),
        Align2::CENTER_CENTER,
        "Drag block here to delete",
        FontId::new(FONT_SIZE_SMALL_PX, font_family_semibold()),
        STATUS_DANGER_INK,
    );
}

fn paint_trash_bin_icon(painter: &Painter, center: Pos2, icon_color: Color32) {
    let stroke = Stroke::new(1.8, icon_color);
    let lid_bar = [
        pos2(center.x - 12.0, center.y - 8.0),
        pos2(center.x + 12.0, center.y - 8.0),
    ];
    painter.line_segment(lid_bar, stroke);
    let handle = vec![
        pos2(center.x - 4.0, center.y - 8.0),
        pos2(center.x - 4.0, center.y - 12.0),
        pos2(center.x + 4.0, center.y - 12.0),
        pos2(center.x + 4.0, center.y - 8.0),
    ];
    let body = vec![
        pos2(center.x - 9.0, center.y - 8.0),
        pos2(center.x - 7.0, center.y + 12.0),
        pos2(center.x + 7.0, center.y + 12.0),
        pos2(center.x + 9.0, center.y - 8.0),
    ];
    painter.add(Shape::Path(PathShape::line(handle, stroke)));
    painter.add(Shape::Path(PathShape::line(body, stroke)));
    for rib_offset_x_px in [-3.0_f32, 0.0, 3.0] {
        let rib = [
            pos2(center.x + rib_offset_x_px, center.y - 3.5),
            pos2(center.x + rib_offset_x_px, center.y + 7.5),
        ];
        painter.line_segment(rib, Stroke::new(1.3, icon_color));
    }
}
