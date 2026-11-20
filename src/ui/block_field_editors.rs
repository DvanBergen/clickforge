mod condition_editor;
mod key_picker;

use std::ops::RangeInclusive;

use clickforge_sdk::GaussianRangeMs;
use egui::{
    Button, Color32, CornerRadius, DragValue, FontId, Frame, Id, Margin, Popup, PopupCloseBehavior,
    Sense, Stroke, TextEdit, TextStyle, Ui, style::WidgetVisuals, vec2,
};

use crate::models::{BlockId, CustomBlockDefinition, StatementKind, lookup_custom_block};
use crate::ui::theme::{
    ACCENT_PRIMARY, BORDER_STRONG, BlockVisualColors, CORNER_RADIUS_PILL_PX, CORNER_RADIUS_SM_PX,
    FONT_SIZE_BODY_PX, FONT_SIZE_SMALL_PX, STATUS_DANGER_INK, STATUS_DANGER_SOLID,
    STATUS_RUNNING_INK, SURFACE_HOVER, TEXT_MUTED, TEXT_PRIMARY, font_family_mono_bold,
    rich_mono_bold, rich_semibold, truncate_display_text,
};

pub(crate) use key_picker::render_input_trigger_picker;

use condition_editor::render_condition_editor;
use key_picker::{render_key_code_picker, render_mouse_button_combo};

pub(super) const VARIABLE_INPUT_WIDTH_PX: f32 = 88.0;
const TYPE_TEXT_WIDTH_PX: f32 = 148.0;
const PILL_TEXT_SIZE_PX: f32 = 12.5;
const BLOCK_LABEL_SIZE_PX: f32 = 13.5;
const GAUSSIAN_POPUP_WIDTH_PX: f32 = 256.0;
const MAX_CUSTOM_BLOCK_INLINE_NAME_CHARS: usize = 32;

/// Outcome returned by `render_leaf_block_sentence`, capturing field edits and quick-edit clicks.
#[derive(Debug, Default)]
pub(crate) struct LeafBlockSentenceOutcome {
    pub did_edit: bool,
    pub edit_custom_block_id: Option<String>,
}

/// Applies recessed dark pill-well styling and bold tabular monospace digits inside a block row.
pub(crate) fn with_pill_well_style<ResultType>(
    ui: &mut Ui,
    visual_colors: BlockVisualColors,
    add_contents: impl FnOnce(&mut Ui) -> ResultType,
) -> ResultType {
    ui.scope(|ui| {
        let pill_radius = CornerRadius::same(CORNER_RADIUS_PILL_PX);
        let style = ui.style_mut();
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(PILL_TEXT_SIZE_PX, font_family_mono_bold()),
        );
        style.drag_value_text_style = TextStyle::Monospace;
        style.spacing.button_padding = vec2(8.0, 2.5);
        style.spacing.interact_size = vec2(26.0, 23.0);
        style.spacing.item_spacing = vec2(5.0, 4.0);

        let visuals = &mut style.visuals;
        visuals.text_edit_bg_color = Some(visual_colors.pill_well_bg);
        visuals.extreme_bg_color = visual_colors.pill_well_bg;
        let text_stroke = Stroke::new(1.0, visual_colors.text_primary);
        let widget_states: [(&mut WidgetVisuals, Color32, Stroke); 4] = [
            (
                &mut visuals.widgets.inactive,
                visual_colors.pill_well_bg,
                Stroke::new(1.0, visual_colors.pill_well_border),
            ),
            (
                &mut visuals.widgets.hovered,
                visual_colors.badge_bg,
                Stroke::new(1.25, ACCENT_PRIMARY),
            ),
            (
                &mut visuals.widgets.active,
                visual_colors.pill_well_bg,
                Stroke::new(1.5, ACCENT_PRIMARY),
            ),
            (
                &mut visuals.widgets.open,
                visual_colors.badge_bg,
                Stroke::new(1.5, ACCENT_PRIMARY),
            ),
        ];
        for (widget_visuals, fill, stroke) in widget_states {
            widget_visuals.bg_fill = fill;
            widget_visuals.weak_bg_fill = fill;
            widget_visuals.bg_stroke = stroke;
            widget_visuals.fg_stroke = text_stroke;
            widget_visuals.corner_radius = pill_radius;
        }
        add_contents(ui)
    })
    .inner
}

