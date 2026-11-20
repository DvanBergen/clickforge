mod key_picker;

use std::fmt::Debug;
use std::hash::Hash;
use std::ops::RangeInclusive;

use clickforge_sdk::{GaussianRangeMs, InputTrigger, MouseButton};
use egui::{
    Button, Color32, ComboBox, CornerRadius, DragValue, FontId, Frame, Id, Margin, Popup,
    PopupCloseBehavior, Sense, Stroke, TextEdit, TextStyle, Ui, style::WidgetVisuals, vec2,
};

use crate::models::{BlockId, ComparisonOperator, ConditionExpr, StatementKind};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BORDER_STRONG, BlockVisualColors, CORNER_RADIUS_PILL_PX,
    CORNER_RADIUS_SM_PX, FONT_SIZE_BODY_PX, FONT_SIZE_SMALL_PX, STATUS_DANGER_INK,
    STATUS_RUNNING_INK, SURFACE_HOVER, TEXT_MUTED, TEXT_PRIMARY, font_family_mono_bold,
    rich_mono_bold, rich_semibold,
};

pub(crate) use key_picker::render_input_trigger_picker;

use key_picker::{render_key_code_picker, render_mouse_button_combo};

const VARIABLE_INPUT_WIDTH_PX: f32 = 88.0;
const TYPE_TEXT_WIDTH_PX: f32 = 148.0;
const PILL_TEXT_SIZE_PX: f32 = 12.5;
const BLOCK_LABEL_SIZE_PX: f32 = 13.5;
const MAX_INLINE_CONDITION_DEPTH: usize = 2;
const GAUSSIAN_POPUP_WIDTH_PX: f32 = 256.0;

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

