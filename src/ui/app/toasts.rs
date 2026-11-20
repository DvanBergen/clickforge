use std::time::{Duration, Instant};

use egui::{Align2, Area, Button, Color32, CornerRadius, Frame, Id, Margin, Order, Stroke, vec2};

use crate::models::ScriptDocument;
use crate::ui::app::ClickForgeWorkbenchApp;
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BORDER_STRONG, CORNER_RADIUS_MD_PX, CORNER_RADIUS_SM_PX,
    FONT_SIZE_SMALL_PX, STATUS_DANGER, STATUS_DANGER_INK, STATUS_DANGER_TINT, STATUS_RUNNING,
    STATUS_RUNNING_INK, STATUS_RUNNING_TINT, SURFACE_PANEL, TEXT_MUTED, TEXT_PRIMARY,
    rich_semibold,
};

const STATUS_TOAST_DURATION_MS: u64 = 4_000;
const TOAST_CLEARANCE_ABOVE_DRAWER_PX: f32 = 14.0;
const TOAST_RIGHT_MARGIN_PX: f32 = 16.0;

pub(crate) struct StatusToast {
    pub(crate) message: String,
    pub(crate) is_error: bool,
    pub(crate) expires_at: Instant,
}

pub(crate) struct DeletedScriptUndoEntry {
    pub(crate) script: ScriptDocument,
    pub(crate) original_index: usize,
    pub(crate) expires_at: Instant,
}

impl ClickForgeWorkbenchApp {
    pub(crate) fn show_toast(&mut self, message: impl Into<String>, is_error: bool) {
        self.status_toast = Some(StatusToast {
            message: message.into(),
            is_error,
            expires_at: Instant::now() + Duration::from_millis(STATUS_TOAST_DURATION_MS),
        });
    }

    pub(super) fn render_toasts(
        &mut self,
        egui_context: &egui::Context,
        rendered_drawer_height_px: f32,
    ) {
        let toast_offset_y_px = -(rendered_drawer_height_px + TOAST_CLEARANCE_ABOVE_DRAWER_PX);
        if self.render_deleted_script_undo_toast(egui_context, toast_offset_y_px) {
            return;
        }
        self.render_status_toast(egui_context, toast_offset_y_px);
    }

    fn render_deleted_script_undo_toast(
        &mut self,
        egui_context: &egui::Context,
        toast_offset_y_px: f32,
    ) -> bool {
        let Some(entry) = &self.last_deleted_script else {
            return false;
        };
        let now = Instant::now();
        if now >= entry.expires_at {
            self.last_deleted_script = None;
            return false;
        }

        let remaining_seconds = entry
            .expires_at
            .saturating_duration_since(now)
            .as_secs()
            .saturating_add(1);
        let script_name = entry.script.name.clone();
        let mut should_restore = false;
        let mut should_dismiss = false;

        Area::new(Id::unique("workbench_undo_delete_toast"))
            .order(Order::Foreground)
            .anchor(
                Align2::RIGHT_BOTTOM,
                vec2(-TOAST_RIGHT_MARGIN_PX, toast_offset_y_px),
            )
            .interactable(true)
            .show(egui_context, |ui| {
                Frame::NONE
                    .fill(SURFACE_PANEL)
                    .stroke(Stroke::new(1.0, BORDER_STRONG))
                    .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
                    .shadow(ui.style().visuals.popup_shadow)
                    .inner_margin(Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.label(rich_semibold(
                                format!("Deleted '{script_name}' ({remaining_seconds}s)"),
                                FONT_SIZE_SMALL_PX,
                                TEXT_PRIMARY,
                            ));
                            let undo_button = Button::new(rich_semibold(
                                "Undo",
                                FONT_SIZE_SMALL_PX,
                                ACCENT_PRIMARY,
                            ))
                            .fill(ACCENT_PRIMARY_TINT)
                            .stroke(Stroke::new(1.0, ACCENT_PRIMARY))
                            .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
                            .min_size(vec2(0.0, 22.0));
                            if ui.add(undo_button).clicked() {
                                should_restore = true;
                            }
                            let dismiss_button =
                                Button::new(rich_semibold("×", FONT_SIZE_SMALL_PX, TEXT_MUTED))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::NONE)
                                    .min_size(vec2(20.0, 22.0));
                            if ui.add(dismiss_button).clicked() {
                                should_dismiss = true;
                            }
                        });
                    });
            });

        if should_restore {
            self.restore_last_deleted_script();
        } else if should_dismiss {
            self.last_deleted_script = None;
        }
        true
    }

    fn render_status_toast(&mut self, egui_context: &egui::Context, toast_offset_y_px: f32) {
        let Some(toast) = &self.status_toast else {
            return;
        };
        if Instant::now() >= toast.expires_at {
            self.status_toast = None;
            return;
        }

        let (fill_color, border_color, text_color) = if toast.is_error {
            (STATUS_DANGER_TINT, STATUS_DANGER, STATUS_DANGER_INK)
        } else {
            (STATUS_RUNNING_TINT, STATUS_RUNNING, STATUS_RUNNING_INK)
        };

        Area::new(Id::unique("workbench_status_toast"))
            .order(Order::Foreground)
            .anchor(
                Align2::RIGHT_BOTTOM,
                vec2(-TOAST_RIGHT_MARGIN_PX, toast_offset_y_px),
            )
            .interactable(false)
            .show(egui_context, |ui| {
                Frame::NONE
                    .fill(fill_color)
                    .stroke(Stroke::new(1.0, border_color))
                    .corner_radius(CornerRadius::same(CORNER_RADIUS_MD_PX))
                    .shadow(ui.style().visuals.popup_shadow)
                    .inner_margin(Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.label(rich_semibold(
                            &toast.message,
                            FONT_SIZE_SMALL_PX,
                            text_color,
                        ));
                    });
            });
    }
}
