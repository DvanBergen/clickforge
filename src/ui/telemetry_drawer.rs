use egui::{
    Align, Button, Color32, CornerRadius, FontId, Frame, Layout, Margin, Painter, Rect, RichText,
    ScrollArea, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

use crate::engine::timing::compute_gaussian_histogram_buckets;
use crate::models::{
    ActiveWindowSnapshot, CoordinatorSnapshot, InputSubsystemStatus, ScriptDocument,
    ScriptRuntimeStatus, StatementBlock, StatementKind, TelemetryLevel, TelemetryLogEntry,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BG_CANVAS, BORDER_STRONG, BORDER_SUBTLE, CATEGORY_TIMING,
    CORNER_RADIUS_PILL_PX, CORNER_RADIUS_SM_PX, FONT_SIZE_SMALL_PX, STATUS_DANGER_INK,
    STATUS_DANGER_TINT, STATUS_INFO_INK, STATUS_RUNNING_INK, STATUS_RUNNING_TINT, SURFACE_CODE,
    SURFACE_ELEVATED, TEXT_MUTED, TEXT_PRIMARY, font_family_semibold, rich_mono, rich_mono_bold,
    rich_semibold, truncate_display_text,
};

const MIN_DRAWER_BODY_HEIGHT_PX: f32 = 138.0;
const HISTOGRAM_WIDTH_RATIO: f32 = 0.36;
const HISTOGRAM_MIN_WIDTH_PX: f32 = 320.0;
const HISTOGRAM_MAX_WIDTH_PX: f32 = 420.0;
const HISTOGRAM_CHROME_HEIGHT_PX: f32 = 62.0;
const HISTOGRAM_AXIS_HEIGHT_PX: f32 = 19.0;
const HISTOGRAM_INSET_PX: f32 = 6.0;
const HISTOGRAM_SAMPLE_COUNT: usize = 2_000;
const HISTOGRAM_BUCKET_COUNT: usize = 20;
const LOGS_HEADER_HEIGHT_PX: f32 = 52.0;
const MAX_FOCUS_CHARS_WIDE: usize = 34;
const MAX_FOCUS_CHARS_NARROW: usize = 22;
const NARROW_HEADER_WIDTH_PX: f32 = 1040.0;
const MAX_COMPOSITOR_CHARS: usize = 14;
const BAR_GAP_PX: f32 = 2.0;
const BADGE_HEIGHT_PX: f32 = 21.0;

/// Cached Gaussian histogram buckets and drawer view preferences (hidden toggle, cleared log cutoff).
#[derive(Debug, Default, Clone)]
pub struct GaussianHistogramCache {
    cached_key: Option<(u64, u64, u64, u64)>,
    buckets: Vec<u32>,
    is_histogram_hidden: bool,
    cleared_log_cutoff_timestamp_ms: u64,
    cleared_log_cutoff_sequence_number: u64,
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

    fn clear_logs_up_to(&mut self, telemetry_logs: &[TelemetryLogEntry]) {
        if let Some(latest_ms) = telemetry_logs
            .iter()
            .map(|entry| entry.timestamp_unix_ms)
            .max()
        {
            self.cleared_log_cutoff_timestamp_ms =
                self.cleared_log_cutoff_timestamp_ms.max(latest_ms);
        }
        if let Some(latest_number) = telemetry_logs
            .iter()
            .map(|entry| entry.sequence_number)
            .max()
        {
            self.cleared_log_cutoff_sequence_number =
                self.cleared_log_cutoff_sequence_number.max(latest_number);
        }
    }

    fn is_log_visible(&self, entry: &TelemetryLogEntry) -> bool {
        (self.cleared_log_cutoff_timestamp_ms == 0 && self.cleared_log_cutoff_sequence_number == 0)
            || entry.sequence_number > self.cleared_log_cutoff_sequence_number
            || entry.timestamp_unix_ms > self.cleared_log_cutoff_timestamp_ms
    }
}

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
    let did_toggle = render_header_strip(ui, is_expanded, active_status, snapshot);
    if !*is_expanded {
        return did_toggle;
    }

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(6.0);

    let body_height_px = drawer_body_height_px.max(MIN_DRAWER_BODY_HEIGHT_PX);
    let total_width_px = ui.available_width();
    let row_size = vec2(total_width_px, body_height_px);
    ui.allocate_ui_with_layout(row_size, Layout::left_to_right(Align::Min), |ui| {
        if !histogram_cache.is_histogram_hidden {
            let column_width_px = (total_width_px * HISTOGRAM_WIDTH_RATIO)
                .clamp(HISTOGRAM_MIN_WIDTH_PX, HISTOGRAM_MAX_WIDTH_PX);
            let column_size = vec2(column_width_px, body_height_px);
            ui.allocate_ui_with_layout(column_size, Layout::top_down(Align::Min), |ui| {
                render_histogram_column(
                    ui,
                    active_script,
                    active_status,
                    &snapshot.recent_delay_samples_ms,
                    histogram_cache,
                    body_height_px,
                );
            });
            ui.separator();
        }
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            render_log_viewer(
                ui,
                &snapshot.telemetry_logs,
                histogram_cache,
                body_height_px,
            );
        });
    });

    did_toggle
}

