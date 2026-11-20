use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use clickforge_sdk::KeyCode;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin, RichText,
    Sense, Shadow, Stroke, Style, TextStyle, Visuals, vec2,
};

use crate::models::{BlockCategory, ScriptExecutionPhase};

/// Deep charcoal backdrop used behind the visual block canvas and code editor.
pub const BG_CANVAS: Color32 = Color32::from_rgb(0x11, 0x13, 0x17);

/// Primary panel surface for sidebar, palette, top settings bar, and telemetry drawer.
pub const SURFACE_PANEL: Color32 = Color32::from_rgb(0x18, 0x1C, 0x23);

/// Elevated surface for input controls, modal dialogs, and recessed cards.
pub const SURFACE_ELEVATED: Color32 = Color32::from_rgb(0x21, 0x26, 0x31);

/// Distinct hover and active selection surface above `SURFACE_ELEVATED`.
pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x2C, 0x33, 0x42);

/// Code editor surface recessed slightly darker than standard panels.
pub const SURFACE_CODE: Color32 = Color32::from_rgb(0x14, 0x17, 0x1D);

/// Crisp 1 px structural divider and container border.
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(0x32, 0x3A, 0x4A);

/// Emphasized border for focused widgets and modal outlines.
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x4A, 0x55, 0x6B);

/// Primary high-contrast ink color on dark slate backgrounds.
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xEE, 0xF1, 0xF6);

/// Secondary metadata and helper ink color passing WCAG AA on elevated surfaces.
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x9A, 0xA7, 0xBA);

/// Single primary interactive accent color (amber-copper).
pub const ACCENT_PRIMARY: Color32 = Color32::from_rgb(0xE5, 0x9A, 0x38);

/// Darkened amber-copper fill used behind active toggles and tags.
pub const ACCENT_PRIMARY_TINT: Color32 = Color32::from_rgb(0x38, 0x28, 0x14);

/// Dark ink used on top of solid amber-copper primary buttons for high legibility.
pub const INK_ON_ACCENT: Color32 = Color32::from_rgb(0x14, 0x12, 0x0E);

/// Reserved status color for armed/running scripts and live active blocks.
pub const STATUS_RUNNING: Color32 = Color32::from_rgb(0x2E, 0xA0, 0x43);

/// High-contrast emerald ink passing WCAG AA on `STATUS_RUNNING_TINT`.
pub const STATUS_RUNNING_INK: Color32 = Color32::from_rgb(0x44, 0xC4, 0x5E);

/// Deep emerald fill for highlighting the currently executing block.
pub const STATUS_RUNNING_TINT: Color32 = Color32::from_rgb(0x16, 0x2B, 0x1D);

/// Reserved status color for Hard Kill-Switch accents and borders.
pub const STATUS_DANGER: Color32 = Color32::from_rgb(0xE0, 0x4F, 0x5F);

/// Deep crimson fill for solid white-text danger buttons (`STOP ALL`).
pub const STATUS_DANGER_SOLID: Color32 = Color32::from_rgb(0xC9, 0x3B, 0x4B);

/// High-contrast coral-crimson ink passing WCAG AA on `STATUS_DANGER_TINT`.
pub const STATUS_DANGER_INK: Color32 = Color32::from_rgb(0xF2, 0x6D, 0x7A);

/// Dark crimson fill used for validation banners and danger controls.
pub const STATUS_DANGER_TINT: Color32 = Color32::from_rgb(0x36, 0x19, 0x1F);

/// High-contrast informational sky ink passing WCAG AA on `SURFACE_ELEVATED`.
pub const STATUS_INFO_INK: Color32 = Color32::from_rgb(0x6C, 0xAD, 0xF5);

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
pub const SYNTAX_TYPE: Color32 = Color32::from_rgb(0x6C, 0xAD, 0xF5);

/// Syntax color for string literals.
pub const SYNTAX_STRING: Color32 = Color32::from_rgb(0x5E, 0xC2, 0xA6);

/// Syntax color for numeric literals.
pub const SYNTAX_NUMBER: Color32 = Color32::from_rgb(0xDF, 0xBF, 0x72);

/// Syntax color for single-line and block comments (WCAG AA >= 5.8:1 on `SURFACE_CODE`).
pub const SYNTAX_COMMENT: Color32 = Color32::from_rgb(0x86, 0x94, 0xA8);

/// Inner UI control corner radius in logical pixels (concentric inside 8 px outer cards).
pub const CORNER_RADIUS_SM_PX: u8 = 4;

