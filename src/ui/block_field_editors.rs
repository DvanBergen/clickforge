use std::fmt::Debug;
use std::hash::Hash;

use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};
use egui::{ComboBox, DragValue, Id, RichText, TextEdit, Ui};

use crate::models::{BlockId, ComparisonOperator, ConditionExpr, StatementKind};
use crate::ui::theme::{ACCENT_PRIMARY, FONT_SIZE_SMALL_PX, TEXT_MUTED};

const CONDITION_TEXT_WIDTH_PX: f32 = 96.0;
const TYPE_TEXT_WIDTH_PX: f32 = 180.0;
const MAX_INLINE_CONDITION_DEPTH: usize = 2;

/// Renders the inline condition or iteration counter inside a C-block container header.
pub(crate) fn render_container_header_fields(
    ui: &mut Ui,
    block_id: BlockId,
    statement_kind: &mut StatementKind,
) -> bool {
    match statement_kind {
        StatementKind::IfThen { condition, .. }
        | StatementKind::IfThenElse { condition, .. }
        | StatementKind::WhileCondition { condition, .. } => {
            render_condition_editor(ui, Id::unique(("cond", block_id.0)), condition, 0)
        }
        StatementKind::RepeatCount { repeat_count, .. } => ui
            .add(
                DragValue::new(repeat_count)
                    .range(1..=100_000)
                    .suffix(" times"),
            )
            .changed(),
        _ => false,
    }
}

/// Renders inline editors for every leaf statement block variant.
pub(crate) fn render_leaf_inline_fields(
    ui: &mut Ui,
    block_id: BlockId,
    statement_kind: &mut StatementKind,
) -> bool {
    match statement_kind {
        StatementKind::BreakLoop | StatementKind::StopScript => false,
        StatementKind::MouseClick {
            mouse_button,
            hold_duration_ms,
        } => {
            let mut did_change = render_mouse_button_combo(ui, ("mc", block_id.0), mouse_button);
            ui.label(
                RichText::new("hold")
                    .size(FONT_SIZE_SMALL_PX)
                    .color(TEXT_MUTED),
            );
            did_change |= ui
                .add(
                    DragValue::new(hold_duration_ms)
                        .range(0..=60_000)
                        .suffix(" ms"),
                )
                .changed();
            did_change
        }
        StatementKind::MouseDown { mouse_button } | StatementKind::MouseUp { mouse_button } => {
            render_mouse_button_combo(ui, ("mb", block_id.0), mouse_button)
        }
        StatementKind::MouseMoveRelative {
            delta_x_px,
            delta_y_px,
        } => {
            let mut did_change = false;
            ui.label(
                RichText::new("dx")
                    .size(FONT_SIZE_SMALL_PX)
                    .color(TEXT_MUTED),
            );
            did_change |= ui
                .add(DragValue::new(delta_x_px).range(-5000..=5000).suffix(" px"))
                .changed();
            ui.label(
                RichText::new("dy")
                    .size(FONT_SIZE_SMALL_PX)
                    .color(TEXT_MUTED),
            );
            did_change |= ui
                .add(DragValue::new(delta_y_px).range(-5000..=5000).suffix(" px"))
                .changed();
            did_change
        }
        StatementKind::MouseScroll { vertical_steps } => ui
            .add(
                DragValue::new(vertical_steps)
                    .range(-100..=100)
                    .suffix(" steps"),
            )
            .changed(),
        StatementKind::KeyTap {
            key_code,
            hold_duration_ms,
        } => {
            let mut did_change = render_key_code_combo(ui, ("kt", block_id.0), key_code);
            ui.label(
                RichText::new("hold")
                    .size(FONT_SIZE_SMALL_PX)
                    .color(TEXT_MUTED),
            );
            did_change |= ui
                .add(
                    DragValue::new(hold_duration_ms)
                        .range(0..=60_000)
                        .suffix(" ms"),
                )
                .changed();
            did_change
        }
        StatementKind::KeyDown { key_code } | StatementKind::KeyUp { key_code } => {
            render_key_code_combo(ui, ("kd", block_id.0), key_code)
        }
        StatementKind::TypeText {
            text,
            keystroke_delay_ms,
            jitter_ms,
        } => render_type_text_fields(ui, text, keystroke_delay_ms, jitter_ms),
        StatementKind::WaitFixed { delay_ms } => ui
            .add(DragValue::new(delay_ms).range(0..=600_000).suffix(" ms"))
            .changed(),
        StatementKind::WaitRandomUniform {
            min_delay_ms,
            max_delay_ms,
        } => {
            let mut did_change = ui
                .add(
                    DragValue::new(min_delay_ms)
                        .range(0..=600_000)
                        .suffix(" ms"),
                )
                .changed();
            ui.label(
                RichText::new("to")
                    .size(FONT_SIZE_SMALL_PX)
                    .color(TEXT_MUTED),
            );
            did_change |= ui
                .add(
                    DragValue::new(max_delay_ms)
                        .range(0..=600_000)
                        .suffix(" ms"),
                )
                .changed();
            did_change
        }
        StatementKind::WaitRandomGaussian {
            base_delay_ms,
            min_offset_ms,
            max_offset_ms,
            mean_offset_ms,
            std_dev_ms,
        } => render_gaussian_wait_fields(
            ui,
            base_delay_ms,
            min_offset_ms,
            max_offset_ms,
            mean_offset_ms,
            std_dev_ms,
        ),
        StatementKind::SetVariable {
            variable_name,
            value,
        } => render_variable_fields(ui, variable_name, "=", value),
        StatementKind::ChangeVariable {
            variable_name,
            delta_value,
        } => render_variable_fields(ui, variable_name, "+=", delta_value),
        _ => false,
    }
}