fn render_header_strip(
    ui: &mut Ui,
    is_expanded: &mut bool,
    active_status: Option<&ScriptRuntimeStatus>,
    snapshot: &CoordinatorSnapshot,
) -> bool {
    let mut did_toggle = false;
    let max_focus_chars = if ui.available_width() < NARROW_HEADER_WIDTH_PX {
        MAX_FOCUS_CHARS_NARROW
    } else {
        MAX_FOCUS_CHARS_WIDE
    };

    ui.horizontal(|ui| {
        let toggle_text = if *is_expanded {
            "▾ Telemetry"
        } else {
            "▸ Telemetry"
        };
        let toggle_tooltip = if *is_expanded {
            "Collapse telemetry and runtime logs"
        } else {
            "Expand telemetry, Gaussian histogram, and runtime logs"
        };
        let toggle_button =
            Button::new(rich_semibold(toggle_text, FONT_SIZE_SMALL_PX, TEXT_PRIMARY))
                .fill(SURFACE_ELEVATED)
                .stroke(Stroke::new(1.0, BORDER_SUBTLE));
        if ui
            .add(toggle_button)
            .on_hover_text(toggle_tooltip)
            .clicked()
        {
            *is_expanded = !*is_expanded;
            did_toggle = true;
        }

        ui.separator();
        render_focus_summary(ui, snapshot.focused_window.as_ref(), max_focus_chars);
        ui.separator();
        render_subsystem_badges(ui, &snapshot.input_status);

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let last_delay = active_status
                .and_then(|status| status.last_sampled_delay_ms)
                .or_else(|| snapshot.recent_delay_samples_ms.last().copied())
                .map(|delay_ms| format!("{delay_ms} ms"))
                .unwrap_or_else(|| "-".to_string());
            let total_actions = active_status
                .map(|status| status.total_actions_emitted)
                .unwrap_or(0);
            let metrics = format!("Actions: {total_actions}  |  Last: {last_delay}");
            let response = ui.label(rich_mono(metrics, FONT_SIZE_SMALL_PX, TEXT_PRIMARY));
            if let Some(tooltip) = format_delay_stats_tooltip(&snapshot.recent_delay_samples_ms) {
                response.on_hover_text(tooltip);
            }
        });
    });

    did_toggle
}