/// Outer panel, card, popover, and modal corner radius in logical pixels.
pub const CORNER_RADIUS_MD_PX: u8 = 8;

/// Rounded pill corner radius for segmented tracks, chips, and inline block wells.
pub const CORNER_RADIUS_PILL_PX: u8 = 10;

/// Small caption and metadata font size in logical pixels.
pub const FONT_SIZE_SMALL_PX: f32 = 12.0;

/// Primary UI body font size in logical pixels.
pub const FONT_SIZE_BODY_PX: f32 = 14.0;

/// Tabular monospace font size for timings, PIDs, and Rust code in logical pixels.
pub const FONT_SIZE_MONOSPACE_PX: f32 = 13.0;

/// Code editor monospace font size in logical pixels.
pub const FONT_SIZE_CODE_PX: f32 = 13.0;

/// Interactive button label font size in logical pixels.
pub const FONT_SIZE_BUTTON_PX: f32 = 13.5;

/// Section heading font size in logical pixels.
pub const FONT_SIZE_HEADING_PX: f32 = 17.5;

const MAX_FONT_FILE_BYTES: u64 = 16 * 1024 * 1024;
const SANS_REGULAR_FONT_KEY: &str = "clickforge_fira_sans_regular";
const SANS_SEMIBOLD_FONT_KEY: &str = "clickforge_fira_sans_semibold";
const SANS_BOLD_FONT_KEY: &str = "clickforge_fira_sans_bold";
const MONO_REGULAR_FONT_KEY: &str = "clickforge_mono_regular";
const MONO_BOLD_FONT_KEY: &str = "clickforge_mono_bold";

const FONT_FAMILY_SEMIBOLD_NAME: &str = "clickforge_semibold";
const FONT_FAMILY_BOLD_NAME: &str = "clickforge_bold";
const FONT_FAMILY_MONO_BOLD_NAME: &str = "clickforge_mono_bold";

const WINDOW_SHADOW_OFFSET_Y_PX: i8 = 8;
const WINDOW_SHADOW_BLUR_PX: u8 = 24;
const WINDOW_SHADOW_ALPHA: u8 = 128;
const POPUP_SHADOW_OFFSET_Y_PX: i8 = 6;
const POPUP_SHADOW_BLUR_PX: u8 = 16;
const POPUP_SHADOW_ALPHA: u8 = 116;

const FIRA_REGULAR_CANDIDATE_PATHS: [&str; 6] = [
    "/usr/share/fonts/TTF/FiraSans-Regular.ttf",
    "/usr/share/fonts/TTF/FiraSans-Book.ttf",
    "/usr/share/fonts/truetype/fira/FiraSans-Regular.ttf",
    "/usr/share/fonts/fira/FiraSans-Regular.ttf",
    "/usr/share/fonts/OTF/FiraSans-Regular.otf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
];

const FIRA_SEMIBOLD_CANDIDATE_PATHS: [&str; 5] = [
    "/usr/share/fonts/TTF/FiraSans-SemiBold.ttf",
    "/usr/share/fonts/TTF/FiraSans-Medium.ttf",
    "/usr/share/fonts/TTF/FiraSans-Regular.ttf",
    "/usr/share/fonts/truetype/fira/FiraSans-SemiBold.ttf",
    "/usr/share/fonts/noto/NotoSans-SemiBold.ttf",
];

const FIRA_BOLD_CANDIDATE_PATHS: [&str; 5] = [
    "/usr/share/fonts/TTF/FiraSans-Bold.ttf",
    "/usr/share/fonts/TTF/FiraSans-SemiBold.ttf",
    "/usr/share/fonts/TTF/FiraSans-Regular.ttf",
    "/usr/share/fonts/truetype/fira/FiraSans-Bold.ttf",
    "/usr/share/fonts/noto/NotoSans-Bold.ttf",
];

const MONO_REGULAR_CANDIDATE_PATHS: [&str; 6] = [
    "/usr/share/fonts/TTF/Hack-Regular.ttf",
    "/usr/share/fonts/truetype/hack/Hack-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf",
    "/usr/share/fonts/noto/NotoSansMono-Regular.ttf",
];

const MONO_BOLD_CANDIDATE_PATHS: [&str; 5] = [
    "/usr/share/fonts/TTF/Hack-Bold.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono-Bold.ttf",
    "/usr/share/fonts/TTF/Hack-Regular.ttf",
    "/usr/share/fonts/truetype/hack/Hack-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
];