/// Renders inline editable parameter controls for any container or leaf statement block.
pub(crate) fn render_statement_inline_fields(
    ui: &mut Ui,
    id_namespace: &str,
    block_id: BlockId,
    statement_kind: &mut StatementKind,
    visual_colors: BlockVisualColors,
    custom_blocks: &[CustomBlockDefinition],
) -> bool {
    let is_container = matches!(
        statement_kind,
        StatementKind::IfThen { .. }
            | StatementKind::IfThenElse { .. }
            | StatementKind::WhileCondition { .. }
            | StatementKind::RepeatCount { .. }
    );
    if is_container {
        render_container_block_sentence(ui, id_namespace, block_id, statement_kind, visual_colors)
    } else {
        render_leaf_block_sentence(
            ui,
            id_namespace,
            block_id,
            statement_kind,
            visual_colors,
            custom_blocks,
            false,
        )
        .did_edit
    }
}

/// Renders the natural-language header sentence inside a C-block container.
pub(crate) fn render_container_block_sentence(
    ui: &mut Ui,
    id_namespace: &str,
    block_id: BlockId,
    statement_kind: &mut StatementKind,
    visual_colors: BlockVisualColors,
) -> bool {
    let is_while_loop = matches!(statement_kind, StatementKind::WhileCondition { .. });
    with_pill_well_style(ui, visual_colors, |ui| match statement_kind {
        StatementKind::IfThen { condition, .. }
        | StatementKind::IfThenElse { condition, .. }
        | StatementKind::WhileCondition { condition, .. } => {
            render_lead_verb(
                ui,
                if is_while_loop { "While" } else { "If" },
                visual_colors,
            );
            render_condition_editor(
                ui,
                Id::unique((id_namespace, "cond", block_id.0)),
                condition,
                visual_colors,
                0,
            )
        }
        StatementKind::RepeatCount { repeat_count, .. } => {
            render_lead_verb(ui, "Repeat", visual_colors);
            let did_change = ui
                .add(DragValue::new(repeat_count).range(1..=100_000))
                .changed();
            render_muted_word(ui, "times", visual_colors);
            did_change
        }
        _ => false,
    })
}