fn render_focus_summary(
    ui: &mut Ui,
    focused_window: Option<&ActiveWindowSnapshot>,
    max_focus_chars: usize,
) {
    let Some(window) = focused_window else {
        ui.label(rich_mono("Focus: None", FONT_SIZE_SMALL_PX, TEXT_MUTED));
        return;
    };
    let full_title = non_empty_or(&window.title, "(untitled)");
    let app_id = non_empty_or(&window.app_id, "none");
    let pid_text = window
        .pid
        .map(|pid| pid.to_string())
        .unwrap_or_else(|| "-".to_string());
    let compositor = non_empty_or(&window.compositor_name, "unknown");
    let short_title = truncate_display_text(full_title, max_focus_chars);
    let tooltip =
        format!("Title: {full_title}\napp_id: {app_id}\nPID: {pid_text}\nCompositor: {compositor}");
    ui.label(rich_mono(
        format!("Focus: {short_title}"),
        FONT_SIZE_SMALL_PX,
        TEXT_MUTED,
    ))
    .on_hover_text(tooltip);
}

fn render_subsystem_badges(ui: &mut Ui, input_status: &InputSubsystemStatus) {
    let uinput_tooltip = if input_status.is_uinput_ready {
        "uinput: Ready (/dev/uinput virtual injection active)".to_string()
    } else {
        let detail = input_status
            .uinput_error
            .as_deref()
            .unwrap_or("Virtual input unavailable");
        format!("uinput: Offline\n{detail}")
    };
    render_ready_pill(ui, "uinput", input_status.is_uinput_ready, &uinput_tooltip);

    let evdev_tooltip = if input_status.is_evdev_ready {
        format!(
            "evdev: Ready ({} monitored input device(s))",
            input_status.monitored_device_count
        )
    } else {
        let detail = input_status
            .evdev_error
            .as_deref()
            .unwrap_or("Input monitor unavailable");
        format!("evdev: Offline\n{detail}")
    };
    render_ready_pill(ui, "evdev", input_status.is_evdev_ready, &evdev_tooltip);

    if input_status.is_trigger_intercept_active {
        let grab_tooltip =
            "Trigger Interception (EVIOCGRAB) is actively suppressing trigger passthrough";
        paint_status_pill(
            ui,
            "GRAB",
            ACCENT_PRIMARY_TINT,
            ACCENT_PRIMARY,
            true,
            grab_tooltip,
        );
    }
    let compositor = input_status.compositor_name.trim();
    if !compositor.is_empty() {
        let short_name = truncate_display_text(compositor, MAX_COMPOSITOR_CHARS);
        let compositor_tooltip = format!("Compositor watcher: {compositor}");
        paint_status_pill(
            ui,
            &short_name,
            SURFACE_ELEVATED,
            TEXT_MUTED,
            false,
            &compositor_tooltip,
        );
    }
}

fn render_ready_pill(ui: &mut Ui, label: &str, is_ready: bool, tooltip: &str) {
    let (fill, ink) = if is_ready {
        (STATUS_RUNNING_TINT, STATUS_RUNNING_INK)
    } else {
        (STATUS_DANGER_TINT, STATUS_DANGER_INK)
    };
    paint_status_pill(ui, label, fill, ink, true, tooltip);
}