/// Complete dark-mode color palette for rendering a visual block category with high contrast.
#[derive(Debug, Clone, Copy)]
pub struct BlockVisualColors {
    pub fill: Color32,
    pub fill_hover: Color32,
    pub border: Color32,
    pub top_highlight: Color32,
    pub spine_fill: Color32,
    pub pill_well_bg: Color32,
    pub pill_well_border: Color32,
    pub badge_bg: Color32,
    pub text_primary: Color32,
    pub text_muted: Color32,
}

/// Returns the SemiBold proportional font family backed by `FiraSans-SemiBold`.
pub fn font_family_semibold() -> FontFamily {
    static FAMILY_NAME: OnceLock<Arc<str>> = OnceLock::new();
    let name_arc = FAMILY_NAME.get_or_init(|| Arc::from(FONT_FAMILY_SEMIBOLD_NAME));
    FontFamily::Name(Arc::clone(name_arc))
}

/// Returns the Bold proportional font family backed by `FiraSans-Bold`.
pub fn font_family_bold() -> FontFamily {
    static FAMILY_NAME: OnceLock<Arc<str>> = OnceLock::new();
    let name_arc = FAMILY_NAME.get_or_init(|| Arc::from(FONT_FAMILY_BOLD_NAME));
    FontFamily::Name(Arc::clone(name_arc))
}

/// Returns the Bold monospace font family backed by `Hack-Bold`.
pub fn font_family_mono_bold() -> FontFamily {
    static FAMILY_NAME: OnceLock<Arc<str>> = OnceLock::new();
    let name_arc = FAMILY_NAME.get_or_init(|| Arc::from(FONT_FAMILY_MONO_BOLD_NAME));
    FontFamily::Name(Arc::clone(name_arc))
}

/// Builds a `RichText` span rendered in the SemiBold proportional family.
pub fn rich_semibold(text: impl Into<String>, size_px: f32, color: Color32) -> RichText {
    RichText::new(text)
        .font(FontId::new(size_px, font_family_semibold()))
        .color(color)
}

/// Builds a `RichText` span rendered in the Bold proportional family.
pub fn rich_bold(text: impl Into<String>, size_px: f32, color: Color32) -> RichText {
    RichText::new(text)
        .font(FontId::new(size_px, font_family_bold()))
        .color(color)
}

/// Builds a `RichText` span rendered in the Regular tabular monospace family.
pub fn rich_mono(text: impl Into<String>, size_px: f32, color: Color32) -> RichText {
    RichText::new(text)
        .font(FontId::new(size_px, FontFamily::Monospace))
        .color(color)
}

/// Builds a `RichText` span rendered in the Bold tabular monospace family.
pub fn rich_mono_bold(text: impl Into<String>, size_px: f32, color: Color32) -> RichText {
    RichText::new(text)
        .font(FontId::new(size_px, font_family_mono_bold()))
        .color(color)
}

