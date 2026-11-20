use egui::{
    Align, Button, CornerRadius, Frame, Layout, Margin, Rect, RichText, ScrollArea, Sense, Stroke,
    StrokeKind, Ui, pos2, vec2,
};

use crate::engine::timing::compute_gaussian_histogram_buckets;
use crate::models::{
    ActiveWindowSnapshot, CoordinatorSnapshot, InputSubsystemStatus, ScriptDocument,
    ScriptRuntimeStatus, StatementBlock, StatementKind, TelemetryLevel, TelemetryLogEntry,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BG_CANVAS, BORDER_SUBTLE, CATEGORY_ACTION,
    CATEGORY_TIMING, CORNER_RADIUS_SM_PX, FONT_SIZE_SMALL_PX, STATUS_DANGER, STATUS_DANGER_TINT,
    STATUS_RUNNING, STATUS_RUNNING_TINT, SURFACE_CODE, SURFACE_ELEVATED, TEXT_MUTED, TEXT_PRIMARY,
    render_badge_pill, truncate_display_text,
};

const HISTOGRAM_COLUMN_WIDTH_PX: f32 = 310.0;
const HISTOGRAM_PLOT_HEIGHT_PX: f32 = 76.0;
const HISTOGRAM_SAMPLE_COUNT: usize = 2_000;
const HISTOGRAM_BUCKET_COUNT: usize = 20;
const MAX_FOCUS_TITLE_CHARS: usize = 36;
const BAR_GAP_PX: f32 = 2.0;

/// Cached Gaussian histogram buckets so the UI only resamples when timing parameters change.
#[derive(Debug, Default, Clone)]
pub struct GaussianHistogramCache {
    cached_key: Option<(u64, u64, u64, u64)>,
    buckets: Vec<u32>,
}

impl GaussianHistogramCache {
    /// Returns cached histogram buckets for the supplied Gaussian offset parameters.
    pub fn buckets_for(
        &mut self,
        min_offset_ms: f64,
        max_offset_ms: f64,
        mean_offset_ms: f64,
        std_dev_ms: f64,
    ) -> &[u32] {
        let parameter_key = (
            min_offset_ms.to_bits(),
            max_offset_ms.to_bits(),
            mean_offset_ms.to_bits(),
            std_dev_ms.to_bits(),
        );
        if self.cached_key != Some(parameter_key) {
            self.buckets = compute_gaussian_histogram_buckets(
                min_offset_ms,
                max_offset_ms,
                mean_offset_ms,
                std_dev_ms,
                HISTOGRAM_SAMPLE_COUNT,
                HISTOGRAM_BUCKET_COUNT,
            );
            self.cached_key = Some(parameter_key);
        }
        &self.buckets
    }
}

/// Gaussian timing parameters extracted from the active visual script for histogram preview.
#[derive(Debug, Clone, Copy)]
struct GaussianBlockPreview {
    base_delay_ms: u64,
    min_offset_ms: f64,
    max_offset_ms: f64,
    mean_offset_ms: f64,
    std_dev_ms: f64,
}

/// Renders the collapsible bottom telemetry bar, subsystem badges, Gaussian histogram, and logs.
pub fn render_telemetry_drawer(
    ui: &mut Ui,
    is_expanded: &mut bool,
    drawer_body_height_px: f32,
    active_script: Option<&ScriptDocument>,
    snapshot: &CoordinatorSnapshot,
    histogram_cache: &mut GaussianHistogramCache,
) -> bool {
    let active_status = active_script.and_then(|script| snapshot.status_for_script(&script.id));
    let did_toggle_drawer = render_drawer_header_strip(ui, is_expanded, active_status, snapshot);

    if !*is_expanded {
        return did_toggle_drawer;
    }

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(6.0);

    let body_height_px = drawer_body_height_px.max(108.0);
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), body_height_px),
        Layout::left_to_right(Align::Min),
        |ui| {
            ui.allocate_ui_with_layout(
                vec2(HISTOGRAM_COLUMN_WIDTH_PX, body_height_px),
                Layout::top_down(Align::Min),
                |ui| {
                    render_timing_histogram_column(
                        ui,
                        active_script,
                        active_status,
                        &snapshot.recent_delay_samples_ms,
                        histogram_cache,
                    );
                },
            );

            ui.separator();

            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                render_telemetry_log_viewer(ui, &snapshot.telemetry_logs, body_height_px);
            });
        },
    );

    did_toggle_drawer
}