fn paint_status_pill(
    ui: &mut Ui,
    label_text: &str,
    fill: Color32,
    ink: Color32,
    has_dot: bool,
    tooltip: &str,
) {
    let font = FontId::new(FONT_SIZE_SMALL_PX - 0.5, font_family_semibold());
    let galley = ui
        .painter()
        .layout_no_wrap(label_text.to_string(), font, ink);
    let dot_span_px = if has_dot { 11.0 } else { 0.0 };
    let size = vec2(14.0 + dot_span_px + galley.size().x, BADGE_HEIGHT_PX);
    let (pill_rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let border = if has_dot { ink } else { BORDER_SUBTLE };
    let radius = CornerRadius::same(CORNER_RADIUS_PILL_PX);
    let painter = ui.painter();
    painter.rect(
        pill_rect,
        radius,
        fill,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );

    let mut content_x_px = pill_rect.min.x + 7.0;
    let center_y_px = pill_rect.center().y;
    if has_dot {
        painter.circle_filled(pos2(content_x_px + 3.0, center_y_px), 3.0, ink);
        content_x_px += dot_span_px;
    }
    painter.galley(
        pos2(content_x_px, center_y_px - galley.size().y * 0.5),
        galley,
        ink,
    );
    response.on_hover_text(tooltip);
}

fn render_histogram_column(
    ui: &mut Ui,
    active_script: Option<&ScriptDocument>,
    active_status: Option<&ScriptRuntimeStatus>,
    recent_delay_samples_ms: &[u64],
    histogram_cache: &mut GaussianHistogramCache,
    body_height_px: f32,
) {
    let preview_block = active_script
        .and_then(|script| find_first_gaussian_block(&script.visual_program.statements));

    ui.horizontal(|ui| {
        ui.label(rich_semibold(
            "Gaussian Delay",
            FONT_SIZE_SMALL_PX,
            TEXT_PRIMARY,
        ));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let hide =
                render_small_button(ui, "Hide Histogram", true, Color32::TRANSPARENT, TEXT_MUTED);
            if hide.clicked() {
                histogram_cache.is_histogram_hidden = true;
            }
        });
    });

    ui.add_space(4.0);
    let plot_height_px = (body_height_px - HISTOGRAM_CHROME_HEIGHT_PX).clamp(64.0, 98.0);
    if let Some(gaussian) = preview_block {
        let buckets = histogram_cache.buckets_for(
            gaussian.min_offset_ms,
            gaussian.max_offset_ms,
            gaussian.mean_offset_ms,
            gaussian.std_dev_ms,
        );
        let live_delay_ms = active_status.and_then(|status| status.last_sampled_delay_ms);
        paint_histogram_plot(ui, buckets, gaussian, live_delay_ms, plot_height_px);
    } else {
        Frame::NONE
            .fill(SURFACE_CODE)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.set_min_size(vec2(
                    ui.available_width(),
                    (plot_height_px - 20.0).max(36.0),
                ));
                let empty = "Add a Wait Gaussian block to preview its timing distribution.";
                ui.label(
                    RichText::new(empty)
                        .size(FONT_SIZE_SMALL_PX)
                        .color(TEXT_MUTED),
                );
            });
    }

    ui.add_space(4.0);
    render_delay_summary_line(ui, recent_delay_samples_ms, preview_block);
}

