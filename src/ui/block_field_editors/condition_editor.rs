use std::fmt::Debug;
use std::hash::Hash;

use clickforge_sdk::{InputTrigger, MouseButton};
use egui::{ComboBox, CornerRadius, DragValue, Frame, Id, Margin, Stroke, Ui};

use crate::models::{ComparisonOperator, ConditionExpr};
use crate::ui::block_field_editors::key_picker::render_input_trigger_picker;
use crate::ui::block_field_editors::{
    VARIABLE_INPUT_WIDTH_PX, render_f64_pill, render_mono_text_pill, render_muted_word,
};
use crate::ui::theme::{
    ACCENT_PRIMARY, ACCENT_PRIMARY_TINT, BlockVisualColors, CORNER_RADIUS_PILL_PX,
    FONT_SIZE_SMALL_PX, rich_mono_bold,
};

const MAX_INLINE_CONDITION_DEPTH: usize = 2;

/// Renders an inline editable boolean condition expression tree for C-block headers.
pub(super) fn render_condition_editor(
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
