use egui::{
    Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, epaint::PathShape, pos2,
};

use crate::ui::theme::BG_CANVAS;

/// Shared geometric proportions for rendering full canvas blocks and mini palette blocks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BlockGeometrySpec {
    pub notch_offset_x_px: f32,
    pub notch_outer_width_px: f32,
    pub notch_slope_x_px: f32,
    pub notch_depth_px: f32,
    pub corner_radius_px: f32,
    pub hat_dome_height_px: f32,
    pub stroke_width_px: f32,
}

pub(crate) const FULL_BLOCK_GEOMETRY: BlockGeometrySpec = BlockGeometrySpec {
    notch_offset_x_px: 22.0,
    notch_outer_width_px: 26.0,
    notch_slope_x_px: 4.0,
    notch_depth_px: 6.0,
    corner_radius_px: 5.0,
    hat_dome_height_px: 13.0,
    stroke_width_px: 1.5,
};

pub(crate) const MINI_BLOCK_GEOMETRY: BlockGeometrySpec = BlockGeometrySpec {
    notch_offset_x_px: 14.0,
    notch_outer_width_px: 16.0,
    notch_slope_x_px: 2.5,
    notch_depth_px: 3.5,
    corner_radius_px: 4.0,
    hat_dome_height_px: 6.0,
    stroke_width_px: 1.25,
};

pub(crate) const C_BLOCK_SPINE_WIDTH_PX: f32 = 16.0;

const TOP_HIGHLIGHT_ALPHA: u8 = 135;
const DASH_SEGMENT_PX: f32 = 5.0;
const DASH_GAP_PX: f32 = 4.0;

#[derive(Clone, Copy)]
enum CornerSide {
    NorthWest,
    NorthEast,
    SouthEast,
    SouthWest,
}

/// Builds convex fill shapes, seamless outer contour, and top highlight for a Trigger Hat block.
pub(crate) fn build_trigger_hat_shapes(
    rect: Rect,
    geometry: BlockGeometrySpec,
    fill_color: Color32,
    border_color: Color32,
    top_highlight: Color32,
    stroke_width_px: f32,
) -> Vec<Shape> {
    let shoulder_y_px = (rect.min.y + geometry.hat_dome_height_px).min(rect.max.y);
    let max_dome_width_px = (rect.width() - geometry.corner_radius_px - 2.0).max(0.0);
    let dome_width_px = (rect.width() * 0.46)
        .clamp(72.0, 212.0)
        .min(max_dome_width_px);
    let dome_steps: usize = 12;
    let mut dome_points = Vec::with_capacity(dome_steps + 1);
    for step_index in 0..=dome_steps {
        let fraction = (step_index as f32) / (dome_steps as f32);
        let arch_y_px =
            shoulder_y_px - geometry.hat_dome_height_px * (fraction * std::f32::consts::PI).sin();
        dome_points.push(pos2(rect.min.x + dome_width_px * fraction, arch_y_px));
    }

    let mut shapes = Vec::with_capacity(6);
    let mut dome_polygon = dome_points.clone();
    dome_polygon.push(pos2(rect.min.x + dome_width_px, shoulder_y_px + 0.6));
    dome_polygon.push(pos2(rect.min.x, shoulder_y_px + 0.6));
    shapes.push(Shape::convex_polygon(
        dome_polygon,
        fill_color,
        Stroke::NONE,
    ));

    let radius_u8 = geometry.corner_radius_px as u8;
    let body_corners = CornerRadius {
        nw: 0,
        ne: radius_u8,
        sw: radius_u8,
        se: radius_u8,
    };
    let body_rect = Rect::from_min_max(pos2(rect.min.x, shoulder_y_px), rect.max);
    shapes.push(Shape::rect_filled(body_rect, body_corners, fill_color));
    let notch_x_px = rect.min.x + geometry.notch_offset_x_px;
    shapes.push(build_bottom_bump_fill(
        notch_x_px, rect.max.y, geometry, fill_color,
    ));

    let mut outline = dome_points.clone();
    let radius_px = geometry.corner_radius_px;
    push_corner(
        &mut outline,
        rect.max.x,
        shoulder_y_px,
        radius_px,
        CornerSide::NorthEast,
    );
    push_corner(
        &mut outline,
        rect.max.x,
        rect.max.y,
        radius_px,
        CornerSide::SouthEast,
    );
    push_notch_contour(&mut outline, notch_x_px, rect.max.y, geometry, false);
    push_corner(
        &mut outline,
        rect.min.x,
        rect.max.y,
        radius_px,
        CornerSide::SouthWest,
    );
    shapes.push(Shape::Path(PathShape::closed_line(
        outline,
        Stroke::new(stroke_width_px, border_color),
    )));

    let mut highlight_curve: Vec<Pos2> = dome_points
        .into_iter()
        .map(|point| pos2(point.x, point.y + 1.2))
        .collect();
    highlight_curve.push(pos2(rect.max.x - radius_px, shoulder_y_px + 1.2));
    let highlight_stroke = Stroke::new(1.0, with_alpha(top_highlight, TOP_HIGHLIGHT_ALPHA));
    shapes.push(Shape::Path(PathShape::line(
        highlight_curve,
        highlight_stroke,
    )));
    shapes
}