/// Renders the natural-language sentence and pill inputs for every leaf statement block variant.
pub(crate) fn render_leaf_block_sentence(
    ui: &mut Ui,
    id_namespace: &str,
    block_id: BlockId,
    statement_kind: &mut StatementKind,
    visual_colors: BlockVisualColors,
    custom_blocks: &[CustomBlockDefinition],
    allow_quick_edit: bool,
) -> LeafBlockSentenceOutcome {
    let is_press_down = matches!(
        statement_kind,
        StatementKind::MouseDown { .. } | StatementKind::KeyDown { .. }
    );
    let mut outcome = LeafBlockSentenceOutcome::default();
    outcome.did_edit = with_pill_well_style(ui, visual_colors, |ui| match statement_kind {
        StatementKind::BreakLoop => {
            render_lead_verb(ui, "Break Loop", visual_colors);
            false
        }
        StatementKind::StopScript => {
            render_lead_verb(ui, "Stop Script", visual_colors);
            false
        }
        StatementKind::DoNothing => {
            render_lead_verb(ui, "Do Nothing", visual_colors);
            render_muted_word(ui, "(no action)", visual_colors);
            false
        }
        StatementKind::MouseClick {
            mouse_button,
            hold_duration_ms,
        } => {
            render_lead_verb(ui, "Click", visual_colors);
            let mut did_change =
                render_mouse_button_combo(ui, (id_namespace, "mc", block_id.0), mouse_button);
            did_change |= render_hold_duration_suffix(ui, hold_duration_ms, visual_colors);
            did_change
        }
        StatementKind::MouseDown { mouse_button } | StatementKind::MouseUp { mouse_button } => {
            render_lead_verb(
                ui,
                if is_press_down { "Hold" } else { "Release" },
                visual_colors,
            );
            render_mouse_button_combo(ui, (id_namespace, "mb", block_id.0), mouse_button)
        }
        StatementKind::MouseHoldWhilePressed {
            button,
            watch_input,
        } => {
            render_lead_verb(ui, "Hold", visual_colors);
            let mut did_change =
                render_mouse_button_combo(ui, (id_namespace, "mhw_btn", block_id.0), button);
            render_muted_word(ui, "until", visual_colors);
            did_change |= render_input_trigger_picker(
                ui,
                (id_namespace, "mhw_watch", block_id.0),
                watch_input,
                visual_colors,
            );
            render_muted_word(ui, "released", visual_colors);
            did_change
        }
        StatementKind::MouseMoveRelative {
            delta_x_px,
            delta_y_px,
        } => {
            render_lead_verb(ui, "Move Mouse", visual_colors);
            ui.add_space(4.0);
            render_muted_word(ui, "X", visual_colors);
            let mut did_change = ui
                .add(DragValue::new(delta_x_px).range(-5000..=5000))
                .changed();
            ui.add_space(3.0);
            render_muted_word(ui, "Y", visual_colors);
            did_change |= ui
                .add(DragValue::new(delta_y_px).range(-5000..=5000))
                .changed();
            render_muted_word(ui, "px", visual_colors);
            did_change
        }
        StatementKind::MouseScroll { vertical_steps } => {
            render_lead_verb(ui, "Scroll", visual_colors);
            let did_change = ui
                .add(DragValue::new(vertical_steps).range(-100..=100))
                .changed();
            render_muted_word(ui, "steps", visual_colors);
            did_change
        }
        StatementKind::KeyTap {
            key_code,
            hold_duration_ms,
        } => {
            render_lead_verb(ui, "Tap Key", visual_colors);
            let mut did_change = render_key_code_picker(
                ui,
                (id_namespace, "kt", block_id.0),
                key_code,
                visual_colors,
            );
            did_change |= render_hold_duration_suffix(ui, hold_duration_ms, visual_colors);
            did_change
        }
        StatementKind::KeyDown { key_code } | StatementKind::KeyUp { key_code } => {
            render_lead_verb(
                ui,
                if is_press_down {
                    "Hold Key"
                } else {
                    "Release Key"
                },
                visual_colors,
            );
            render_key_code_picker(
                ui,
                (id_namespace, "kb", block_id.0),
                key_code,
                visual_colors,
            )
        }
        StatementKind::TypeText {
            text,
            keystroke_delay_ms,
            jitter_ms,
        } => {
            render_lead_verb(ui, "Type", visual_colors);
            let mut did_change = render_mono_text_pill(ui, text, TYPE_TEXT_WIDTH_PX, "text");
            ui.add_space(4.0);
            render_muted_word(ui, "delay", visual_colors);
            did_change |= render_u64_pill(ui, keystroke_delay_ms, 0..=10_000);
            render_muted_word(ui, "ms  ±", visual_colors);
            did_change |= render_u64_pill(ui, jitter_ms, 0..=5_000);
            render_muted_word(ui, "ms", visual_colors);
            did_change
        }
        StatementKind::WaitFixed { delay_ms } => {
            render_lead_verb(ui, "Wait", visual_colors);
            let did_change = render_u64_pill(ui, delay_ms, 0..=600_000);
            render_muted_word(ui, "ms", visual_colors);
            did_change
        }
        StatementKind::WaitRandomUniform {
            min_delay_ms,
            max_delay_ms,
        } => {
            render_lead_verb(ui, "Wait", visual_colors);
            let mut did_change = render_u64_pill(ui, min_delay_ms, 0..=600_000);
            render_muted_word(ui, "..", visual_colors);
            did_change |= render_u64_pill(ui, max_delay_ms, 0..=600_000);
            render_muted_word(ui, "ms", visual_colors);
            did_change
        }
        StatementKind::WaitRandomGaussian {
            base_delay_ms,
            min_offset_ms,
            max_offset_ms,
            mean_offset_ms,
            std_dev_ms,
        } => render_gaussian_wait_sentence(
            ui,
            id_namespace,
            block_id,
            base_delay_ms,
            min_offset_ms,
            max_offset_ms,
            mean_offset_ms,
            std_dev_ms,
            visual_colors,
        ),
        StatementKind::SetVariable {
            variable_name,
            value,
        } => render_variable_sentence(ui, "Set", "to", variable_name, value, visual_colors),
        StatementKind::ChangeVariable {
            variable_name,
            delta_value,
        } => render_variable_sentence(
            ui,
            "Change",
            "by",
            variable_name,
            delta_value,
            visual_colors,
        ),
        StatementKind::RunCustomBlock {
            custom_block_id,
            cached_name,
        } => {
            if let Some(requested_id) = render_custom_block_sentence(
                ui,
                custom_block_id,
                cached_name,
                visual_colors,
                custom_blocks,
                allow_quick_edit,
            ) {
                outcome.edit_custom_block_id = Some(requested_id);
            }
            false
        }
        _ => false,
    });
    outcome
}