/// Returns the cohesive dark-mode surface, border, and high-contrast ink tokens for a block category.
pub fn block_visual_colors(category: BlockCategory) -> BlockVisualColors {
    match category {
        BlockCategory::Trigger => BlockVisualColors {
            fill: Color32::from_rgb(0x4D, 0x2A, 0x10),
            fill_hover: Color32::from_rgb(0x5E, 0x35, 0x15),
            border: CATEGORY_TRIGGER,
            top_highlight: Color32::from_rgb(0xE8, 0x98, 0x4E),
            spine_fill: Color32::from_rgb(0x3F, 0x22, 0x0C),
            pill_well_bg: Color32::from_rgb(0x26, 0x14, 0x07),
            pill_well_border: Color32::from_rgb(0x8C, 0x4E, 0x1B),
            badge_bg: Color32::from_rgb(0x69, 0x3A, 0x14),
            text_primary: Color32::from_rgb(0xFF, 0xF4, 0xE6),
            text_muted: Color32::from_rgb(0xE8, 0xC4, 0x9E),
        },
        BlockCategory::Control => BlockVisualColors {
            fill: Color32::from_rgb(0x45, 0x35, 0x11),
            fill_hover: Color32::from_rgb(0x55, 0x42, 0x17),
            border: CATEGORY_CONTROL,
            top_highlight: Color32::from_rgb(0xDF, 0xB8, 0x56),
            spine_fill: Color32::from_rgb(0x39, 0x2B, 0x0D),
            pill_well_bg: Color32::from_rgb(0x24, 0x1B, 0x08),
            pill_well_border: Color32::from_rgb(0x8A, 0x6A, 0x1F),
            badge_bg: Color32::from_rgb(0x5E, 0x49, 0x18),
            text_primary: Color32::from_rgb(0xFF, 0xF8, 0xE3),
            text_muted: Color32::from_rgb(0xE3, 0xCF, 0xA0),
        },
        BlockCategory::Action => BlockVisualColors {
            fill: Color32::from_rgb(0x17, 0x32, 0x4D),
            fill_hover: Color32::from_rgb(0x1E, 0x40, 0x63),
            border: Color32::from_rgb(0x4A, 0x90, 0xD9),
            top_highlight: Color32::from_rgb(0x73, 0xB0, 0xED),
            spine_fill: Color32::from_rgb(0x12, 0x28, 0x3E),
            pill_well_bg: Color32::from_rgb(0x0D, 0x1D, 0x2E),
            pill_well_border: Color32::from_rgb(0x31, 0x62, 0x96),
            badge_bg: Color32::from_rgb(0x21, 0x47, 0x6E),
            text_primary: Color32::from_rgb(0xED, 0xF5, 0xFF),
            text_muted: Color32::from_rgb(0xB0, 0xCC, 0xE8),
        },
        BlockCategory::Timing => BlockVisualColors {
            fill: Color32::from_rgb(0x13, 0x3B, 0x36),
            fill_hover: Color32::from_rgb(0x19, 0x4C, 0x46),
            border: Color32::from_rgb(0x2F, 0xA8, 0x99),
            top_highlight: Color32::from_rgb(0x56, 0xC7, 0xB8),
            spine_fill: Color32::from_rgb(0x0E, 0x2F, 0x2B),
            pill_well_bg: Color32::from_rgb(0x0A, 0x21, 0x1E),
            pill_well_border: Color32::from_rgb(0x21, 0x73, 0x68),
            badge_bg: Color32::from_rgb(0x1B, 0x52, 0x4B),
            text_primary: Color32::from_rgb(0xE8, 0xFA, 0xF7),
            text_muted: Color32::from_rgb(0xA5, 0xD6, 0xCF),
        },
        BlockCategory::Variable => BlockVisualColors {
            fill: Color32::from_rgb(0x27, 0x2E, 0x3B),
            fill_hover: Color32::from_rgb(0x31, 0x3A, 0x4A),
            border: Color32::from_rgb(0x7A, 0x89, 0xA1),
            top_highlight: Color32::from_rgb(0x9D, 0xAC, 0xC4),
            spine_fill: Color32::from_rgb(0x1F, 0x25, 0x30),
            pill_well_bg: Color32::from_rgb(0x15, 0x19, 0x21),
            pill_well_border: Color32::from_rgb(0x51, 0x5C, 0x6E),
            badge_bg: Color32::from_rgb(0x36, 0x40, 0x52),
            text_primary: Color32::from_rgb(0xEF, 0xF3, 0xF8),
            text_muted: Color32::from_rgb(0xB8, 0xC4, 0xD6),
        },
    }
}

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
        FontId::new(FONT_SIZE_HEADING_PX, font_family_bold()),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(FONT_SIZE_MONOSPACE_PX, FontFamily::Monospace),
    );

    style.spacing.item_spacing = vec2(8.0, 6.0);
    style.spacing.button_padding = vec2(10.0, 5.0);
    style.spacing.window_margin = Margin::same(12);
    style.spacing.menu_margin = Margin::same(8);
    style.spacing.indent = 16.0;
    style.spacing.combo_width = 148.0;
    style.spacing.interact_size = vec2(36.0, 28.0);

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
    visuals.menu_corner_radius = CornerRadius::same(CORNER_RADIUS_MD_PX);
    visuals.window_shadow = Shadow {
        offset: [0, WINDOW_SHADOW_OFFSET_Y_PX],
        blur: WINDOW_SHADOW_BLUR_PX,
        spread: 0,
        color: Color32::from_black_alpha(WINDOW_SHADOW_ALPHA),
    };
    visuals.popup_shadow = Shadow {
        offset: [0, POPUP_SHADOW_OFFSET_Y_PX],
        blur: POPUP_SHADOW_BLUR_PX,
        spread: 0,
        color: Color32::from_black_alpha(POPUP_SHADOW_ALPHA),
    };
    visuals.extreme_bg_color = BG_CANVAS;
    visuals.text_edit_bg_color = Some(SURFACE_CODE);
    visuals.faint_bg_color = SURFACE_ELEVATED;
    visuals.code_bg_color = SURFACE_CODE;
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.weak_text_color = Some(TEXT_MUTED);
    visuals.hyperlink_color = ACCENT_PRIMARY;
    visuals.warn_fg_color = ACCENT_PRIMARY;
    visuals.error_fg_color = STATUS_DANGER_INK;
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

    let has_sans_regular = register_font_face(
        &mut font_definitions,
        SANS_REGULAR_FONT_KEY,
        &FIRA_REGULAR_CANDIDATE_PATHS,
    );
    let has_sans_semibold = register_font_face(
        &mut font_definitions,
        SANS_SEMIBOLD_FONT_KEY,
        &FIRA_SEMIBOLD_CANDIDATE_PATHS,
    );
    let has_sans_bold = register_font_face(
        &mut font_definitions,
        SANS_BOLD_FONT_KEY,
        &FIRA_BOLD_CANDIDATE_PATHS,
    );
    let has_mono_regular = register_font_face(
        &mut font_definitions,
        MONO_REGULAR_FONT_KEY,
        &MONO_REGULAR_CANDIDATE_PATHS,
    );
    let has_mono_bold = register_font_face(
        &mut font_definitions,
        MONO_BOLD_FONT_KEY,
        &MONO_BOLD_CANDIDATE_PATHS,
    );

    if has_sans_regular
        && let Some(proportional_chain) =
            font_definitions.families.get_mut(&FontFamily::Proportional)
    {
        proportional_chain.insert(0, SANS_REGULAR_FONT_KEY.to_string());
    }

    if has_mono_regular
        && let Some(monospace_chain) = font_definitions.families.get_mut(&FontFamily::Monospace)
    {
        monospace_chain.insert(0, MONO_REGULAR_FONT_KEY.to_string());
    }

    let proportional_fallbacks = font_definitions
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let monospace_fallbacks = font_definitions
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();

    let mut semibold_chain = proportional_fallbacks.clone();
    if has_sans_semibold {
        semibold_chain.insert(0, SANS_SEMIBOLD_FONT_KEY.to_string());
    }
    font_definitions
        .families
        .insert(font_family_semibold(), semibold_chain);

    let mut bold_chain = proportional_fallbacks;
    if has_sans_semibold {
        bold_chain.insert(0, SANS_SEMIBOLD_FONT_KEY.to_string());
    }
    if has_sans_bold {
        bold_chain.insert(0, SANS_BOLD_FONT_KEY.to_string());
    }
    font_definitions
        .families
        .insert(font_family_bold(), bold_chain);

    let mut mono_bold_chain = monospace_fallbacks;
    if has_mono_bold {
        mono_bold_chain.insert(0, MONO_BOLD_FONT_KEY.to_string());
    }
    font_definitions
        .families
        .insert(font_family_mono_bold(), mono_bold_chain);

    egui_context.set_fonts(font_definitions);
}

