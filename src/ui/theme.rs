use std::fs;
use std::path::Path;
use std::sync::Arc;

use clickforge_sdk::KeyCode;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin, Sense,
    Shadow, Stroke, Style, TextStyle, Visuals, vec2,
};

use crate::models::{BlockCategory, ScriptExecutionPhase};

/// Deep charcoal backdrop used behind the visual block canvas and code editor.
pub const BG_CANVAS: Color32 = Color32::from_rgb(0x11, 0x13, 0x17);

/// Primary panel surface for sidebar, palette, top settings bar, and telemetry drawer.
pub const SURFACE_PANEL: Color32 = Color32::from_rgb(0x18, 0x1B, 0x21);

/// Elevated surface for input controls, modal dialogs, and block bodies.
pub const SURFACE_ELEVATED: Color32 = Color32::from_rgb(0x20, 0x24, 0x2D);

/// Subtle hover surface for interactive buttons and selectable rows.
pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x27, 0x2C, 0x37);

/// Code editor surface recessed slightly darker than standard panels.
pub const SURFACE_CODE: Color32 = Color32::from_rgb(0x14, 0x17, 0x1D);

/// Crisp 1 px structural divider and container border.
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(0x2A, 0x30, 0x3C);

/// Emphasized border for focused widgets and modal outlines.
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x3D, 0x46, 0x57);

/// Primary high-contrast ink color on dark slate backgrounds.
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xEC, 0xEF, 0xF4);

/// Secondary metadata and helper ink color.
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x94, 0xA0, 0xB2);

/// Single primary interactive accent color (amber-copper).
pub const ACCENT_PRIMARY: Color32 = Color32::from_rgb(0xE5, 0x9A, 0x38);

/// Darkened amber-copper fill used behind active toggles and tags.
pub const ACCENT_PRIMARY_TINT: Color32 = Color32::from_rgb(0x38, 0x28, 0x14);

/// Dark ink used on top of solid amber-copper primary buttons for high legibility.
pub const INK_ON_ACCENT: Color32 = Color32::from_rgb(0x14, 0x12, 0x0E);

/// Reserved status color for armed/running scripts and live active blocks.
pub const STATUS_RUNNING: Color32 = Color32::from_rgb(0x2E, 0xA0, 0x43);

/// Deep emerald fill for highlighting the currently executing block.
pub const STATUS_RUNNING_TINT: Color32 = Color32::from_rgb(0x16, 0x2B, 0x1D);

/// Reserved status color for Hard Kill-Switch, validation blockers, and emergency stop.
pub const STATUS_DANGER: Color32 = Color32::from_rgb(0xE0, 0x4F, 0x5F);

/// Dark crimson fill used for validation banners and danger controls.
pub const STATUS_DANGER_TINT: Color32 = Color32::from_rgb(0x36, 0x19, 0x1F);

/// Block category accent for Trigger hat blocks.
pub const CATEGORY_TRIGGER: Color32 = Color32::from_rgb(0xD9, 0x77, 0x24);

/// Block category accent for Control flow blocks (`If`, `While`, `Repeat`).
pub const CATEGORY_CONTROL: Color32 = Color32::from_rgb(0xC9, 0x9A, 0x2E);

/// Block category accent for Mouse and Keyboard action blocks.
pub const CATEGORY_ACTION: Color32 = Color32::from_rgb(0x3B, 0x82, 0xC4);

/// Block category accent for Fixed, Uniform, and Gaussian timing blocks.
pub const CATEGORY_TIMING: Color32 = Color32::from_rgb(0x2A, 0x9D, 0x8F);

/// Block category accent for Variable and state blocks.
pub const CATEGORY_VARIABLE: Color32 = Color32::from_rgb(0x7A, 0x86, 0x99);

/// Syntax color for Rust language keywords.
pub const SYNTAX_KEYWORD: Color32 = Color32::from_rgb(0xE5, 0x9A, 0x38);