fn paint_histogram_plot(
    ui: &mut Ui,
    buckets: &[u32],
    gaussian: GaussianBlockPreview,
    live_delay_ms: Option<u64>,
    plot_height_px: f32,
) {
    let plot_size = vec2(ui.available_width().max(200.0), plot_height_px);
    let (plot_rect, response) = ui.allocate_exact_size(plot_size, Sense::hover());
    let painter = ui
        .painter()
        .with_clip_rect(plot_rect.intersect(ui.clip_rect()));
    let radius = CornerRadius::same(CORNER_RADIUS_SM_PX);
    painter.rect(
        plot_rect,
        radius,
        BG_CANVAS,
        Stroke::new(1.0, BORDER_SUBTLE),
        StrokeKind::Inside,
    );

    let bar_bottom_y_px = (plot_rect.max.y - HISTOGRAM_AXIS_HEIGHT_PX).max(plot_rect.min.y + 18.0);
    let bars_rect = Rect::from_min_max(
        pos2(
            plot_rect.min.x + HISTOGRAM_INSET_PX,
            plot_rect.min.y + HISTOGRAM_INSET_PX,
        ),
        pos2(plot_rect.max.x - HISTOGRAM_INSET_PX, bar_bottom_y_px),
    );
    let baseline = [
        pos2(bars_rect.min.x, bar_bottom_y_px),
        pos2(bars_rect.max.x, bar_bottom_y_px),
    ];
    painter.line_segment(baseline, Stroke::new(1.0, BORDER_STRONG));

    if !buckets.is_empty() {
        let max_count = buckets.iter().copied().max().unwrap_or(1).max(1) as f32;
        let count_f32 = buckets.len() as f32;
        let total_gaps_px = BAR_GAP_PX * (count_f32 - 1.0).max(0.0);
        let bar_width_px = ((bars_rect.width() - total_gaps_px) / count_f32).max(2.0);
        let max_height_px = (bars_rect.height() - 2.0).max(4.0);
        let bar_radius = CornerRadius {
            nw: 2,
            ne: 2,
            sw: 0,
            se: 0,
        };
        for (bucket_index, bucket_count) in buckets.iter().enumerate() {
            let height_px = (max_height_px * (*bucket_count as f32 / max_count)).max(1.0);
            let left_x_px = bars_rect.min.x + (bucket_index as f32) * (bar_width_px + BAR_GAP_PX);
            let min_point = pos2(left_x_px, bar_bottom_y_px - height_px);
            let max_point = pos2(left_x_px + bar_width_px, bar_bottom_y_px);
            painter.rect_filled(
                Rect::from_min_max(min_point, max_point),
                bar_radius,
                CATEGORY_TIMING,
            );
        }
    }

    let span_ms = (gaussian.max_offset_ms - gaussian.min_offset_ms).max(0.001);
    let mean_ratio = ((gaussian.mean_offset_ms - gaussian.min_offset_ms) / span_ms).clamp(0.0, 1.0);
    let mean_x_px = bars_rect.min.x + bars_rect.width() * (mean_ratio as f32);
    let mean_line = [
        pos2(mean_x_px, bars_rect.min.y),
        pos2(mean_x_px, bar_bottom_y_px),
    ];
    painter.line_segment(mean_line, Stroke::new(1.5, ACCENT_PRIMARY));

    if let Some(sampled_total_ms) = live_delay_ms {
        let offset_ms = (sampled_total_ms as f64) - (gaussian.base_delay_ms as f64);
        let ratio = ((offset_ms - gaussian.min_offset_ms) / span_ms).clamp(0.0, 1.0);
        let sample_x_px = bars_rect.min.x + bars_rect.width() * (ratio as f32);
        painter.circle_filled(
            pos2(sample_x_px, bars_rect.min.y + 4.0),
            3.5,
            STATUS_RUNNING_INK,
        );
    }

    paint_x_axis_ticks(&painter, plot_rect, bars_rect, gaussian, mean_x_px);
    let base_ms = gaussian.base_delay_ms as f64;
    let tooltip = format!(
        "Base: {} ms\nOffset: {:.1} .. {:.1} ms\nMean: {:.1} ms (total {:.1} ms)\nStd Dev: {:.1} ms\nTotal: {:.1} .. {:.1} ms",
        gaussian.base_delay_ms,
        gaussian.min_offset_ms,
        gaussian.max_offset_ms,
        gaussian.mean_offset_ms,
        base_ms + gaussian.mean_offset_ms,
        gaussian.std_dev_ms,
        base_ms + gaussian.min_offset_ms,
        base_ms + gaussian.max_offset_ms,
    );
    response.on_hover_text(tooltip);
}

fn paint_x_axis_ticks(
    painter: &Painter,
    plot_rect: Rect,
    bars_rect: Rect,
    gaussian: GaussianBlockPreview,
    mean_x_px: f32,
) {
    let font = FontId::monospace((FONT_SIZE_SMALL_PX - 1.5).max(9.5));
    let axis_y_px = (bars_rect.max.y + plot_rect.max.y) * 0.5;
    let base_ms = gaussian.base_delay_ms as f64;
    let layout_tick = |prefix: &str, value_ms: f64, color: Color32| {
        let label = format!("{prefix} {}", format_compact_ms(value_ms));
        painter.layout_no_wrap(label, font.clone(), color)
    };
    let min_galley = layout_tick("min", base_ms + gaussian.min_offset_ms, TEXT_MUTED);
    let max_galley = layout_tick("max", base_ms + gaussian.max_offset_ms, TEXT_MUTED);
    let mean_galley = layout_tick("mean", base_ms + gaussian.mean_offset_ms, ACCENT_PRIMARY);

    let tick_y_px = axis_y_px - min_galley.size().y * 0.5;
    let min_width_px = min_galley.size().x;
    let max_width_px = max_galley.size().x;
    let mean_width_px = mean_galley.size().x;
    painter.galley(pos2(bars_rect.min.x, tick_y_px), min_galley, TEXT_MUTED);
    painter.galley(
        pos2(bars_rect.max.x - max_width_px, tick_y_px),
        max_galley,
        TEXT_MUTED,
    );

    let min_allowed_x_px = bars_rect.min.x + min_width_px + 8.0;
    let max_allowed_x_px = bars_rect.max.x - max_width_px - 8.0 - mean_width_px;
    if max_allowed_x_px >= min_allowed_x_px {
        let clamped_x_px =
            (mean_x_px - mean_width_px * 0.5).clamp(min_allowed_x_px, max_allowed_x_px);
        painter.galley(pos2(clamped_x_px, tick_y_px), mean_galley, ACCENT_PRIMARY);
    }
}