fn render_type_text_fields(
    ui: &mut Ui,
    text: &mut String,
    keystroke_delay_ms: &mut u64,
    jitter_ms: &mut u64,
) -> bool {
    let mut did_change = ui
        .add(
            TextEdit::singleline(text)
                .desired_width(TYPE_TEXT_WIDTH_PX)
                .hint_text("ASCII text"),
        )
        .changed();
    ui.label(
        RichText::new("delay")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    );
    did_change |= ui
        .add(
            DragValue::new(keystroke_delay_ms)
                .range(0..=10_000)
                .suffix(" ms"),
        )
        .changed();
    ui.label(
        RichText::new("+/-")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    );
    did_change |= ui
        .add(DragValue::new(jitter_ms).range(0..=5_000).suffix(" ms"))
        .changed();
    did_change
}

fn render_gaussian_wait_fields(
    ui: &mut Ui,
    base_delay_ms: &mut u64,
    min_offset_ms: &mut f64,
    max_offset_ms: &mut f64,
    mean_offset_ms: &mut f64,
    std_dev_ms: &mut f64,
) -> bool {
    let mut did_change = false;
    ui.label(
        RichText::new("base")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    );
    did_change |= ui
        .add(
            DragValue::new(base_delay_ms)
                .range(0..=600_000)
                .suffix(" ms"),
        )
        .changed();
    ui.label(
        RichText::new("+ G[")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    );
    did_change |= ui
        .add(
            DragValue::new(min_offset_ms)
                .speed(0.5)
                .range(0.0..=600_000.0)
                .suffix(" ms"),
        )
        .changed();
    ui.label(
        RichText::new("..")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    );
    did_change |= ui
        .add(
            DragValue::new(max_offset_ms)
                .speed(0.5)
                .range(0.0..=600_000.0)
                .suffix(" ms"),
        )
        .changed();
    ui.label(
        RichText::new("mean")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    );
    did_change |= ui
        .add(
            DragValue::new(mean_offset_ms)
                .speed(0.5)
                .range(0.0..=600_000.0)
                .suffix(" ms"),
        )
        .changed();
    ui.label(
        RichText::new("sd")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    );
    did_change |= ui
        .add(
            DragValue::new(std_dev_ms)
                .speed(0.25)
                .range(0.0..=100_000.0)
                .suffix(" ms"),
        )
        .changed();
    ui.label(
        RichText::new("]")
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    );
    did_change
}