fn render_drawer_header_strip(
    ui: &mut Ui,
    is_expanded: &mut bool,
    active_status: Option<&ScriptRuntimeStatus>,
    snapshot: &CoordinatorSnapshot,
) -> bool {
    let mut did_toggle = false;

    ui.horizontal_wrapped(|ui| {
        let toggle_label = if *is_expanded {
            "v Telemetry & Logs"
        } else {
            "^ Telemetry & Logs"
        };
        let toggle_button = Button::new(
            RichText::new(toggle_label)
                .size(FONT_SIZE_SMALL_PX)
                .strong()
                .color(TEXT_PRIMARY),
        )
        .fill(SURFACE_ELEVATED)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE));
        if ui.add(toggle_button).clicked() {
            *is_expanded = !*is_expanded;
            did_toggle = true;
        }

        ui.separator();
        render_focused_window_summary(ui, snapshot.focused_window.as_ref());
        ui.separator();
        render_subsystem_status_badges(ui, &snapshot.input_status);

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let last_delay_label = active_status
                .and_then(|status| status.last_sampled_delay_ms)
                .or_else(|| snapshot.recent_delay_samples_ms.last().copied())
                .map(|delay_ms| format!("Last Delay: {delay_ms} ms"))
                .unwrap_or_else(|| "Last Delay: -".to_string());

            let total_actions = active_status
                .map(|status| status.total_actions_emitted)
                .unwrap_or(0);

            ui.label(
                RichText::new(format!("Actions: {total_actions}   |   {last_delay_label}"))
                    .size(FONT_SIZE_SMALL_PX)
                    .monospace()
                    .color(TEXT_PRIMARY),
            );
        });
    });

    did_toggle
}

fn render_focused_window_summary(ui: &mut Ui, focused_window: Option<&ActiveWindowSnapshot>) {
    let Some(window) = focused_window else {
        ui.label(
            RichText::new("Focus: None")
                .size(FONT_SIZE_SMALL_PX)
                .monospace()
                .color(TEXT_MUTED),
        );
        return;
    };

    let display_title = if window.title.trim().is_empty() {
        "(untitled)".to_string()
    } else {
        truncate_display_text(&window.title, MAX_FOCUS_TITLE_CHARS)
    };
    let app_id_label = if window.app_id.trim().is_empty() {
        "-".to_string()
    } else {
        window.app_id.clone()
    };
    let pid_label = window
        .pid
        .map(|pid| pid.to_string())
        .unwrap_or_else(|| "-".to_string());

    ui.label(
        RichText::new(format!(
            "Focus: {display_title} [{app_id_label}] (PID {pid_label})"
        ))
        .size(FONT_SIZE_SMALL_PX)
        .monospace()
        .color(TEXT_MUTED),
    );
}

fn render_subsystem_status_badges(ui: &mut Ui, input_status: &InputSubsystemStatus) {
    let (uinput_label, uinput_fill, uinput_ink, uinput_border) = if input_status.is_uinput_ready {
        (
            "uinput: Ready",
            STATUS_RUNNING_TINT,
            STATUS_RUNNING,
            STATUS_RUNNING,
        )
    } else {
        (
            "uinput: Offline",
            STATUS_DANGER_TINT,
            STATUS_DANGER,
            STATUS_DANGER,
        )
    };
    let uinput_response = ui.scope(|ui| {
        render_badge_pill(ui, uinput_label, uinput_fill, uinput_ink, uinput_border);
    });
    if let Some(error_detail) = &input_status.uinput_error {
        uinput_response.response.on_hover_text(error_detail);
    }

    let (evdev_label, evdev_fill, evdev_ink, evdev_border) = if input_status.is_evdev_ready {
        (
            format!("evdev: {} dev", input_status.monitored_device_count),
            STATUS_RUNNING_TINT,
            STATUS_RUNNING,
            STATUS_RUNNING,
        )
    } else {
        (
            "evdev: Offline".to_string(),
            STATUS_DANGER_TINT,
            STATUS_DANGER,
            STATUS_DANGER,
        )
    };
    let evdev_response = ui.scope(|ui| {
        render_badge_pill(ui, &evdev_label, evdev_fill, evdev_ink, evdev_border);
    });
    if let Some(error_detail) = &input_status.evdev_error {
        evdev_response.response.on_hover_text(error_detail);
    }

    if input_status.is_trigger_intercept_active {
        render_badge_pill(
            ui,
            "Intercept: GRAB",
            ACCENT_PRIMARY_TINT,
            ACCENT_PRIMARY,
            ACCENT_PRIMARY,
        );
    }

    if !input_status.compositor_name.trim().is_empty() {
        render_badge_pill(
            ui,
            &input_status.compositor_name,
            SURFACE_ELEVATED,
            TEXT_MUTED,
            BORDER_SUBTLE,
        );
    }
}