fn format_compact_ms(milliseconds: f64) -> String {
    if (milliseconds.round() - milliseconds).abs() < 0.05 {
        format!("{:.0}ms", milliseconds)
    } else {
        format!("{:.1}ms", milliseconds)
    }
}

fn render_delay_summary_line(
    ui: &mut Ui,
    recent_delay_samples_ms: &[u64],
    preview_block: Option<GaussianBlockPreview>,
) {
    if let Some((count, average_ms, min_ms, max_ms)) =
        compute_delay_sample_stats(recent_delay_samples_ms)
    {
        let summary = format!("n={count}  avg {average_ms:.1} ms  [{min_ms}..{max_ms} ms]");
        let response = ui.label(rich_mono(summary, FONT_SIZE_SMALL_PX, TEXT_PRIMARY));
        if let Some(tooltip) = format_delay_stats_tooltip(recent_delay_samples_ms) {
            response.on_hover_text(tooltip);
        }
        return;
    }
    let idle_text = match preview_block {
        Some(gaussian) => format!(
            "Base {} ms | sd {:.1} ms | Waiting",
            gaussian.base_delay_ms, gaussian.std_dev_ms
        ),
        None => "Waiting for live script samples".to_string(),
    };
    ui.label(rich_mono(idle_text, FONT_SIZE_SMALL_PX, TEXT_MUTED))
        .on_hover_text("Live timing samples appear once a script executes");
}

fn compute_delay_sample_stats(samples_ms: &[u64]) -> Option<(usize, f64, u64, u64)> {
    if samples_ms.is_empty() {
        return None;
    }
    let sample_count = samples_ms.len();
    let sum_ms: u128 = samples_ms
        .iter()
        .map(|sample_ms| u128::from(*sample_ms))
        .sum();
    let average_ms = (sum_ms as f64) / (sample_count as f64);
    let min_ms = samples_ms.iter().copied().min().unwrap_or(0);
    let max_ms = samples_ms.iter().copied().max().unwrap_or(0);
    Some((sample_count, average_ms, min_ms, max_ms))
}

fn format_delay_stats_tooltip(samples_ms: &[u64]) -> Option<String> {
    let (count, average_ms, min_ms, max_ms) = compute_delay_sample_stats(samples_ms)?;
    Some(format!(
        "Recent Samples: {count}\nAverage Delay: {average_ms:.1} ms\nObserved Range: {min_ms} .. {max_ms} ms"
    ))
}

fn render_small_button(
    ui: &mut Ui,
    label: &str,
    is_enabled: bool,
    fill: Color32,
    ink: Color32,
) -> egui::Response {
    let text = RichText::new(label)
        .size(FONT_SIZE_SMALL_PX - 0.5)
        .color(ink);
    ui.add_enabled(
        is_enabled,
        Button::new(text)
            .fill(fill)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
    )
}