fn register_font_face(
    font_definitions: &mut FontDefinitions,
    font_key: &str,
    candidate_paths: &[&str],
) -> bool {
    let Some(font_bytes) = read_first_available_font(candidate_paths) else {
        return false;
    };
    font_definitions.font_data.insert(
        font_key.to_string(),
        Arc::new(FontData::from_owned(font_bytes)),
    );
    true
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
        ScriptExecutionPhase::Running => STATUS_RUNNING_INK,
        ScriptExecutionPhase::Compiling => STATUS_INFO_INK,
        ScriptExecutionPhase::StoppedByKillSwitch | ScriptExecutionPhase::Error => {
            STATUS_DANGER_INK
        }
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
            ui.label(rich_semibold(label_text, FONT_SIZE_SMALL_PX, text_color));
        });
}

/// Builds a bordered or borderless `egui::Button` with explicit Forge Slate styling.
pub(crate) fn build_styled_button(
    label: RichText,
    fill_color: Color32,
    border_color: Color32,
    corner_radius_px: u8,
    min_size_px: egui::Vec2,
) -> egui::Button<'static> {
    let stroke = if border_color == Color32::TRANSPARENT {
        Stroke::NONE
    } else {
        Stroke::new(1.0, border_color)
    };
    egui::Button::new(label)
        .fill(fill_color)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(corner_radius_px))
        .min_size(min_size_px)
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