/// Builds convex fill shapes, seamless outer contour, and top highlight for a statement snap-block.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_leaf_block_shapes(
    rect: Rect,
    geometry: BlockGeometrySpec,
    fill_color: Color32,
    border_color: Color32,
    top_highlight: Color32,
    stroke_width_px: f32,
    has_top_notch: bool,
    has_bottom_bump: bool,
) -> Vec<Shape> {
    let notch_x_px = rect.min.x + geometry.notch_offset_x_px;
    let radius_px = geometry.corner_radius_px;
    let mut shapes = Vec::with_capacity(8);

    push_notched_bar_fill(
        &mut shapes,
        rect,
        has_top_notch.then_some(notch_x_px),
        has_bottom_bump.then_some(notch_x_px),
        true,
        true,
        geometry,
        fill_color,
    );

    let mut outline = Vec::with_capacity(28);
    push_corner(
        &mut outline,
        rect.min.x,
        rect.min.y,
        radius_px,
        CornerSide::NorthWest,
    );
    if has_top_notch {
        push_notch_contour(&mut outline, notch_x_px, rect.min.y, geometry, true);
    }
    push_corner(
        &mut outline,
        rect.max.x,
        rect.min.y,
        radius_px,
        CornerSide::NorthEast,
    );
    push_corner(
        &mut outline,
        rect.max.x,
        rect.max.y,
        radius_px,
        CornerSide::SouthEast,
    );
    if has_bottom_bump {
        push_notch_contour(&mut outline, notch_x_px, rect.max.y, geometry, false);
    }
    push_corner(
        &mut outline,
        rect.min.x,
        rect.max.y,
        radius_px,
        CornerSide::SouthWest,
    );
    shapes.push(Shape::Path(PathShape::closed_line(
        outline,
        Stroke::new(stroke_width_px, border_color),
    )));

    push_top_highlight_segments(
        &mut shapes,
        rect,
        has_top_notch.then_some(notch_x_px),
        geometry,
        top_highlight,
    );
    shapes
}