/// Syntax color for SDK and standard Rust type identifiers.
pub const SYNTAX_TYPE: Color32 = Color32::from_rgb(0x6C, 0xA6, 0xE8);

/// Syntax color for string literals.
pub const SYNTAX_STRING: Color32 = Color32::from_rgb(0x5E, 0xC2, 0xA6);

/// Syntax color for numeric literals.
pub const SYNTAX_NUMBER: Color32 = Color32::from_rgb(0xD9, 0xB8, 0x6C);

/// Syntax color for single-line and block comments.
pub const SYNTAX_COMMENT: Color32 = Color32::from_rgb(0x72, 0x7F, 0x93);

/// Small UI corner radius in logical pixels.
pub const CORNER_RADIUS_SM_PX: u8 = 4;

/// Standard panel and card corner radius in logical pixels.
pub const CORNER_RADIUS_MD_PX: u8 = 6;

/// Small caption font size in logical pixels.
pub const FONT_SIZE_SMALL_PX: f32 = 11.0;

/// Primary UI body font size in logical pixels.
pub const FONT_SIZE_BODY_PX: f32 = 13.5;

/// Tabular monospace font size for timings, PIDs, and Rust code in logical pixels.
pub const FONT_SIZE_MONOSPACE_PX: f32 = 13.0;

/// Interactive button label font size in logical pixels.
pub const FONT_SIZE_BUTTON_PX: f32 = 13.0;

/// Section heading font size in logical pixels.
pub const FONT_SIZE_HEADING_PX: f32 = 16.5;

const MAX_FONT_FILE_BYTES: u64 = 16 * 1024 * 1024;
const SANS_FONT_KEY: &str = "clickforge_fira_sans";
const MONO_FONT_KEY: &str = "clickforge_monospace";

const FIRA_SANS_CANDIDATE_PATHS: [&str; 6] = [
    "/usr/share/fonts/TTF/FiraSans-Regular.ttf",
    "/usr/share/fonts/TTF/FiraSans-Book.ttf",
    "/usr/share/fonts/truetype/fira/FiraSans-Regular.ttf",
    "/usr/share/fonts/fira/FiraSans-Regular.ttf",
    "/usr/share/fonts/OTF/FiraSans-Regular.otf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
];

const MONOSPACE_CANDIDATE_PATHS: [&str; 6] = [
    "/usr/share/fonts/TTF/Hack-Regular.ttf",
    "/usr/share/fonts/truetype/hack/Hack-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf",
    "/usr/share/fonts/noto/NotoSansMono-Regular.ttf",
];