fn render_custom_block_sentence(
    ui: &mut Ui,
    custom_block_id: &str,
    cached_name: &str,
    visual_colors: BlockVisualColors,
    custom_blocks: &[CustomBlockDefinition],
    allow_quick_edit: bool,
) -> Option<String> {
    let Some(definition) = lookup_custom_block(custom_blocks, custom_block_id) else {
        let fallback_name = if cached_name.trim().is_empty() {
            "Deleted Tile"
        } else {
            cached_name.trim()
        };
        let short_name = truncate_display_text(fallback_name, MAX_CUSTOM_BLOCK_INLINE_NAME_CHARS);
        ui.label(rich_semibold(
            format!("★ {short_name}"),
            BLOCK_LABEL_SIZE_PX,
            STATUS_DANGER_INK,
        ));
        Frame::NONE
            .fill(STATUS_DANGER_SOLID)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
            .inner_margin(Margin::symmetric(7, 2))
            .show(ui, |ui| {
                ui.label(rich_semibold(
                    "Missing Tile",
                    FONT_SIZE_SMALL_PX,
                    Color32::WHITE,
                ));
            });
        return None;
    };

    let short_name = truncate_display_text(&definition.name, MAX_CUSTOM_BLOCK_INLINE_NAME_CHARS);
    let title_response = ui.label(rich_semibold(
        format!("★ {short_name}"),
        BLOCK_LABEL_SIZE_PX,
        visual_colors.text_primary,
    ));
    if !definition.description.trim().is_empty() {
        title_response.on_hover_text(definition.description.trim());
    }

    let step_count = definition.total_block_count();
    let step_label = if step_count == 1 {
        "1 step".to_string()
    } else {
        format!("{step_count} steps")
    };
    Frame::NONE
        .fill(visual_colors.badge_bg)
        .stroke(Stroke::new(1.0, visual_colors.pill_well_border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(rich_mono_bold(
                step_label,
                FONT_SIZE_SMALL_PX,
                visual_colors.text_muted,
            ));
        });

    if !allow_quick_edit {
        return None;
    }

    let edit_button = Button::new(rich_semibold(
        "Edit",
        FONT_SIZE_SMALL_PX,
        visual_colors.text_primary,
    ))
    .fill(visual_colors.pill_well_bg)
    .stroke(Stroke::new(1.0, visual_colors.pill_well_border))
    .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
    .sense(Sense::click_and_drag());

    if ui
        .add(edit_button)
        .on_hover_text("Edit this custom tile definition")
        .clicked()
    {
        return Some(custom_block_id.to_string());
    }
    None
}