/// Builds a seamless unified continuous C-block (or E-block for `If / Else`) with zero internal seams.
pub(crate) fn build_c_block_shapes(
    header_rect: Rect,
    else_rect: Option<Rect>,
    footer_rect: Rect,
    fill_color: Color32,
    border_color: Color32,
    top_highlight: Color32,
    stroke_width_px: f32,
) -> Vec<Shape> {
    let geometry = FULL_BLOCK_GEOMETRY;
    let radius_px = geometry.corner_radius_px;
    let left_x_px = header_rect.min.x;
    let spine_right_x_px = left_x_px + C_BLOCK_SPINE_WIDTH_PX;
    let outer_notch_x_px = left_x_px + geometry.notch_offset_x_px;
    let inner_notch_x_px = spine_right_x_px + geometry.notch_offset_x_px;
    let mut shapes = Vec::with_capacity(18);

    push_notched_bar_fill(
        &mut shapes,
        header_rect,
        Some(outer_notch_x_px),
        Some(inner_notch_x_px),
        true,
        false,
        geometry,
        fill_color,
    );
    let spine_rect = Rect::from_min_max(
        pos2(left_x_px, header_rect.max.y - 1.0),
        pos2(
            spine_right_x_px + 0.5,
            footer_rect.min.y + geometry.notch_depth_px + 1.0,
        ),
    );
    shapes.push(Shape::rect_filled(
        spine_rect,
        CornerRadius::ZERO,
        fill_color,
    ));

    if let Some(else_bar) = else_rect {
        let joined_else =
            Rect::from_min_max(pos2(spine_right_x_px - 0.5, else_bar.min.y), else_bar.max);
        push_notched_bar_fill(
            &mut shapes,
            joined_else,
            Some(inner_notch_x_px),
            Some(inner_notch_x_px),
            false,
            false,
            geometry,
            fill_color,
        );
    }

    let footer_shoulder_max_y_px =
        (footer_rect.min.y + geometry.notch_depth_px).min(footer_rect.max.y);
    let footer_shoulders = Rect::from_min_max(
        pos2(spine_right_x_px - 0.5, footer_rect.min.y),
        pos2(footer_rect.max.x, footer_shoulder_max_y_px),
    );
    push_top_shoulders(
        &mut shapes,
        footer_shoulders,
        inner_notch_x_px,
        false,
        geometry,
        fill_color,
    );
    let footer_body =
        Rect::from_min_max(pos2(left_x_px, footer_shoulder_max_y_px), footer_rect.max);
    let radius_u8 = radius_px as u8;
    shapes.push(Shape::rect_filled(
        footer_body,
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: radius_u8,
            se: radius_u8,
        },
        fill_color,
    ));
    shapes.push(build_bottom_bump_fill(
        outer_notch_x_px,
        footer_rect.max.y,
        geometry,
        fill_color,
    ));

    let mut outline = Vec::with_capacity(56);
    push_corner(
        &mut outline,
        left_x_px,
        header_rect.min.y,
        radius_px,
        CornerSide::NorthWest,
    );
    push_notch_contour(
        &mut outline,
        outer_notch_x_px,
        header_rect.min.y,
        geometry,
        true,
    );
    push_corner(
        &mut outline,
        header_rect.max.x,
        header_rect.min.y,
        radius_px,
        CornerSide::NorthEast,
    );
    push_corner(
        &mut outline,
        header_rect.max.x,
        header_rect.max.y,
        radius_px,
        CornerSide::SouthEast,
    );
    push_notch_contour(
        &mut outline,
        inner_notch_x_px,
        header_rect.max.y,
        geometry,
        false,
    );
    outline.push(pos2(spine_right_x_px, header_rect.max.y));

    if let Some(else_bar) = else_rect {
        outline.push(pos2(spine_right_x_px, else_bar.min.y));
        push_notch_contour(
            &mut outline,
            inner_notch_x_px,
            else_bar.min.y,
            geometry,
            true,
        );
        push_corner(
            &mut outline,
            else_bar.max.x,
            else_bar.min.y,
            radius_px,
            CornerSide::NorthEast,
        );
        push_corner(
            &mut outline,
            else_bar.max.x,
            else_bar.max.y,
            radius_px,
            CornerSide::SouthEast,
        );
        push_notch_contour(
            &mut outline,
            inner_notch_x_px,
            else_bar.max.y,
            geometry,
            false,
        );
        outline.push(pos2(spine_right_x_px, else_bar.max.y));
    }

    outline.push(pos2(spine_right_x_px, footer_rect.min.y));
    push_notch_contour(
        &mut outline,
        inner_notch_x_px,
        footer_rect.min.y,
        geometry,
        true,
    );
    push_corner(
        &mut outline,
        footer_rect.max.x,
        footer_rect.min.y,
        radius_px,
        CornerSide::NorthEast,
    );
    push_corner(
        &mut outline,
        footer_rect.max.x,
        footer_rect.max.y,
        radius_px,
        CornerSide::SouthEast,
    );
    push_notch_contour(
        &mut outline,
        outer_notch_x_px,
        footer_rect.max.y,
        geometry,
        false,
    );
    push_corner(
        &mut outline,
        left_x_px,
        footer_rect.max.y,
        radius_px,
        CornerSide::SouthWest,
    );
    shapes.push(Shape::Path(PathShape::closed_line(
        outline,
        Stroke::new(stroke_width_px, border_color),
    )));

    push_top_highlight_segments(
        &mut shapes,
        header_rect,
        Some(outer_notch_x_px),
        geometry,
        top_highlight,
    );
    shapes
}