/// Applies the Forge Slate visual theme, spacing tokens, and system fonts once at startup.
pub fn apply_forge_slate_theme(egui_context: &egui::Context) {
    load_system_fonts(egui_context);

    let mut style = Style::default();
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(FONT_SIZE_SMALL_PX, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(FONT_SIZE_BODY_PX, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(FONT_SIZE_BUTTON_PX, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(FONT_SIZE_HEADING_PX, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(FONT_SIZE_MONOSPACE_PX, FontFamily::Monospace),
    );

    style.spacing.item_spacing = vec2(8.0, 6.0);
    style.spacing.button_padding = vec2(9.0, 4.0);
    style.spacing.window_margin = Margin::same(10);
    style.spacing.menu_margin = Margin::same(6);
    style.spacing.indent = 16.0;
    style.spacing.combo_width = 140.0;
    style.spacing.interact_size = vec2(32.0, 24.0);

    style.visuals = build_forge_slate_visuals();
    egui_context.set_theme(egui::ThemePreference::Dark);
    egui_context.set_style_of(egui::Theme::Dark, style);
}

fn build_forge_slate_visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.dark_mode = true;
    visuals.panel_fill = SURFACE_PANEL;
    visuals.window_fill = SURFACE_PANEL;
    visuals.window_stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.window_corner_radius = CornerRadius::same(CORNER_RADIUS_MD_PX);
    visuals.menu_corner_radius = CornerRadius::same(CORNER_RADIUS_SM_PX);
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.extreme_bg_color = BG_CANVAS;
    visuals.text_edit_bg_color = Some(SURFACE_CODE);
    visuals.faint_bg_color = SURFACE_ELEVATED;
    visuals.code_bg_color = SURFACE_CODE;
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.weak_text_color = Some(TEXT_MUTED);
    visuals.hyperlink_color = ACCENT_PRIMARY;
    visuals.warn_fg_color = ACCENT_PRIMARY;
    visuals.error_fg_color = STATUS_DANGER;
    visuals.selection.bg_fill = ACCENT_PRIMARY_TINT;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT_PRIMARY);

    let corner_sm = CornerRadius::same(CORNER_RADIUS_SM_PX);

    visuals.widgets.noninteractive.bg_fill = SURFACE_PANEL;
    visuals.widgets.noninteractive.weak_bg_fill = SURFACE_PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.noninteractive.corner_radius = corner_sm;

    visuals.widgets.inactive.bg_fill = SURFACE_ELEVATED;
    visuals.widgets.inactive.weak_bg_fill = SURFACE_ELEVATED;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = corner_sm;

    visuals.widgets.hovered.bg_fill = SURFACE_HOVER;
    visuals.widgets.hovered.weak_bg_fill = SURFACE_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.hovered.corner_radius = corner_sm;
    visuals.widgets.hovered.expansion = 0.0;

    visuals.widgets.active.bg_fill = ACCENT_PRIMARY_TINT;
    visuals.widgets.active.weak_bg_fill = ACCENT_PRIMARY_TINT;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT_PRIMARY);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.active.corner_radius = corner_sm;
    visuals.widgets.active.expansion = 0.0;

    visuals.widgets.open.bg_fill = SURFACE_HOVER;
    visuals.widgets.open.weak_bg_fill = SURFACE_HOVER;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, ACCENT_PRIMARY);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.open.corner_radius = corner_sm;

    visuals
}

fn load_system_fonts(egui_context: &egui::Context) {
    let mut font_definitions = FontDefinitions::default();
    let mut did_install_any = false;

    if let Some(sans_bytes) = read_first_available_font(&FIRA_SANS_CANDIDATE_PATHS) {
        font_definitions.font_data.insert(
            SANS_FONT_KEY.to_string(),
            Arc::new(FontData::from_owned(sans_bytes)),
        );
        if let Some(proportional_list) =
            font_definitions.families.get_mut(&FontFamily::Proportional)
        {
            proportional_list.insert(0, SANS_FONT_KEY.to_string());
        }
        did_install_any = true;
    }

    if let Some(mono_bytes) = read_first_available_font(&MONOSPACE_CANDIDATE_PATHS) {
        font_definitions.font_data.insert(
            MONO_FONT_KEY.to_string(),
            Arc::new(FontData::from_owned(mono_bytes)),
        );
        if let Some(monospace_list) = font_definitions.families.get_mut(&FontFamily::Monospace) {
            monospace_list.insert(0, MONO_FONT_KEY.to_string());
        }
        did_install_any = true;
    }

    if did_install_any {
        egui_context.set_fonts(font_definitions);
    }
}

fn read_first_available_font(candidate_paths: &[&str]) -> Option<Vec<u8>> {
    for raw_path in candidate_paths {
        let font_path = Path::new(raw_path);
        let Ok(metadata) = fs::metadata(font_path) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_FONT_FILE_BYTES {
            continue;
        }
        if let Ok(font_bytes) = fs::read(font_path) {
            return Some(font_bytes);
        }
    }
    None
}

/// Builds a crisp flat panel frame with explicit background fill and inner padding.
pub fn build_panel_frame(fill_color: Color32, margin_x_px: i8, margin_y_px: i8) -> Frame {
    Frame::NONE
        .fill(fill_color)
        .inner_margin(Margin::symmetric(margin_x_px, margin_y_px))
}