fn render_log_viewer(
    ui: &mut Ui,
    telemetry_logs: &[TelemetryLogEntry],
    histogram_cache: &mut GaussianHistogramCache,
    body_height_px: f32,
) {
    let visible_entries: Vec<&TelemetryLogEntry> = telemetry_logs
        .iter()
        .filter(|entry| histogram_cache.is_log_visible(entry))
        .collect();
    let visible_count = visible_entries.len();

    ui.horizontal(|ui| {
        ui.label(rich_semibold(
            "Runtime & Compiler Logs",
            FONT_SIZE_SMALL_PX,
            TEXT_PRIMARY,
        ));
        ui.label(rich_mono(
            format!("({visible_count})"),
            FONT_SIZE_SMALL_PX,
            TEXT_MUTED,
        ));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let clear_ink = if visible_count > 0 {
                TEXT_PRIMARY
            } else {
                TEXT_MUTED
            };
            let clear = render_small_button(
                ui,
                "Clear",
                visible_count > 0,
                Color32::TRANSPARENT,
                clear_ink,
            );
            if clear
                .on_hover_text("Hide current runtime logs until newer events arrive")
                .clicked()
            {
                histogram_cache.clear_logs_up_to(telemetry_logs);
            }
            if histogram_cache.is_histogram_hidden {
                let show =
                    render_small_button(ui, "Show Histogram", true, SURFACE_ELEVATED, TEXT_PRIMARY);
                if show.clicked() {
                    histogram_cache.is_histogram_hidden = false;
                }
            }
        });
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
                .max_height((body_height_px - LOGS_HEADER_HEIGHT_PX).max(60.0))
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if visible_entries.is_empty() {
                        let empty = if telemetry_logs.is_empty() {
                            "No runtime events recorded yet."
                        } else {
                            "Logs cleared. Newer runtime events will appear here."
                        };
                        ui.label(rich_mono(empty, FONT_SIZE_SMALL_PX, TEXT_MUTED));
                        return;
                    }
                    for entry in visible_entries {
                        render_single_log_row(ui, entry);
                    }
                });
        });
}

fn render_single_log_row(ui: &mut Ui, entry: &TelemetryLogEntry) {
    let (level_tag, level_color) = match entry.level {
        TelemetryLevel::Info => ("INFO", STATUS_INFO_INK),
        TelemetryLevel::Warning => ("WARN", ACCENT_PRIMARY),
        TelemetryLevel::Error => ("ERR ", STATUS_DANGER_INK),
        TelemetryLevel::Action => ("ACT ", STATUS_RUNNING_INK),
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let sequence_text = format!("#{:04}", entry.sequence_number);
        ui.label(rich_mono(sequence_text, FONT_SIZE_SMALL_PX, TEXT_MUTED));
        ui.label(rich_mono_bold(level_tag, FONT_SIZE_SMALL_PX, level_color));
        ui.label(rich_mono(&entry.message, FONT_SIZE_SMALL_PX, TEXT_PRIMARY));
    });
}

fn find_first_gaussian_block(statements: &[StatementBlock]) -> Option<GaussianBlockPreview> {
    for block in statements {
        let found = match &block.kind {
            StatementKind::WaitRandomGaussian {
                base_delay_ms,
                min_offset_ms,
                max_offset_ms,
                mean_offset_ms,
                std_dev_ms,
            } => Some(GaussianBlockPreview {
                base_delay_ms: *base_delay_ms,
                min_offset_ms: *min_offset_ms,
                max_offset_ms: *max_offset_ms,
                mean_offset_ms: *mean_offset_ms,
                std_dev_ms: *std_dev_ms,
            }),
            StatementKind::IfThen { then_blocks, .. } => find_first_gaussian_block(then_blocks),
            StatementKind::IfThenElse {
                then_blocks,
                else_blocks,
                ..
            } => find_first_gaussian_block(then_blocks)
                .or_else(|| find_first_gaussian_block(else_blocks)),
            StatementKind::WhileCondition { body_blocks, .. }
            | StatementKind::RepeatCount { body_blocks, .. } => {
                find_first_gaussian_block(body_blocks)
            }
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

fn non_empty_or<'text>(raw_text: &'text str, fallback: &'text str) -> &'text str {
    if raw_text.trim().is_empty() {
        fallback
    } else {
        raw_text
    }
}