fn render_hold_duration_suffix(
    ui: &mut Ui,
    hold_duration_ms: &mut u64,
    visual_colors: BlockVisualColors,
) -> bool {
    ui.add_space(4.0);
    render_muted_word(ui, "hold", visual_colors);
    let did_change = render_u64_pill(ui, hold_duration_ms, 0..=60_000);
    render_muted_word(ui, "ms", visual_colors);
    did_change
}

fn render_lead_verb(ui: &mut Ui, verb_text: &str, visual_colors: BlockVisualColors) {
    ui.label(rich_semibold(
        verb_text,
        BLOCK_LABEL_SIZE_PX,
        visual_colors.text_primary,
    ));
}

pub(super) fn render_muted_word(ui: &mut Ui, word_text: &str, visual_colors: BlockVisualColors) {
    ui.label(rich_semibold(
        word_text,
        FONT_SIZE_SMALL_PX,
        visual_colors.text_muted,
    ));
}

fn render_u64_pill(ui: &mut Ui, value: &mut u64, bounds: RangeInclusive<u64>) -> bool {
    ui.add(DragValue::new(value).range(bounds)).changed()
}

pub(super) fn render_f64_pill(
    ui: &mut Ui,
    value: &mut f64,
    bounds: RangeInclusive<f64>,
    speed: f64,
    max_decimals: usize,
) -> bool {
    ui.add(
        DragValue::new(value)
            .speed(speed)
            .range(bounds)
            .max_decimals(max_decimals),
    )
    .changed()
}

pub(super) fn render_mono_text_pill(
    ui: &mut Ui,
    text: &mut String,
    width_px: f32,
    hint_text: &str,
) -> bool {
    ui.add(
        TextEdit::singleline(text)
            .desired_width(width_px)
            .hint_text(hint_text)
            .font(FontId::new(PILL_TEXT_SIZE_PX, font_family_mono_bold())),
    )
    .changed()
}

#[allow(clippy::too_many_arguments)]
fn render_gaussian_wait_sentence(
    ui: &mut Ui,
    id_namespace: &str,
    block_id: BlockId,
    base_delay_ms: &mut u64,
    min_offset_ms: &mut f64,
    max_offset_ms: &mut f64,
    mean_offset_ms: &mut f64,
    std_dev_ms: &mut f64,
    visual_colors: BlockVisualColors,
) -> bool {
    let mut did_change = false;
    render_lead_verb(ui, "Wait", visual_colors);
    did_change |= render_u64_pill(ui, base_delay_ms, 0..=600_000);
    render_muted_word(ui, "ms  + Gaussian", visual_colors);
    did_change |= render_f64_pill(ui, min_offset_ms, 0.0..=600_000.0, 0.5, 1);
    render_muted_word(ui, "..", visual_colors);
    did_change |= render_f64_pill(ui, max_offset_ms, 0.0..=600_000.0, 0.5, 1);
    render_muted_word(ui, "ms", visual_colors);

    let is_valid =
        GaussianRangeMs::new(*min_offset_ms, *max_offset_ms, *mean_offset_ms, *std_dev_ms).is_ok();
    let summary_label = format!(
        "μ {} / σ {} ▾",
        format_compact_number(*mean_offset_ms),
        format_compact_number(*std_dev_ms)
    );
    let border_color = if is_valid {
        visual_colors.pill_well_border
    } else {
        STATUS_DANGER_INK
    };
    let pill = Button::new(rich_mono_bold(
        summary_label,
        PILL_TEXT_SIZE_PX,
        visual_colors.text_primary,
    ))
    .fill(visual_colors.badge_bg)
    .stroke(Stroke::new(1.0, border_color))
    .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
    .sense(Sense::click_and_drag());
    let trigger_response = ui
        .add(pill)
        .on_hover_text("Tune Gaussian mean offset and standard deviation");

    let popup_id = Id::unique((id_namespace, "gaussian_tune_popup", block_id.0));
    Popup::from_toggle_button_response(&trigger_response)
        .id(popup_id)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .width(GAUSSIAN_POPUP_WIDTH_PX)
        .show(|ui| {
            did_change |= render_gaussian_distribution_popover(
                ui,
                *base_delay_ms,
                *min_offset_ms,
                *max_offset_ms,
                mean_offset_ms,
                std_dev_ms,
            );
        });
    did_change
}

