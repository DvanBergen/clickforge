use std::sync::mpsc::SyncSender;

use ksni::blocking::{Handle as KsniBlockingHandle, TrayMethods};
use ksni::menu::StandardItem;
use ksni::{Category, Icon, MenuItem, OfflineReason, Status, ToolTip, Tray};
use tracing::{debug, warn};

use crate::models::runtime_contracts::{ScriptExecutionPhase, TrayRuntimeState, TrayUiEvent};

const TRAY_ITEM_ID: &str = "clickforge";
const TRAY_TITLE: &str = "ClickForge";
const TRAY_ICON_SIZE_PX: i32 = 24;
const BYTES_PER_ARGB_PIXEL: usize = 4;

// Forge Slate tray palette matching the desktop workbench design system.
const COLOR_BG_SURFACE: [u8; 4] = [0xFF, 0x18, 0x1B, 0x21];
const COLOR_BORDER: [u8; 4] = [0xFF, 0x2A, 0x30, 0x3C];
const COLOR_GLYPH_PRIMARY: [u8; 4] = [0xFF, 0xEC, 0xEF, 0xF4];
const COLOR_DOT_OUTLINE: [u8; 4] = [0xFF, 0x11, 0x13, 0x17];
const COLOR_STATUS_MUTED: [u8; 4] = [0xFF, 0x94, 0xA0, 0xB2];
const COLOR_STATUS_AMBER: [u8; 4] = [0xFF, 0xE5, 0x9A, 0x38];
const COLOR_STATUS_EMERALD: [u8; 4] = [0xFF, 0x2E, 0xA0, 0x43];
const COLOR_STATUS_CRIMSON: [u8; 4] = [0xFF, 0xE0, 0x4F, 0x5F];
const COLOR_TRANSPARENT: [u8; 4] = [0x00, 0x00, 0x00, 0x00];

/// Internal `ksni::Tray` implementation for ClickForge's Linux StatusNotifierItem.
pub struct ClickForgeTrayService {
    runtime_state: TrayRuntimeState,
    tray_event_sender: SyncSender<TrayUiEvent>,
}

impl ClickForgeTrayService {
    fn new(tray_event_sender: SyncSender<TrayUiEvent>) -> Self {
        Self {
            runtime_state: TrayRuntimeState::default(),
            tray_event_sender,
        }
    }

    fn emit_ui_event(&self, ui_event: TrayUiEvent) {
        let _ = self.tray_event_sender.try_send(ui_event);
    }

    fn status_summary_label(&self) -> String {
        let phase_label = self.runtime_state.overall_phase.display_label();
        format!(
            "Status: {phase_label} ({} enabled, {} active)",
            self.runtime_state.enabled_script_count, self.runtime_state.active_script_count
        )
    }

    fn tooltip_description(&self) -> String {
        if !self.runtime_state.status_tooltip.trim().is_empty() {
            return self.runtime_state.status_tooltip.clone();
        }
        let mut summary = self.status_summary_label();
        if let Some(focused_title) = &self.runtime_state.focused_window_title
            && !focused_title.trim().is_empty()
        {
            summary.push_str(" | Focus: ");
            summary.push_str(focused_title.trim());
        }
        summary
    }
}

impl Tray for ClickForgeTrayService {
    fn id(&self) -> String {
        TRAY_ITEM_ID.to_string()
    }

    fn title(&self) -> String {
        TRAY_TITLE.to_string()
    }

    fn category(&self) -> Category {
        Category::ApplicationStatus
    }

    fn status(&self) -> Status {
        match self.runtime_state.overall_phase {
            ScriptExecutionPhase::StoppedByKillSwitch | ScriptExecutionPhase::Error => {
                Status::NeedsAttention
            }
            _ => Status::Active,
        }
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![build_status_tray_icon(self.runtime_state.overall_phase)]
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            icon_name: String::new(),
            icon_pixmap: vec![build_status_tray_icon(self.runtime_state.overall_phase)],
            title: TRAY_TITLE.to_string(),
            description: self.tooltip_description(),
        }
    }

    fn activate(&mut self, _coord_x_px: i32, _coord_y_px: i32) {
        self.emit_ui_event(TrayUiEvent::ToggleMainWindow);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::Standard(StandardItem {
                label: self.status_summary_label(),
                enabled: false,
                ..StandardItem::default()
            }),
            MenuItem::Separator,
            MenuItem::Standard(StandardItem {
                label: "Show ClickForge".to_string(),
                activate: Box::new(|tray_service: &mut Self| {
                    tray_service.emit_ui_event(TrayUiEvent::ShowMainWindow);
                }),
                ..StandardItem::default()
            }),
            MenuItem::Standard(StandardItem {
                label: "Emergency Stop All".to_string(),
                activate: Box::new(|tray_service: &mut Self| {
                    tray_service.emit_ui_event(TrayUiEvent::EmergencyStopAll);
                }),
                ..StandardItem::default()
            }),
            MenuItem::Standard(StandardItem {
                label: "Disable All Scripts".to_string(),
                activate: Box::new(|tray_service: &mut Self| {
                    tray_service.emit_ui_event(TrayUiEvent::DisableAllScripts);
                }),
                ..StandardItem::default()
            }),
            MenuItem::Separator,
            MenuItem::Standard(StandardItem {
                label: "Quit".to_string(),
                activate: Box::new(|tray_service: &mut Self| {
                    tray_service.emit_ui_event(TrayUiEvent::QuitApplication);
                }),
                ..StandardItem::default()
            }),
        ]
    }

    fn watcher_offline(&self, reason: OfflineReason) -> bool {
        debug!(
            "StatusNotifierWatcher went offline ({reason:?}); keeping service armed for reconnect"
        );
        true
    }
}