/// Returns the reserved Forge Slate accent color for a visual block category.
pub fn category_color(category: BlockCategory) -> Color32 {
    match category {
        BlockCategory::Trigger => CATEGORY_TRIGGER,
        BlockCategory::Control => CATEGORY_CONTROL,
        BlockCategory::Action => CATEGORY_ACTION,
        BlockCategory::Timing => CATEGORY_TIMING,
        BlockCategory::Variable => CATEGORY_VARIABLE,
    }
}

/// Returns the reserved status dot color for a script execution phase.
pub fn phase_status_color(phase: ScriptExecutionPhase) -> Color32 {
    match phase {
        ScriptExecutionPhase::Disabled => TEXT_MUTED,
        ScriptExecutionPhase::WaitingForFocus => CATEGORY_VARIABLE,
        ScriptExecutionPhase::Armed => ACCENT_PRIMARY,
        ScriptExecutionPhase::Running => STATUS_RUNNING,
        ScriptExecutionPhase::Compiling => CATEGORY_ACTION,
        ScriptExecutionPhase::StoppedByKillSwitch | ScriptExecutionPhase::Error => STATUS_DANGER,
    }
}

/// Paints a small crisp circular status indicator dot.
pub fn paint_status_dot(ui: &mut egui::Ui, dot_color: Color32, radius_px: f32) -> egui::Response {
    let diameter_px = radius_px * 2.0;
    let (dot_rect, response) =
        ui.allocate_exact_size(vec2(diameter_px, diameter_px), Sense::hover());
    ui.painter()
        .circle_filled(dot_rect.center(), radius_px, dot_color);
    response
}

/// Renders a compact bordered pill badge with explicit background, border, and ink colors.
pub fn render_badge_pill(
    ui: &mut egui::Ui,
    label_text: &str,
    fill_color: Color32,
    text_color: Color32,
    border_color: Color32,
) {
    Frame::NONE
        .fill(fill_color)
        .stroke(Stroke::new(1.0, border_color))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(label_text)
                    .size(FONT_SIZE_SMALL_PX)
                    .color(text_color),
            );
        });
}

/// Truncates display text safely on Unicode character boundaries with an ellipsis.
pub fn truncate_display_text(raw_text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let char_count = raw_text.chars().count();
    if char_count <= max_chars {
        return raw_text.to_string();
    }
    let visible_chars = max_chars.saturating_sub(1);
    let mut truncated: String = raw_text.chars().take(visible_chars).collect();
    truncated.push('…');
    truncated
}