/// Renders the natural-language header sentence inside a C-block container.
pub(crate) fn render_container_block_sentence(
    ui: &mut Ui,
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
                Id::unique(("cond", block_id.0)),
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
    block_id: BlockId,
    statement_kind: &mut StatementKind,
    visual_colors: BlockVisualColors,
) -> bool {
    let is_press_down = matches!(
        statement_kind,
        StatementKind::MouseDown { .. } | StatementKind::KeyDown { .. }
    );
    with_pill_well_style(ui, visual_colors, |ui| match statement_kind {
        StatementKind::BreakLoop => {
            render_lead_verb(ui, "Break Loop", visual_colors);
            false
        }
        StatementKind::StopScript => {
            render_lead_verb(ui, "Stop Script", visual_colors);
            false
        }
        StatementKind::MouseClick {
            mouse_button,
            hold_duration_ms,
        } => {
            render_lead_verb(ui, "Click", visual_colors);
            let mut did_change = render_mouse_button_combo(ui, ("mc", block_id.0), mouse_button);
            did_change |= render_hold_duration_suffix(ui, hold_duration_ms, visual_colors);
            did_change
        }
        StatementKind::MouseDown { mouse_button } | StatementKind::MouseUp { mouse_button } => {
            render_lead_verb(
                ui,
                if is_press_down { "Hold" } else { "Release" },
                visual_colors,
            );
            render_mouse_button_combo(ui, ("mb", block_id.0), mouse_button)
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
            let mut did_change =
                render_key_code_picker(ui, ("kt", block_id.0), key_code, visual_colors);
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
            render_key_code_picker(ui, ("kb", block_id.0), key_code, visual_colors)
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
        _ => false,
    })
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

fn render_muted_word(ui: &mut Ui, word_text: &str, visual_colors: BlockVisualColors) {
    ui.label(rich_semibold(
        word_text,
        FONT_SIZE_SMALL_PX,
        visual_colors.text_muted,
    ));
}

fn render_u64_pill(ui: &mut Ui, value: &mut u64, bounds: RangeInclusive<u64>) -> bool {
    ui.add(DragValue::new(value).range(bounds)).changed()
}

fn render_f64_pill(
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

fn render_mono_text_pill(ui: &mut Ui, text: &mut String, width_px: f32, hint_text: &str) -> bool {
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

    let popup_id = Id::unique(("gaussian_tune_popup", block_id.0));
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

fn render_condition_editor(
    ui: &mut Ui,
    widget_id: Id,
    condition: &mut ConditionExpr,
    visual_colors: BlockVisualColors,
    nesting_depth: usize,
) -> bool {
    let mut did_change = render_condition_kind_combo(ui, widget_id, condition, nesting_depth);
    let next_depth = nesting_depth.saturating_add(1);
    let is_and_condition = matches!(condition, ConditionExpr::And { .. });
    match condition {
        ConditionExpr::IsInputPressed { input } => {
            did_change |=
                render_input_trigger_picker(ui, widget_id.with("in"), input, visual_colors);
        }
        ConditionExpr::RandomChance { chance_percent } => {
            did_change |= render_f64_pill(ui, chance_percent, 0.0..=100.0, 1.0, 1);
            render_muted_word(ui, "%", visual_colors);
        }
        ConditionExpr::CompareVariable {
            variable_name,
            operator,
            target_value,
        } => {
            did_change |= render_mono_text_pill(ui, variable_name, VARIABLE_INPUT_WIDTH_PX, "var");
            did_change |= render_comparison_operator_combo(ui, widget_id.with("op"), operator);
            did_change |= ui
                .add(DragValue::new(target_value).speed(0.5).max_decimals(2))
                .changed();
        }
        ConditionExpr::Not { inner_condition } => {
            did_change |= render_nested_condition_chip(
                ui,
                widget_id.with("not"),
                inner_condition,
                visual_colors,
                next_depth,
            );
        }
        ConditionExpr::And {
            left_condition,
            right_condition,
        }
        | ConditionExpr::Or {
            left_condition,
            right_condition,
        } => {
            let connector = if is_and_condition { "AND" } else { "OR" };
            did_change |= render_nested_condition_chip(
                ui,
                widget_id.with((connector, "l")),
                left_condition,
                visual_colors,
                next_depth,
            );
            Frame::NONE
                .fill(ACCENT_PRIMARY_TINT)
                .stroke(Stroke::new(1.0, ACCENT_PRIMARY))
                .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
                .inner_margin(Margin::symmetric(6, 2))
                .show(ui, |ui| {
                    ui.label(rich_mono_bold(
                        connector,
                        FONT_SIZE_SMALL_PX,
                        ACCENT_PRIMARY,
                    ));
                });
            did_change |= render_nested_condition_chip(
                ui,
                widget_id.with((connector, "r")),
                right_condition,
                visual_colors,
                next_depth,
            );
        }
    }
    did_change
}

fn render_nested_condition_chip(
    ui: &mut Ui,
    widget_id: Id,
    condition: &mut ConditionExpr,
    visual_colors: BlockVisualColors,
    nesting_depth: usize,
) -> bool {
    let mut did_change = false;
    Frame::NONE
        .fill(visual_colors.badge_bg)
        .stroke(Stroke::new(1.0, visual_colors.pill_well_border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS_PILL_PX))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                did_change =
                    render_condition_editor(ui, widget_id, condition, visual_colors, nesting_depth);
            });
        });
    did_change
}