/// Thread-safe handle to the running Linux `StatusNotifierItem` tray service.
pub struct SystemTrayHandle {
    ksni_handle: KsniBlockingHandle<ClickForgeTrayService>,
}

impl SystemTrayHandle {
    /// Pushes updated script runtime status, tooltip, and icon badge to the system tray.
    pub fn update_state(&self, tray_state: TrayRuntimeState) {
        let _ = self.ksni_handle.update(move |tray_service| {
            tray_service.runtime_state = tray_state;
        });
    }

    /// Cleanly unregisters and terminates the background system tray D-Bus service.
    pub fn shutdown(&self) {
        if self.ksni_handle.is_closed() {
            return;
        }
        self.ksni_handle.shutdown().wait();
    }
}

/// Spawns the Linux `ksni` tray icon, returning `None` cleanly if no StatusNotifierWatcher exists.
pub fn spawn_system_tray(tray_event_sender: SyncSender<TrayUiEvent>) -> Option<SystemTrayHandle> {
    let tray_service = ClickForgeTrayService::new(tray_event_sender);
    match tray_service.spawn() {
        Ok(ksni_handle) => Some(SystemTrayHandle { ksni_handle }),
        Err(spawn_error) => {
            warn!(
                "System tray unavailable (StatusNotifierWatcher not running or D-Bus unreachable): {spawn_error}"
            );
            None
        }
    }
}

/// Generates a crisp 24x24 network-byte-order ARGB32 tray icon matching the current script phase.
pub fn build_status_tray_icon(execution_phase: ScriptExecutionPhase) -> Icon {
    let icon_side_px = TRAY_ICON_SIZE_PX as usize;
    let total_bytes = icon_side_px
        .saturating_mul(icon_side_px)
        .saturating_mul(BYTES_PER_ARGB_PIXEL);
    let mut argb_pixels = vec![0u8; total_bytes];
    let status_dot_color = status_dot_argb_for_phase(execution_phase);

    for pixel_y_px in 0..TRAY_ICON_SIZE_PX {
        for pixel_x_px in 0..TRAY_ICON_SIZE_PX {
            let pixel_color = compute_icon_pixel_argb(pixel_x_px, pixel_y_px, status_dot_color);
            write_argb_pixel(&mut argb_pixels, pixel_x_px, pixel_y_px, pixel_color);
        }
    }

    Icon {
        width: TRAY_ICON_SIZE_PX,
        height: TRAY_ICON_SIZE_PX,
        data: argb_pixels,
    }
}

fn status_dot_argb_for_phase(execution_phase: ScriptExecutionPhase) -> [u8; 4] {
    match execution_phase {
        ScriptExecutionPhase::Disabled => COLOR_STATUS_MUTED,
        ScriptExecutionPhase::WaitingForFocus
        | ScriptExecutionPhase::Armed
        | ScriptExecutionPhase::Compiling => COLOR_STATUS_AMBER,
        ScriptExecutionPhase::Running => COLOR_STATUS_EMERALD,
        ScriptExecutionPhase::StoppedByKillSwitch | ScriptExecutionPhase::Error => {
            COLOR_STATUS_CRIMSON
        }
    }
}

fn compute_icon_pixel_argb(pixel_x_px: i32, pixel_y_px: i32, status_dot_color: [u8; 4]) -> [u8; 4] {
    // Bottom-right circular status indicator dot centered at (18, 18).
    let delta_dot_x = pixel_x_px.saturating_sub(18);
    let delta_dot_y = pixel_y_px.saturating_sub(18);
    let dot_distance_sq = delta_dot_x
        .saturating_mul(delta_dot_x)
        .saturating_add(delta_dot_y.saturating_mul(delta_dot_y));
    if dot_distance_sq <= 10 {
        return status_dot_color;
    }
    if dot_distance_sq <= 18 {
        return COLOR_DOT_OUTLINE;
    }

    if !is_inside_rounded_badge(pixel_x_px, pixel_y_px) {
        return COLOR_TRANSPARENT;
    }
    if is_on_badge_border(pixel_x_px, pixel_y_px) {
        return COLOR_BORDER;
    }
    if is_on_forge_glyph(pixel_x_px, pixel_y_px) {
        return COLOR_GLYPH_PRIMARY;
    }
    COLOR_BG_SURFACE
}