/// Maps an `egui::Key` press event into a platform-neutral `clickforge_sdk::KeyCode`.
pub fn map_egui_key_to_sdk_key(egui_key: egui::Key) -> Option<KeyCode> {
    let mapped_key = match egui_key {
        egui::Key::A => KeyCode::A,
        egui::Key::B => KeyCode::B,
        egui::Key::C => KeyCode::C,
        egui::Key::D => KeyCode::D,
        egui::Key::E => KeyCode::E,
        egui::Key::F => KeyCode::F,
        egui::Key::G => KeyCode::G,
        egui::Key::H => KeyCode::H,
        egui::Key::I => KeyCode::I,
        egui::Key::J => KeyCode::J,
        egui::Key::K => KeyCode::K,
        egui::Key::L => KeyCode::L,
        egui::Key::M => KeyCode::M,
        egui::Key::N => KeyCode::N,
        egui::Key::O => KeyCode::O,
        egui::Key::P => KeyCode::P,
        egui::Key::Q => KeyCode::Q,
        egui::Key::R => KeyCode::R,
        egui::Key::S => KeyCode::S,
        egui::Key::T => KeyCode::T,
        egui::Key::U => KeyCode::U,
        egui::Key::V => KeyCode::V,
        egui::Key::W => KeyCode::W,
        egui::Key::X => KeyCode::X,
        egui::Key::Y => KeyCode::Y,
        egui::Key::Z => KeyCode::Z,
        egui::Key::Num0 => KeyCode::Digit0,
        egui::Key::Num1 => KeyCode::Digit1,
        egui::Key::Num2 => KeyCode::Digit2,
        egui::Key::Num3 => KeyCode::Digit3,
        egui::Key::Num4 => KeyCode::Digit4,
        egui::Key::Num5 => KeyCode::Digit5,
        egui::Key::Num6 => KeyCode::Digit6,
        egui::Key::Num7 => KeyCode::Digit7,
        egui::Key::Num8 => KeyCode::Digit8,
        egui::Key::Num9 => KeyCode::Digit9,
        egui::Key::F1 => KeyCode::F1,
        egui::Key::F2 => KeyCode::F2,
        egui::Key::F3 => KeyCode::F3,
        egui::Key::F4 => KeyCode::F4,
        egui::Key::F5 => KeyCode::F5,
        egui::Key::F6 => KeyCode::F6,
        egui::Key::F7 => KeyCode::F7,
        egui::Key::F8 => KeyCode::F8,
        egui::Key::F9 => KeyCode::F9,
        egui::Key::F10 => KeyCode::F10,
        egui::Key::F11 => KeyCode::F11,
        egui::Key::F12 => KeyCode::F12,
        egui::Key::ShiftLeft => KeyCode::LeftShift,
        egui::Key::ShiftRight => KeyCode::RightShift,
        egui::Key::ControlLeft => KeyCode::LeftCtrl,
        egui::Key::ControlRight => KeyCode::RightCtrl,
        egui::Key::AltLeft => KeyCode::LeftAlt,
        egui::Key::AltRight => KeyCode::RightAlt,
        egui::Key::SuperLeft => KeyCode::LeftSuper,
        egui::Key::SuperRight => KeyCode::RightSuper,
        egui::Key::CapsLock => KeyCode::CapsLock,
        egui::Key::ArrowUp => KeyCode::Up,
        egui::Key::ArrowDown => KeyCode::Down,
        egui::Key::ArrowLeft => KeyCode::Left,
        egui::Key::ArrowRight => KeyCode::Right,
        egui::Key::Home => KeyCode::Home,
        egui::Key::End => KeyCode::End,
        egui::Key::PageUp => KeyCode::PageUp,
        egui::Key::PageDown => KeyCode::PageDown,
        egui::Key::Insert => KeyCode::Insert,
        egui::Key::Space => KeyCode::Space,
        egui::Key::Enter => KeyCode::Enter,
        egui::Key::Tab => KeyCode::Tab,
        egui::Key::Escape => KeyCode::Escape,
        egui::Key::Backspace => KeyCode::Backspace,
        egui::Key::Delete => KeyCode::Delete,
        egui::Key::Pause => KeyCode::Pause,
        egui::Key::ScrollLock => KeyCode::ScrollLock,
        egui::Key::PrintScreen => KeyCode::PrintScreen,
        egui::Key::Minus => KeyCode::Minus,
        egui::Key::Equals => KeyCode::Equal,
        egui::Key::OpenBracket => KeyCode::LeftBracket,
        egui::Key::CloseBracket => KeyCode::RightBracket,
        egui::Key::Backslash => KeyCode::Backslash,
        egui::Key::Semicolon => KeyCode::Semicolon,
        egui::Key::Quote => KeyCode::Apostrophe,
        egui::Key::Backtick => KeyCode::Grave,
        egui::Key::Comma => KeyCode::Comma,
        egui::Key::Period => KeyCode::Period,
        egui::Key::Slash => KeyCode::Slash,
        egui::Key::NumLock => KeyCode::NumLock,
        _ => return None,
    };
    Some(mapped_key)
}