fn render_condition_kind_combo(
    ui: &mut Ui,
    widget_id: Id,
    condition: &mut ConditionExpr,
    nesting_depth: usize,
) -> bool {
    let mut did_change = false;
    ComboBox::from_id_salt(widget_id.with(("cond_kind", nesting_depth)))
        .selected_text(condition_variant_label(condition))
        .width(124.0)
        .show_ui(ui, |ui| {
            let is_input = matches!(condition, ConditionExpr::IsInputPressed { .. });
            if ui.selectable_label(is_input, "Input Pressed").clicked() {
                *condition = ConditionExpr::IsInputPressed {
                    input: InputTrigger::Mouse(MouseButton::Left),
                };
                did_change = true;
            }
            let is_chance = matches!(condition, ConditionExpr::RandomChance { .. });
            if ui.selectable_label(is_chance, "Random Chance %").clicked() {
                *condition = ConditionExpr::RandomChance {
                    chance_percent: 50.0,
                };
                did_change = true;
            }
            let is_compare = matches!(condition, ConditionExpr::CompareVariable { .. });
            if ui
                .selectable_label(is_compare, "Compare Variable")
                .clicked()
            {
                *condition = ConditionExpr::CompareVariable {
                    variable_name: "counter".to_string(),
                    operator: ComparisonOperator::LessThan,
                    target_value: 10.0,
                };
                did_change = true;
            }
            if nesting_depth < MAX_INLINE_CONDITION_DEPTH {
                ui.separator();
                did_change |= render_compound_condition_options(ui, condition);
            }
        });
    did_change
}

fn render_compound_condition_options(ui: &mut Ui, condition: &mut ConditionExpr) -> bool {
    let mut did_change = false;
    let is_not = matches!(condition, ConditionExpr::Not { .. });
    if ui.selectable_label(is_not, "NOT (...)").clicked() && !is_not {
        *condition = ConditionExpr::Not {
            inner_condition: Box::new(condition.clone()),
        };
        did_change = true;
    }
    let is_and = matches!(condition, ConditionExpr::And { .. });
    if ui.selectable_label(is_and, "AND (A & B)").clicked() && !is_and {
        *condition = ConditionExpr::And {
            left_condition: Box::new(extract_left_or_self(condition)),
            right_condition: Box::new(ConditionExpr::IsInputPressed {
                input: InputTrigger::Mouse(MouseButton::Right),
            }),
        };
        did_change = true;
    }
    let is_or = matches!(condition, ConditionExpr::Or { .. });
    if ui.selectable_label(is_or, "OR (A | B)").clicked() && !is_or {
        *condition = ConditionExpr::Or {
            left_condition: Box::new(extract_left_or_self(condition)),
            right_condition: Box::new(ConditionExpr::RandomChance {
                chance_percent: 50.0,
            }),
        };
        did_change = true;
    }
    did_change
}

fn extract_left_or_self(condition: &ConditionExpr) -> ConditionExpr {
    match condition {
        ConditionExpr::And { left_condition, .. } | ConditionExpr::Or { left_condition, .. } => {
            (**left_condition).clone()
        }
        other => other.clone(),
    }
}

fn condition_variant_label(condition: &ConditionExpr) -> &'static str {
    match condition {
        ConditionExpr::IsInputPressed { .. } => "Input Pressed",
        ConditionExpr::RandomChance { .. } => "Random Chance %",
        ConditionExpr::CompareVariable { .. } => "Compare Variable",
        ConditionExpr::Not { .. } => "NOT",
        ConditionExpr::And { .. } => "All (AND)",
        ConditionExpr::Or { .. } => "Any (OR)",
    }
}

fn render_comparison_operator_combo<SaltType: Hash + Debug>(
    ui: &mut Ui,
    id_salt: SaltType,
    operator: &mut ComparisonOperator,
) -> bool {
    const ALL_OPERATORS: [ComparisonOperator; 6] = [
        ComparisonOperator::Equal,
        ComparisonOperator::NotEqual,
        ComparisonOperator::LessThan,
        ComparisonOperator::LessOrEqual,
        ComparisonOperator::GreaterThan,
        ComparisonOperator::GreaterOrEqual,
    ];
    let mut did_change = false;
    ComboBox::from_id_salt(id_salt)
        .selected_text(operator.display_symbol())
        .width(50.0)
        .show_ui(ui, |ui| {
            for candidate in ALL_OPERATORS {
                did_change |= ui
                    .selectable_value(operator, candidate, candidate.display_symbol())
                    .changed();
            }
        });
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