fn render_timing_histogram_column(
    ui: &mut Ui,
    active_script: Option<&ScriptDocument>,
    active_status: Option<&ScriptRuntimeStatus>,
    recent_delay_samples_ms: &[u64],
    histogram_cache: &mut GaussianHistogramCache,
) {
    let preview_block = active_script
        .and_then(|script| find_first_gaussian_block(&script.visual_program.statements));

    ui.label(
        RichText::new("Gaussian Delay Preview")
            .size(FONT_SIZE_SMALL_PX)
            .strong()
            .color(TEXT_MUTED),
    );

    if let Some(gaussian) = preview_block {
        let buckets = histogram_cache.buckets_for(
            gaussian.min_offset_ms,
            gaussian.max_offset_ms,
            gaussian.mean_offset_ms,
            gaussian.std_dev_ms,
        );
        let live_delay_ms = active_status.and_then(|status| status.last_sampled_delay_ms);
        paint_histogram_plot(ui, buckets, gaussian, live_delay_ms);

        ui.label(
            RichText::new(format!(
                "Base {} ms + G[{:.1}..{:.1}] mean {:.1} sd {:.1} ms",
                gaussian.base_delay_ms,
                gaussian.min_offset_ms,
                gaussian.max_offset_ms,
                gaussian.mean_offset_ms,
                gaussian.std_dev_ms
            ))
            .size(FONT_SIZE_SMALL_PX)
            .monospace()
            .color(TEXT_MUTED),
        );
    } else {
        Frame::NONE
            .fill(SURFACE_CODE)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.set_min_size(vec2(ui.available_width(), HISTOGRAM_PLOT_HEIGHT_PX - 12.0));
                ui.label(
                    RichText::new("Add a Wait Gaussian block to preview its timing curve.")
                        .size(FONT_SIZE_SMALL_PX)
                        .color(TEXT_MUTED),
                );
            });
    }

    ui.add_space(4.0);
    render_recent_delay_stats(ui, recent_delay_samples_ms);
}

fn paint_histogram_plot(
    ui: &mut Ui,
    buckets: &[u32],
    gaussian: GaussianBlockPreview,
    live_delay_ms: Option<u64>,
) {
    let plot_width_px = ui.available_width().max(180.0);
    let (plot_rect, _) = ui.allocate_exact_size(
        vec2(plot_width_px, HISTOGRAM_PLOT_HEIGHT_PX),
        Sense::hover(),
    );
    let painter = ui.painter();

    painter.rect(
        plot_rect,
        CornerRadius::same(CORNER_RADIUS_SM_PX),
        BG_CANVAS,
        Stroke::new(1.0, BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    if buckets.is_empty() {
        return;
    }

    let max_bucket_count = buckets.iter().copied().max().unwrap_or(1).max(1) as f32;
    let inner_rect = plot_rect.shrink(6.0);
    let bucket_count_f32 = buckets.len() as f32;
    let total_gap_px = BAR_GAP_PX * (bucket_count_f32 - 1.0).max(0.0);
    let bar_width_px = ((inner_rect.width() - total_gap_px) / bucket_count_f32).max(2.0);

    for (bucket_index, bucket_count) in buckets.iter().enumerate() {
        let height_ratio = (*bucket_count as f32) / max_bucket_count;
        let bar_height_px = (inner_rect.height() * height_ratio).max(1.0);
        let bar_left_x_px = inner_rect.min.x + (bucket_index as f32) * (bar_width_px + BAR_GAP_PX);
        let bar_rect = Rect::from_min_max(
            pos2(bar_left_x_px, inner_rect.max.y - bar_height_px),
            pos2(bar_left_x_px + bar_width_px, inner_rect.max.y),
        );
        painter.rect_filled(bar_rect, 1.5, CATEGORY_TIMING);
    }

    let span_ms = (gaussian.max_offset_ms - gaussian.min_offset_ms).max(0.001);
    let mean_ratio = ((gaussian.mean_offset_ms - gaussian.min_offset_ms) / span_ms).clamp(0.0, 1.0);
    let mean_x_px = inner_rect.min.x + inner_rect.width() * (mean_ratio as f32);
    painter.line_segment(
        [
            pos2(mean_x_px, inner_rect.min.y),
            pos2(mean_x_px, inner_rect.max.y),
        ],
        Stroke::new(1.5, ACCENT_PRIMARY),
    );

    if let Some(sampled_total_ms) = live_delay_ms {
        let sampled_offset_ms = (sampled_total_ms as f64) - (gaussian.base_delay_ms as f64);
        let sample_ratio = ((sampled_offset_ms - gaussian.min_offset_ms) / span_ms).clamp(0.0, 1.0);
        let sample_x_px = inner_rect.min.x + inner_rect.width() * (sample_ratio as f32);
        painter.circle_filled(
            pos2(sample_x_px, inner_rect.min.y + 4.0),
            3.0,
            STATUS_RUNNING,
        );
    }
}

fn render_recent_delay_stats(ui: &mut Ui, recent_delay_samples_ms: &[u64]) {
    if recent_delay_samples_ms.is_empty() {
        ui.label(
            RichText::new("Live Samples: waiting for script activity")
                .size(FONT_SIZE_SMALL_PX)
                .monospace()
                .color(TEXT_MUTED),
        );
        return;
    }

    let sample_count = recent_delay_samples_ms.len();
    let sum_ms: u128 = recent_delay_samples_ms
        .iter()
        .map(|sample| u128::from(*sample))
        .sum();
    let average_ms = (sum_ms as f64) / (sample_count as f64);
    let min_ms = recent_delay_samples_ms.iter().copied().min().unwrap_or(0);
    let max_ms = recent_delay_samples_ms.iter().copied().max().unwrap_or(0);

    ui.label(
        RichText::new(format!(
            "Samples: {sample_count}  |  Avg: {average_ms:.1} ms  |  Range: {min_ms}..{max_ms} ms"
        ))
        .size(FONT_SIZE_SMALL_PX)
        .monospace()
        .color(TEXT_PRIMARY),
    );
}

fn render_telemetry_log_viewer(
    ui: &mut Ui,
    telemetry_logs: &[TelemetryLogEntry],
    body_height_px: f32,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Runtime & Compiler Logs")
                .size(FONT_SIZE_SMALL_PX)
                .strong()
                .color(TEXT_MUTED),
        );
        ui.label(
            RichText::new(format!("({} entries)", telemetry_logs.len()))
                .size(FONT_SIZE_SMALL_PX)
                .monospace()
                .color(TEXT_MUTED),
        );
    });
    ui.add_space(4.0);

    Frame::NONE
        .fill(SURFACE_CODE)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_salt("telemetry_logs_scroll")
                .max_height((body_height_px - 32.0).max(60.0))
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if telemetry_logs.is_empty() {
                        ui.label(
                            RichText::new("No runtime events recorded yet.")
                                .size(FONT_SIZE_SMALL_PX)
                                .monospace()
                                .color(TEXT_MUTED),
                        );
                        return;
                    }

                    for entry in telemetry_logs {
                        render_single_log_row(ui, entry);
                    }
                });
        });
}