fn render_variable_fields(
    ui: &mut Ui,
    variable_name: &mut String,
    operator_text: &str,
    numeric_value: &mut f64,
) -> bool {
    let mut did_change = ui
        .add(
            TextEdit::singleline(variable_name)
                .desired_width(CONDITION_TEXT_WIDTH_PX)
                .hint_text("name"),
        )
        .changed();
    ui.label(
        RichText::new(operator_text)
            .size(FONT_SIZE_SMALL_PX)
            .color(TEXT_MUTED),
    );
    did_change |= ui.add(DragValue::new(numeric_value).speed(0.5)).changed();
    did_change
}

fn render_condition_editor(
    ui: &mut Ui,
    widget_id: Id,
    condition: &mut ConditionExpr,
    nesting_depth: usize,
) -> bool {
    let mut did_change = render_condition_kind_combo(ui, widget_id, condition, nesting_depth);

    match condition {
        ConditionExpr::IsInputPressed { input } => {
            did_change |= render_input_trigger_picker(ui, widget_id.with("cond_input"), input);
        }
        ConditionExpr::RandomChance { chance_percent } => {
            did_change |= ui
                .add(
                    DragValue::new(chance_percent)
                        .speed(1.0)
                        .range(0.0..=100.0)
                        .suffix(" %"),
                )
                .changed();
        }
        ConditionExpr::CompareVariable {
            variable_name,
            operator,
            target_value,
        } => {
            did_change |= ui
                .add(
                    TextEdit::singleline(variable_name)
                        .desired_width(CONDITION_TEXT_WIDTH_PX)
                        .hint_text("var"),
                )
                .changed();
            did_change |= render_comparison_operator_combo(ui, widget_id.with("cmp_op"), operator);
            did_change |= ui.add(DragValue::new(target_value).speed(0.5)).changed();
        }
        ConditionExpr::Not { inner_condition } => {
            did_change |= render_condition_editor(
                ui,
                widget_id.with("not"),
                inner_condition,
                nesting_depth.saturating_add(1),
            );
        }
        ConditionExpr::And {
            left_condition,
            right_condition,
        } => {
            did_change |= render_condition_editor(
                ui,
                widget_id.with("and_l"),
                left_condition,
                nesting_depth.saturating_add(1),
            );
            ui.label(
                RichText::new("AND")
                    .size(FONT_SIZE_SMALL_PX)
                    .color(ACCENT_PRIMARY),
            );
            did_change |= render_condition_editor(
                ui,
                widget_id.with("and_r"),
                right_condition,
                nesting_depth.saturating_add(1),
            );
        }
        ConditionExpr::Or {
            left_condition,
            right_condition,
        } => {
            did_change |= render_condition_editor(
                ui,
                widget_id.with("or_l"),
                left_condition,
                nesting_depth.saturating_add(1),
            );
            ui.label(
                RichText::new("OR")
                    .size(FONT_SIZE_SMALL_PX)
                    .color(ACCENT_PRIMARY),
            );
            did_change |= render_condition_editor(
                ui,
                widget_id.with("or_r"),
                right_condition,
                nesting_depth.saturating_add(1),
            );
        }
    }

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
        .width(126.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(
                    matches!(condition, ConditionExpr::IsInputPressed { .. }),
                    "Input Pressed",
                )
                .clicked()
            {
                *condition = ConditionExpr::IsInputPressed {
                    input: InputTrigger::Mouse(MouseButton::Left),
                };
                did_change = true;
            }
            if ui
                .selectable_label(
                    matches!(condition, ConditionExpr::RandomChance { .. }),
                    "Random Chance %",
                )
                .clicked()
            {
                *condition = ConditionExpr::RandomChance {
                    chance_percent: 50.0,
                };
                did_change = true;
            }
            if ui
                .selectable_label(
                    matches!(condition, ConditionExpr::CompareVariable { .. }),
                    "Compare Variable",
                )
                .clicked()
            {
                *condition = ConditionExpr::CompareVariable {
                    variable_name: "counter".to_string(),
                    operator: ComparisonOperator::LessThan,
                    target_value: 10.0,
                };
                did_change = true;
            }
            if nesting_depth < MAX_INLINE_CONDITION_DEPTH
                && ui
                    .selectable_label(matches!(condition, ConditionExpr::Not { .. }), "NOT (...)")
                    .clicked()
            {
                *condition = ConditionExpr::Not {
                    inner_condition: Box::new(ConditionExpr::IsInputPressed {
                        input: InputTrigger::Mouse(MouseButton::Right),
                    }),
                };
                did_change = true;
            }
        });
    did_change
}