fn render_gaussian_distribution_popover(
    ui: &mut Ui,
    base_delay_ms: u64,
    min_offset_ms: f64,
    max_offset_ms: f64,
    mean_offset_ms: &mut f64,
    std_dev_ms: &mut f64,
) -> bool {
    ui.style_mut().drag_value_text_style = TextStyle::Monospace;
    let mut did_change = false;
    ui.label(rich_semibold(
        "Gaussian Distribution",
        FONT_SIZE_BODY_PX,
        TEXT_PRIMARY,
    ));
    ui.add_space(4.0);
    did_change |= render_popover_ms_row(ui, "Mean Offset", mean_offset_ms, 0.0..=600_000.0, 0.5, 1);
    did_change |= render_popover_ms_row(ui, "Std-Dev (σ)", std_dev_ms, 0.0..=100_000.0, 0.1, 2);
    ui.add_space(6.0);

    match GaussianRangeMs::new(min_offset_ms, max_offset_ms, *mean_offset_ms, *std_dev_ms) {
        Ok(_) => {
            let base_ms = base_delay_ms as f64;
            let text = format!(
                "Total: {}..{} ms (μ {} ms)",
                format_compact_number(base_ms + min_offset_ms),
                format_compact_number(base_ms + max_offset_ms),
                format_compact_number(base_ms + *mean_offset_ms),
            );
            ui.label(rich_mono_bold(text, FONT_SIZE_SMALL_PX, STATUS_RUNNING_INK));
        }
        Err(validation_error) => {
            ui.label(rich_semibold(
                validation_error.to_string(),
                FONT_SIZE_SMALL_PX,
                STATUS_DANGER_INK,
            ));
            if min_offset_ms <= max_offset_ms {
                ui.add_space(4.0);
                let center_button = Button::new(rich_semibold(
                    "Auto-Center Mean",
                    FONT_SIZE_SMALL_PX,
                    TEXT_PRIMARY,
                ))
                .fill(SURFACE_HOVER)
                .stroke(Stroke::new(1.0, BORDER_STRONG))
                .corner_radius(CornerRadius::same(CORNER_RADIUS_SM_PX));
                if ui.add(center_button).clicked() {
                    *mean_offset_ms = (min_offset_ms + max_offset_ms) * 0.5;
                    did_change = true;
                }
            }
        }
    }
    did_change
}

fn render_popover_ms_row(
    ui: &mut Ui,
    label: &str,
    value: &mut f64,
    bounds: RangeInclusive<f64>,
    speed: f64,
    max_decimals: usize,
) -> bool {
    let mut did_change = false;
    ui.horizontal(|ui| {
        ui.label(rich_semibold(label, FONT_SIZE_SMALL_PX, TEXT_MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(rich_semibold("ms", FONT_SIZE_SMALL_PX, TEXT_MUTED));
            did_change = render_f64_pill(ui, value, bounds, speed, max_decimals);
        });
    });
    did_change
}

fn render_variable_sentence(
    ui: &mut Ui,
    lead_verb: &str,
    preposition: &str,
    variable_name: &mut String,
    numeric_value: &mut f64,
    visual_colors: BlockVisualColors,
) -> bool {
    render_lead_verb(ui, lead_verb, visual_colors);
    let mut did_change = render_mono_text_pill(ui, variable_name, VARIABLE_INPUT_WIDTH_PX, "name");
    render_muted_word(ui, preposition, visual_colors);
    did_change |= ui
        .add(DragValue::new(numeric_value).speed(0.5).max_decimals(2))
        .changed();
    did_change
}

fn format_compact_number(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if (rounded - rounded.round()).abs() < 0.01 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    }
}