fn render_single_log_row(ui: &mut Ui, entry: &TelemetryLogEntry) {
    let (level_tag, level_color) = match entry.level {
        TelemetryLevel::Info => ("INFO", CATEGORY_ACTION),
        TelemetryLevel::Warning => ("WARN", ACCENT_PRIMARY),
        TelemetryLevel::Error => ("ERR ", STATUS_DANGER),
        TelemetryLevel::Action => ("ACT ", STATUS_RUNNING),
    };

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            RichText::new(format!("#{:04}", entry.sequence_number))
                .size(FONT_SIZE_SMALL_PX)
                .monospace()
                .color(TEXT_MUTED),
        );
        ui.label(
            RichText::new(level_tag)
                .size(FONT_SIZE_SMALL_PX)
                .monospace()
                .strong()
                .color(level_color),
        );
        ui.label(
            RichText::new(&entry.message)
                .size(FONT_SIZE_SMALL_PX)
                .monospace()
                .color(TEXT_PRIMARY),
        );
    });
}

fn find_first_gaussian_block(statements: &[StatementBlock]) -> Option<GaussianBlockPreview> {
    for block in statements {
        match &block.kind {
            StatementKind::WaitRandomGaussian {
                base_delay_ms,
                min_offset_ms,
                max_offset_ms,
                mean_offset_ms,
                std_dev_ms,
            } => {
                return Some(GaussianBlockPreview {
                    base_delay_ms: *base_delay_ms,
                    min_offset_ms: *min_offset_ms,
                    max_offset_ms: *max_offset_ms,
                    mean_offset_ms: *mean_offset_ms,
                    std_dev_ms: *std_dev_ms,
                });
            }
            StatementKind::IfThen { then_blocks, .. } => {
                if let Some(found) = find_first_gaussian_block(then_blocks) {
                    return Some(found);
                }
            }
            StatementKind::IfThenElse {
                then_blocks,
                else_blocks,
                ..
            } => {
                if let Some(found) = find_first_gaussian_block(then_blocks)
                    .or_else(|| find_first_gaussian_block(else_blocks))
                {
                    return Some(found);
                }
            }
            StatementKind::WhileCondition { body_blocks, .. }
            | StatementKind::RepeatCount { body_blocks, .. } => {
                if let Some(found) = find_first_gaussian_block(body_blocks) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}