#[allow(clippy::too_many_arguments)]
fn push_notched_bar_fill(
    shapes: &mut Vec<Shape>,
    bar_rect: Rect,
    top_notch_x_px: Option<f32>,
    bottom_bump_x_px: Option<f32>,
    round_nw: bool,
    round_sw: bool,
    geometry: BlockGeometrySpec,
    fill_color: Color32,
) {
    let radius_u8 = geometry.corner_radius_px as u8;
    let sw_radius = if round_sw { radius_u8 } else { 0 };
    if let Some(notch_x_px) = top_notch_x_px {
        let shoulder_max_y_px = (bar_rect.min.y + geometry.notch_depth_px).min(bar_rect.max.y);
        let shoulder_band =
            Rect::from_min_max(bar_rect.min, pos2(bar_rect.max.x, shoulder_max_y_px));
        push_top_shoulders(
            shapes,
            shoulder_band,
            notch_x_px,
            round_nw,
            geometry,
            fill_color,
        );
        if shoulder_band.max.y < bar_rect.max.y {
            let body = Rect::from_min_max(pos2(bar_rect.min.x, shoulder_band.max.y), bar_rect.max);
            let corners = CornerRadius {
                nw: 0,
                ne: 0,
                sw: sw_radius,
                se: radius_u8,
            };
            shapes.push(Shape::rect_filled(body, corners, fill_color));
        }
    } else {
        let nw_radius = if round_nw { radius_u8 } else { 0 };
        let corners = CornerRadius {
            nw: nw_radius,
            ne: radius_u8,
            sw: sw_radius,
            se: radius_u8,
        };
        shapes.push(Shape::rect_filled(bar_rect, corners, fill_color));
    }
    if let Some(bump_x_px) = bottom_bump_x_px {
        shapes.push(build_bottom_bump_fill(
            bump_x_px,
            bar_rect.max.y,
            geometry,
            fill_color,
        ));
    }
}

fn push_top_shoulders(
    shapes: &mut Vec<Shape>,
    band_rect: Rect,
    notch_x_px: f32,
    round_nw: bool,
    geometry: BlockGeometrySpec,
    fill_color: Color32,
) {
    let slope_px = geometry.notch_slope_x_px;
    let width_px = geometry.notch_outer_width_px;
    let floor_y_px = band_rect.max.y + 0.5;
    let radius_px = geometry.corner_radius_px.min(band_rect.height().max(0.0));

    let mut left_polygon = Vec::with_capacity(6);
    if round_nw {
        push_corner(
            &mut left_polygon,
            band_rect.min.x,
            band_rect.min.y,
            radius_px,
            CornerSide::NorthWest,
        );
    } else {
        left_polygon.push(band_rect.left_top());
    }
    left_polygon.extend_from_slice(&[
        pos2(notch_x_px, band_rect.min.y),
        pos2(notch_x_px + slope_px, floor_y_px),
        pos2(band_rect.min.x, floor_y_px),
    ]);
    shapes.push(Shape::convex_polygon(
        left_polygon,
        fill_color,
        Stroke::NONE,
    ));

    let mut right_polygon = vec![pos2(notch_x_px + width_px, band_rect.min.y)];
    push_corner(
        &mut right_polygon,
        band_rect.max.x,
        band_rect.min.y,
        radius_px,
        CornerSide::NorthEast,
    );
    right_polygon.extend_from_slice(&[
        pos2(band_rect.max.x, floor_y_px),
        pos2(notch_x_px + width_px - slope_px, floor_y_px),
    ]);
    shapes.push(Shape::convex_polygon(
        right_polygon,
        fill_color,
        Stroke::NONE,
    ));
}

fn push_top_highlight_segments(
    shapes: &mut Vec<Shape>,
    rect: Rect,
    notch_x_px: Option<f32>,
    geometry: BlockGeometrySpec,
    top_highlight: Color32,
) {
    let stroke = Stroke::new(1.0, with_alpha(top_highlight, TOP_HIGHLIGHT_ALPHA));
    let line_y_px = rect.min.y + 1.2;
    let radius_px = geometry.corner_radius_px;
    if let Some(notch_start_x_px) = notch_x_px {
        let left_segment = [
            pos2(rect.min.x + radius_px, line_y_px),
            pos2(notch_start_x_px - 0.5, line_y_px),
        ];
        let right_segment = [
            pos2(
                notch_start_x_px + geometry.notch_outer_width_px + 0.5,
                line_y_px,
            ),
            pos2(rect.max.x - radius_px, line_y_px),
        ];
        shapes.push(Shape::line_segment(left_segment, stroke));
        shapes.push(Shape::line_segment(right_segment, stroke));
    } else {
        let full_segment = [
            pos2(rect.min.x + radius_px, line_y_px),
            pos2(rect.max.x - radius_px, line_y_px),
        ];
        shapes.push(Shape::line_segment(full_segment, stroke));
    }
}

fn build_bottom_bump_fill(
    notch_x_px: f32,
    bottom_y_px: f32,
    geometry: BlockGeometrySpec,
    fill_color: Color32,
) -> Shape {
    let width_px = geometry.notch_outer_width_px;
    let slope_px = geometry.notch_slope_x_px;
    let tip_y_px = bottom_y_px + geometry.notch_depth_px;
    let polygon = vec![
        pos2(notch_x_px, bottom_y_px - 0.5),
        pos2(notch_x_px + width_px, bottom_y_px - 0.5),
        pos2(notch_x_px + width_px - slope_px, tip_y_px),
        pos2(notch_x_px + slope_px, tip_y_px),
    ];
    Shape::convex_polygon(polygon, fill_color, Stroke::NONE)
}