fn condition_variant_label(condition: &ConditionExpr) -> &'static str {
    match condition {
        ConditionExpr::IsInputPressed { .. } => "Input Pressed",
        ConditionExpr::RandomChance { .. } => "Random Chance %",
        ConditionExpr::CompareVariable { .. } => "Compare Variable",
        ConditionExpr::Not { .. } => "NOT (...)",
        ConditionExpr::And { .. } => "AND",
        ConditionExpr::Or { .. } => "OR",
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
        .width(52.0)
        .show_ui(ui, |ui| {
            for candidate in ALL_OPERATORS {
                if ui
                    .selectable_value(operator, candidate, candidate.display_symbol())
                    .changed()
                {
                    did_change = true;
                }
            }
        });
    did_change
}

/// Renders an inline Mouse/Key input trigger selector shared by trigger hats and conditions.
pub(crate) fn render_input_trigger_picker<SaltType: Hash + Debug + Copy>(
    ui: &mut Ui,
    id_salt: SaltType,
    trigger_input: &mut InputTrigger,
) -> bool {
    let mut did_change = false;
    let is_mouse = matches!(trigger_input, InputTrigger::Mouse(_));

    ComboBox::from_id_salt((id_salt, "trigger_kind"))
        .selected_text(if is_mouse { "Mouse" } else { "Key" })
        .width(68.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(is_mouse, "Mouse").clicked() && !is_mouse {
                *trigger_input = InputTrigger::Mouse(MouseButton::Left);
                did_change = true;
            }
            if ui.selectable_label(!is_mouse, "Key").clicked() && is_mouse {
                *trigger_input = InputTrigger::Key(KeyCode::E);
                did_change = true;
            }
        });

    match trigger_input {
        InputTrigger::Mouse(mouse_button) => {
            did_change |= render_mouse_button_combo(ui, (id_salt, "mouse_button"), mouse_button);
        }
        InputTrigger::Key(key_code) => {
            did_change |= render_key_code_combo(ui, (id_salt, "key_code"), key_code);
        }
    }
    did_change
}

/// Renders a dropdown selecting one of the supported `MouseButton` variants.
pub(crate) fn render_mouse_button_combo<SaltType: Hash + Debug>(
    ui: &mut Ui,
    id_salt: SaltType,
    mouse_button: &mut MouseButton,
) -> bool {
    let mut did_change = false;
    ComboBox::from_id_salt(id_salt)
        .selected_text(mouse_button.display_label())
        .width(122.0)
        .show_ui(ui, |ui| {
            for candidate_button in MouseButton::all_buttons() {
                if ui
                    .selectable_value(
                        mouse_button,
                        *candidate_button,
                        candidate_button.display_label(),
                    )
                    .changed()
                {
                    did_change = true;
                }
            }
        });
    did_change
}

/// Renders a dropdown selecting one of the supported `KeyCode` variants.
pub(crate) fn render_key_code_combo<SaltType: Hash + Debug>(
    ui: &mut Ui,
    id_salt: SaltType,
    key_code: &mut KeyCode,
) -> bool {
    let mut did_change = false;
    ComboBox::from_id_salt(id_salt)
        .selected_text(key_code.display_label())
        .width(106.0)
        .show_ui(ui, |ui| {
            for candidate_key in KeyCode::all_keys() {
                if ui
                    .selectable_value(key_code, *candidate_key, candidate_key.display_label())
                    .changed()
                {
                    did_change = true;
                }
            }
        });
    did_change
}