fn is_inside_rounded_badge(pixel_x_px: i32, pixel_y_px: i32) -> bool {
    let min_edge_px = 1;
    let max_edge_px = TRAY_ICON_SIZE_PX.saturating_sub(2);
    if pixel_x_px < min_edge_px
        || pixel_x_px > max_edge_px
        || pixel_y_px < min_edge_px
        || pixel_y_px > max_edge_px
    {
        return false;
    }
    let is_outer_corner = (pixel_x_px == min_edge_px || pixel_x_px == max_edge_px)
        && (pixel_y_px == min_edge_px || pixel_y_px == max_edge_px);
    !is_outer_corner
}

fn is_on_badge_border(pixel_x_px: i32, pixel_y_px: i32) -> bool {
    let min_edge_px = 1;
    let max_edge_px = TRAY_ICON_SIZE_PX.saturating_sub(2);
    pixel_x_px == min_edge_px
        || pixel_x_px == max_edge_px
        || pixel_y_px == min_edge_px
        || pixel_y_px == max_edge_px
}

fn is_on_forge_glyph(pixel_x_px: i32, pixel_y_px: i32) -> bool {
    // Crisp "CF" geometric forge emblem: left C-bracket + top/mid bar on the right.
    let is_vertical_stem = (pixel_x_px == 6 || pixel_x_px == 7) && (6..=17).contains(&pixel_y_px);
    let is_top_bar = (6..=14).contains(&pixel_x_px) && (pixel_y_px == 6 || pixel_y_px == 7);
    let is_bottom_bar = (6..=12).contains(&pixel_x_px) && (pixel_y_px == 16 || pixel_y_px == 17);
    let is_inner_spark = (10..=14).contains(&pixel_x_px) && (pixel_y_px == 11 || pixel_y_px == 12);
    is_vertical_stem || is_top_bar || is_bottom_bar || is_inner_spark
}

fn write_argb_pixel(argb_pixels: &mut [u8], pixel_x_px: i32, pixel_y_px: i32, argb_color: [u8; 4]) {
    let Ok(x_usize) = usize::try_from(pixel_x_px) else {
        return;
    };
    let Ok(y_usize) = usize::try_from(pixel_y_px) else {
        return;
    };
    let stride_px = TRAY_ICON_SIZE_PX as usize;
    let pixel_offset_bytes = y_usize
        .saturating_mul(stride_px)
        .saturating_add(x_usize)
        .saturating_mul(BYTES_PER_ARGB_PIXEL);
    let pixel_end_offset_bytes = pixel_offset_bytes.saturating_add(BYTES_PER_ARGB_PIXEL);
    if let Some(pixel_slice) = argb_pixels.get_mut(pixel_offset_bytes..pixel_end_offset_bytes) {
        pixel_slice.copy_from_slice(&argb_color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_valid_24x24_argb32_icons_with_distinct_phase_colors() {
        let idle_icon = build_status_tray_icon(ScriptExecutionPhase::Disabled);
        let armed_icon = build_status_tray_icon(ScriptExecutionPhase::Armed);
        let running_icon = build_status_tray_icon(ScriptExecutionPhase::Running);
        let stopped_icon = build_status_tray_icon(ScriptExecutionPhase::StoppedByKillSwitch);

        let expected_byte_count = (24 * 24 * 4) as usize;
        for icon in [&idle_icon, &armed_icon, &running_icon, &stopped_icon] {
            assert_eq!(icon.width, 24);
            assert_eq!(icon.height, 24);
            assert_eq!(icon.data.len(), expected_byte_count);
        }

        // Center pixel of the status indicator dot at (18, 18).
        let dot_center_offset = ((18 * 24) + 18) * 4;
        assert_eq!(
            &idle_icon.data[dot_center_offset..dot_center_offset + 4],
            &COLOR_STATUS_MUTED
        );
        assert_eq!(
            &armed_icon.data[dot_center_offset..dot_center_offset + 4],
            &COLOR_STATUS_AMBER
        );
        assert_eq!(
            &running_icon.data[dot_center_offset..dot_center_offset + 4],
            &COLOR_STATUS_EMERALD
        );
        assert_eq!(
            &stopped_icon.data[dot_center_offset..dot_center_offset + 4],
            &COLOR_STATUS_CRIMSON
        );
    }
}