fn push_notch_contour(
    points: &mut Vec<Pos2>,
    notch_x_px: f32,
    edge_y_px: f32,
    geometry: BlockGeometrySpec,
    is_left_to_right: bool,
) {
    let width_px = geometry.notch_outer_width_px;
    let slope_px = geometry.notch_slope_x_px;
    let inner_y_px = edge_y_px + geometry.notch_depth_px;
    let vertices = if is_left_to_right {
        [
            pos2(notch_x_px, edge_y_px),
            pos2(notch_x_px + slope_px, inner_y_px),
            pos2(notch_x_px + width_px - slope_px, inner_y_px),
            pos2(notch_x_px + width_px, edge_y_px),
        ]
    } else {
        [
            pos2(notch_x_px + width_px, edge_y_px),
            pos2(notch_x_px + width_px - slope_px, inner_y_px),
            pos2(notch_x_px + slope_px, inner_y_px),
            pos2(notch_x_px, edge_y_px),
        ]
    };
    points.extend_from_slice(&vertices);
}

fn push_corner(
    points: &mut Vec<Pos2>,
    corner_x_px: f32,
    corner_y_px: f32,
    radius_px: f32,
    side: CornerSide,
) {
    let (enter_x, enter_y, exit_x, exit_y) = match side {
        CornerSide::NorthWest => (0.0, 1.0, 1.0, 0.0),
        CornerSide::NorthEast => (-1.0, 0.0, 0.0, 1.0),
        CornerSide::SouthEast => (0.0, -1.0, -1.0, 0.0),
        CornerSide::SouthWest => (1.0, 0.0, 0.0, -1.0),
    };
    let mid_x_px = (enter_x + exit_x) * radius_px * 0.3;
    let mid_y_px = (enter_y + exit_y) * radius_px * 0.3;
    points.extend_from_slice(&[
        pos2(
            corner_x_px + enter_x * radius_px,
            corner_y_px + enter_y * radius_px,
        ),
        pos2(corner_x_px + mid_x_px, corner_y_px + mid_y_px),
        pos2(
            corner_x_px + exit_x * radius_px,
            corner_y_px + exit_y * radius_px,
        ),
    ]);
}

/// Paints a crisp dashed rectangular border for empty drop pockets.
pub(crate) fn paint_dashed_rect_border(painter: &Painter, rect: Rect, stroke: Stroke) {
    let edges = [
        (rect.left_top(), rect.right_top()),
        (rect.right_top(), rect.right_bottom()),
        (rect.right_bottom(), rect.left_bottom()),
        (rect.left_bottom(), rect.left_top()),
    ];
    for (start_point, end_point) in edges {
        let delta = end_point - start_point;
        let total_px = delta.length();
        if total_px <= 0.1 {
            continue;
        }
        let unit = delta / total_px;
        let mut offset_px = 0.0;
        while offset_px < total_px {
            let segment_end_px = (offset_px + DASH_SEGMENT_PX).min(total_px);
            painter.line_segment(
                [
                    start_point + unit * offset_px,
                    start_point + unit * segment_end_px,
                ],
                stroke,
            );
            offset_px += DASH_SEGMENT_PX + DASH_GAP_PX;
        }
    }
}

/// Paints a compact overlapping-squares duplicate icon centered at `center`.
pub(crate) fn paint_duplicate_icon(painter: &Painter, center: Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    let back_rect = Rect::from_min_max(
        pos2(center.x - 4.5, center.y - 5.0),
        pos2(center.x + 1.5, center.y + 1.5),
    );
    let front_rect = Rect::from_min_max(
        pos2(center.x - 1.5, center.y - 1.5),
        pos2(center.x + 4.5, center.y + 5.0),
    );
    painter.rect_stroke(back_rect, 1.0, stroke, StrokeKind::Inside);
    painter.rect_filled(front_rect, 1.0, BG_CANVAS);
    painter.rect_stroke(front_rect, 1.0, stroke, StrokeKind::Inside);
}

/// Paints a compact crisp cross delete icon centered at `center`.
pub(crate) fn paint_close_icon(painter: &Painter, center: Pos2, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let half_px = 3.8;
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

/// Returns `color` with an explicit unmultiplied alpha byte.
pub(crate) fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}
