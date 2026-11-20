use clickforge_sdk::InputTrigger;

use crate::models::{
    ComparisonOperator, ConditionExpr, ScriptDocument, StatementBlock, StatementKind, TriggerBlock,
    VisualProgram,
};

const IDLE_POLL_DELAY_MS: u64 = 4;
const ANTI_SPIN_YIELD_DELAY_MS: u64 = 1;
const INDENT_WIDTH_SPACES: usize = 4;

#[derive(Default)]
struct ProgramFeatureFlags {
    uses_mouse_button: bool,
    uses_key_code: bool,
    uses_gaussian_range: bool,
    uses_variables: bool,
    mutates_variables: bool,
    uses_random_chance: bool,
}

/// Translates any Visual Block script into compiling native Rust using `clickforge_sdk`.
pub fn generate_rust_source_from_script(script: &ScriptDocument) -> String {
    let program = &script.visual_program;
    let feature_flags = inspect_program_features(program);
    let mut output_lines = Vec::new();

    emit_imports(&feature_flags, &mut output_lines);
    output_lines.push(String::new());
    output_lines.push(format!(
        "// Exported from Visual Blocks script: {}",
        sanitize_single_line_comment(&script.name)
    ));
    output_lines.push(
        "pub fn run_script(script_context: &mut ScriptContext) -> ScriptResult<()> {".to_string(),
    );

    if feature_flags.uses_variables {
        let mutability_keyword = if feature_flags.mutates_variables {
            "let mut"
        } else {
            "let"
        };
        output_lines.push(format!(
            "    {mutability_keyword} script_variables: HashMap<&str, f64> = HashMap::new();"
        ));
    }

    emit_trigger_body(program, &mut output_lines);

    output_lines.push("    Ok(())".to_string());
    output_lines.push("}".to_string());
    output_lines.join("\n")
}

fn emit_imports(feature_flags: &ProgramFeatureFlags, output_lines: &mut Vec<String>) {
    if feature_flags.uses_variables {
        output_lines.push("use std::collections::HashMap;".to_string());
        output_lines.push(String::new());
    }

    let mut sdk_items = Vec::new();
    if feature_flags.uses_gaussian_range {
        sdk_items.push("GaussianRangeMs");
    }
    if feature_flags.uses_key_code {
        sdk_items.push("KeyCode");
    }
    if feature_flags.uses_mouse_button {
        sdk_items.push("MouseButton");
    }
    sdk_items.push("ScriptContext");
    sdk_items.push("ScriptResult");

    output_lines.push(format!("use clickforge_sdk::{{{}}};", sdk_items.join(", ")));
    if feature_flags.uses_random_chance {
        output_lines.push("use rand::RngExt;".to_string());
    }
}

fn emit_trigger_body(program: &VisualProgram, output_lines: &mut Vec<String>) {
    let body_has_wait = slice_contains_wait_action(&program.statements);

    match program.trigger {
        TriggerBlock::WhileInputHeld { trigger_input } => {
            let input_expr = format_input_pressed_expr(trigger_input);
            output_lines.push("    while !script_context.should_stop() {".to_string());
            output_lines.push(format!("        if {input_expr} {{"));
            emit_statement_slice(&program.statements, 3, output_lines);
            if !body_has_wait {
                output_lines.push(format!(
                    "{}script_context.wait_fixed_ms({ANTI_SPIN_YIELD_DELAY_MS})?;",
                    indent_prefix(3)
                ));
            }
            output_lines.push("        } else {".to_string());
            output_lines.push(format!(
                "            script_context.wait_fixed_ms({IDLE_POLL_DELAY_MS})?;"
            ));
            output_lines.push("        }".to_string());
            output_lines.push("    }".to_string());
        }
        TriggerBlock::OnInputToggle { trigger_input } => {
            let input_expr = format_input_pressed_expr(trigger_input);
            output_lines.push("    let mut is_toggle_active = false;".to_string());
            output_lines.push("    let mut was_trigger_pressed = false;".to_string());
            output_lines.push("    while !script_context.should_stop() {".to_string());
            output_lines.push(format!("        let is_trigger_pressed = {input_expr};"));
            output_lines
                .push("        if is_trigger_pressed && !was_trigger_pressed {".to_string());
            output_lines.push("            is_toggle_active = !is_toggle_active;".to_string());
            output_lines.push("        }".to_string());
            output_lines.push("        was_trigger_pressed = is_trigger_pressed;".to_string());
            output_lines.push("        if is_toggle_active {".to_string());
            emit_statement_slice(&program.statements, 3, output_lines);
            if !body_has_wait {
                output_lines.push(format!(
                    "{}script_context.wait_fixed_ms({ANTI_SPIN_YIELD_DELAY_MS})?;",
                    indent_prefix(3)
                ));
            }
            output_lines.push("        } else {".to_string());
            output_lines.push(format!(
                "            script_context.wait_fixed_ms({IDLE_POLL_DELAY_MS})?;"
            ));
            output_lines.push("        }".to_string());
            output_lines.push("    }".to_string());
        }
        TriggerBlock::OnInputPressed { trigger_input } => {
            let input_expr = format_input_pressed_expr(trigger_input);
            output_lines.push("    let mut was_trigger_pressed = false;".to_string());
            output_lines.push("    while !script_context.should_stop() {".to_string());
            output_lines.push(format!("        let is_trigger_pressed = {input_expr};"));
            output_lines
                .push("        if is_trigger_pressed && !was_trigger_pressed {".to_string());
            output_lines.push("            was_trigger_pressed = true;".to_string());
            emit_statement_slice(&program.statements, 3, output_lines);
            output_lines.push("        } else {".to_string());
            output_lines.push("            was_trigger_pressed = is_trigger_pressed;".to_string());
            output_lines.push(format!(
                "            script_context.wait_fixed_ms({IDLE_POLL_DELAY_MS})?;"
            ));
            output_lines.push("        }".to_string());
            output_lines.push("    }".to_string());
        }
        TriggerBlock::LoopWhileActive => {
            output_lines.push("    while !script_context.should_stop() {".to_string());
            emit_statement_slice(&program.statements, 2, output_lines);
            if !body_has_wait && !slice_ends_with_terminator(&program.statements) {
                output_lines.push(format!(
                    "{}script_context.wait_fixed_ms({ANTI_SPIN_YIELD_DELAY_MS})?;",
                    indent_prefix(2)
                ));
            }
            output_lines.push("    }".to_string());
        }
    }
}

fn emit_statement_slice(
    statements: &[StatementBlock],
    indent_level: usize,
    output_lines: &mut Vec<String>,
) {
    for block in statements {
        emit_single_statement(&block.kind, indent_level, output_lines);
    }
}

fn emit_single_statement(
    kind: &StatementKind,
    indent_level: usize,
    output_lines: &mut Vec<String>,
) {
    let prefix = indent_prefix(indent_level);
    match kind {
        StatementKind::IfThen {
            condition,
            then_blocks,
        } => {
            let condition_code = format_condition_expr(condition);
            output_lines.push(format!("{prefix}if {condition_code} {{"));
            emit_statement_slice(then_blocks, indent_level.saturating_add(1), output_lines);
            output_lines.push(format!("{prefix}}}"));
        }
        StatementKind::IfThenElse {
            condition,
            then_blocks,
            else_blocks,
        } => {
            let condition_code = format_condition_expr(condition);
            output_lines.push(format!("{prefix}if {condition_code} {{"));
            emit_statement_slice(then_blocks, indent_level.saturating_add(1), output_lines);
            output_lines.push(format!("{prefix}}} else {{"));
            emit_statement_slice(else_blocks, indent_level.saturating_add(1), output_lines);
            output_lines.push(format!("{prefix}}}"));
        }
        StatementKind::WhileCondition {
            condition,
            body_blocks,
        } => {
            let condition_code = format_condition_expr(condition);
            output_lines.push(format!(
                "{prefix}while !script_context.should_stop() && ({condition_code}) {{"
            ));
            emit_statement_slice(body_blocks, indent_level.saturating_add(1), output_lines);
            if !slice_contains_wait_action(body_blocks) && !slice_ends_with_terminator(body_blocks) {
                output_lines.push(format!(
                    "{}script_context.wait_fixed_ms({ANTI_SPIN_YIELD_DELAY_MS})?;",
                    indent_prefix(indent_level.saturating_add(1))
                ));
            }
            output_lines.push(format!("{prefix}}}"));
        }
        StatementKind::RepeatCount {
            repeat_count,
            body_blocks,
        } => {
            let inner_prefix = indent_prefix(indent_level.saturating_add(1));
            output_lines.push(format!(
                "{prefix}for _iteration_index in 0..{repeat_count}_u32 {{"
            ));
            output_lines.push(format!("{inner_prefix}if script_context.should_stop() {{"));
            output_lines.push(format!(
                "{}return Ok(());",
                indent_prefix(indent_level.saturating_add(2))
            ));
            output_lines.push(format!("{inner_prefix}}}"));
            emit_statement_slice(body_blocks, indent_level.saturating_add(1), output_lines);
            output_lines.push(format!("{prefix}}}"));
        }
        StatementKind::BreakLoop => output_lines.push(format!("{prefix}break;")),
        StatementKind::StopScript => output_lines.push(format!("{prefix}return Ok(());")),
        StatementKind::MouseClick {
            mouse_button,
            hold_duration_ms,
        } => output_lines.push(format!(
            "{prefix}script_context.mouse_click(MouseButton::{mouse_button:?}, {hold_duration_ms})?;"
        )),
        StatementKind::MouseDown { mouse_button } => output_lines.push(format!(
            "{prefix}script_context.mouse_down(MouseButton::{mouse_button:?})?;"
        )),
        StatementKind::MouseUp { mouse_button } => output_lines.push(format!(
            "{prefix}script_context.mouse_up(MouseButton::{mouse_button:?})?;"
        )),
        StatementKind::MouseMoveRelative {
            delta_x_px,
            delta_y_px,
        } => output_lines.push(format!(
            "{prefix}script_context.mouse_move_relative({delta_x_px}, {delta_y_px})?;"
        )),
        StatementKind::MouseScroll { vertical_steps } => output_lines.push(format!(
            "{prefix}script_context.mouse_scroll({vertical_steps})?;"
        )),
        StatementKind::KeyTap {
            key_code,
            hold_duration_ms,
        } => output_lines.push(format!(
            "{prefix}script_context.key_tap(KeyCode::{key_code:?}, {hold_duration_ms})?;"
        )),
        StatementKind::KeyDown { key_code } => output_lines.push(format!(
            "{prefix}script_context.key_down(KeyCode::{key_code:?})?;"
        )),
        StatementKind::KeyUp { key_code } => output_lines.push(format!(
            "{prefix}script_context.key_up(KeyCode::{key_code:?})?;"
        )),
        StatementKind::TypeText {
            text,
            keystroke_delay_ms,
            jitter_ms,
        } => output_lines.push(format!(
            "{prefix}script_context.type_text({text:?}, {keystroke_delay_ms}, {jitter_ms})?;"
        )),
        StatementKind::WaitFixed { delay_ms } => output_lines.push(format!(
            "{prefix}script_context.wait_fixed_ms({delay_ms})?;"
        )),
        StatementKind::WaitRandomUniform {
            min_delay_ms,
            max_delay_ms,
        } => output_lines.push(format!(
            "{prefix}script_context.wait_uniform_ms({min_delay_ms}, {max_delay_ms})?;"
        )),
        StatementKind::WaitRandomGaussian {
            base_delay_ms,
            min_offset_ms,
            max_offset_ms,
            mean_offset_ms,
            std_dev_ms,
        } => emit_gaussian_wait_lines(
            *base_delay_ms,
            *min_offset_ms,
            *max_offset_ms,
            *mean_offset_ms,
            *std_dev_ms,
            &prefix,
            output_lines,
        ),
        StatementKind::SetVariable {
            variable_name,
            value,
        } => output_lines.push(format!(
            "{prefix}script_variables.insert({variable_name:?}, {});",
            format_f64_literal(*value)
        )),
        StatementKind::ChangeVariable {
            variable_name,
            delta_value,
        } => {
            output_lines.push(format!(
                "{prefix}let updated_value = script_variables.get({variable_name:?}).copied().unwrap_or(0.0_f64) + {};",
                format_f64_literal(*delta_value)
            ));
            output_lines.push(format!(
                "{prefix}script_variables.insert({variable_name:?}, updated_value);"
            ));
        }
    }
}

fn emit_gaussian_wait_lines(
    base_delay_ms: u64,
    min_offset_ms: f64,
    max_offset_ms: f64,
    mean_offset_ms: f64,
    std_dev_ms: f64,
    prefix: &str,
    output_lines: &mut Vec<String>,
) {
    let min_literal = format_f64_literal(min_offset_ms);
    let max_literal = format_f64_literal(max_offset_ms);
    let mean_literal = format_f64_literal(mean_offset_ms);
    let std_dev_literal = format_f64_literal(std_dev_ms);
    output_lines.push(format!(
        "{prefix}let gaussian_range_ms = GaussianRangeMs::new({min_literal}, {max_literal}, {mean_literal}, {std_dev_literal})?;"
    ));
    if base_delay_ms == 0 {
        output_lines.push(format!(
            "{prefix}script_context.wait_gaussian_ms(gaussian_range_ms)?;"
        ));
    } else {
        output_lines.push(format!(
            "{prefix}script_context.wait_base_plus_gaussian_ms({base_delay_ms}, gaussian_range_ms)?;"
        ));
    }
}

fn format_input_pressed_expr(trigger: InputTrigger) -> String {
    match trigger {
        InputTrigger::Mouse(mouse_button) => {
            format!("script_context.is_mouse_pressed(MouseButton::{mouse_button:?})?")
        }
        InputTrigger::Key(key_code) => {
            format!("script_context.is_key_pressed(KeyCode::{key_code:?})?")
        }
    }
}

fn format_condition_expr(condition: &ConditionExpr) -> String {
    match condition {
        ConditionExpr::IsInputPressed { input } => format_input_pressed_expr(*input),
        ConditionExpr::RandomChance { chance_percent } => {
            let clamped_percent = if chance_percent.is_finite() {
                chance_percent.clamp(0.0, 100.0)
            } else {
                0.0
            };
            format!(
                "(rand::rng().random_range(0.0_f64..100.0_f64) < {})",
                format_f64_literal(clamped_percent)
            )
        }
        ConditionExpr::CompareVariable {
            variable_name,
            operator,
            target_value,
        } => {
            let read_expr =
                format!("script_variables.get({variable_name:?}).copied().unwrap_or(0.0_f64)");
            let target_literal = format_f64_literal(*target_value);
            match operator {
                ComparisonOperator::Equal => {
                    format!("(({read_expr} - {target_literal}).abs() <= 1e-9_f64)")
                }
                ComparisonOperator::NotEqual => {
                    format!("(({read_expr} - {target_literal}).abs() > 1e-9_f64)")
                }
                ComparisonOperator::LessThan => format!("({read_expr} < {target_literal})"),
                ComparisonOperator::LessOrEqual => format!("({read_expr} <= {target_literal})"),
                ComparisonOperator::GreaterThan => format!("({read_expr} > {target_literal})"),
                ComparisonOperator::GreaterOrEqual => format!("({read_expr} >= {target_literal})"),
            }
        }
        ConditionExpr::Not { inner_condition } => {
            format!("!({})", format_condition_expr(inner_condition))
        }
        ConditionExpr::And {
            left_condition,
            right_condition,
        } => format!(
            "({}) && ({})",
            format_condition_expr(left_condition),
            format_condition_expr(right_condition)
        ),
        ConditionExpr::Or {
            left_condition,
            right_condition,
        } => format!(
            "({}) || ({})",
            format_condition_expr(left_condition),
            format_condition_expr(right_condition)
        ),
    }
}

fn inspect_program_features(program: &VisualProgram) -> ProgramFeatureFlags {
    let mut flags = ProgramFeatureFlags::default();
    if let Some(trigger_input) = program.trigger.trigger_input() {
        record_trigger_feature(trigger_input, &mut flags);
    }
    inspect_statement_slice_features(&program.statements, &mut flags);
    flags
}

fn inspect_statement_slice_features(
    statements: &[StatementBlock],
    flags: &mut ProgramFeatureFlags,
) {
    for block in statements {
        match &block.kind {
            StatementKind::IfThen {
                condition,
                then_blocks,
            } => {
                inspect_condition_features(condition, flags);
                inspect_statement_slice_features(then_blocks, flags);
            }
            StatementKind::IfThenElse {
                condition,
                then_blocks,
                else_blocks,
            } => {
                inspect_condition_features(condition, flags);
                inspect_statement_slice_features(then_blocks, flags);
                inspect_statement_slice_features(else_blocks, flags);
            }
            StatementKind::WhileCondition {
                condition,
                body_blocks,
            } => {
                inspect_condition_features(condition, flags);
                inspect_statement_slice_features(body_blocks, flags);
            }
            StatementKind::RepeatCount { body_blocks, .. } => {
                inspect_statement_slice_features(body_blocks, flags);
            }
            StatementKind::MouseClick { .. }
            | StatementKind::MouseDown { .. }
            | StatementKind::MouseUp { .. } => flags.uses_mouse_button = true,
            StatementKind::KeyTap { .. }
            | StatementKind::KeyDown { .. }
            | StatementKind::KeyUp { .. } => flags.uses_key_code = true,
            StatementKind::WaitRandomGaussian { .. } => flags.uses_gaussian_range = true,
            StatementKind::SetVariable { .. } | StatementKind::ChangeVariable { .. } => {
                flags.uses_variables = true;
                flags.mutates_variables = true;
            }
            StatementKind::BreakLoop
            | StatementKind::StopScript
            | StatementKind::MouseMoveRelative { .. }
            | StatementKind::MouseScroll { .. }
            | StatementKind::TypeText { .. }
            | StatementKind::WaitFixed { .. }
            | StatementKind::WaitRandomUniform { .. } => {}
        }
    }
}

fn inspect_condition_features(condition: &ConditionExpr, flags: &mut ProgramFeatureFlags) {
    match condition {
        ConditionExpr::IsInputPressed { input } => record_trigger_feature(*input, flags),
        ConditionExpr::RandomChance { .. } => flags.uses_random_chance = true,
        ConditionExpr::CompareVariable { .. } => flags.uses_variables = true,
        ConditionExpr::Not { inner_condition } => {
            inspect_condition_features(inner_condition, flags);
        }
        ConditionExpr::And {
            left_condition,
            right_condition,
        }
        | ConditionExpr::Or {
            left_condition,
            right_condition,
        } => {
            inspect_condition_features(left_condition, flags);
            inspect_condition_features(right_condition, flags);
        }
    }
}

fn record_trigger_feature(trigger: InputTrigger, flags: &mut ProgramFeatureFlags) {
    match trigger {
        InputTrigger::Mouse(_) => flags.uses_mouse_button = true,
        InputTrigger::Key(_) => flags.uses_key_code = true,
    }
}

fn slice_contains_wait_action(statements: &[StatementBlock]) -> bool {
    statements.iter().any(|block| match &block.kind {
        StatementKind::WaitFixed { .. }
        | StatementKind::WaitRandomUniform { .. }
        | StatementKind::WaitRandomGaussian { .. } => true,
        StatementKind::IfThen { then_blocks, .. } => slice_contains_wait_action(then_blocks),
        StatementKind::IfThenElse {
            then_blocks,
            else_blocks,
            ..
        } => slice_contains_wait_action(then_blocks) || slice_contains_wait_action(else_blocks),
        StatementKind::WhileCondition { body_blocks, .. }
        | StatementKind::RepeatCount { body_blocks, .. } => slice_contains_wait_action(body_blocks),
        _ => false,
    })
}

fn slice_ends_with_terminator(statements: &[StatementBlock]) -> bool {
    matches!(
        statements.last().map(|block| &block.kind),
        Some(StatementKind::BreakLoop | StatementKind::StopScript)
    )
}

fn indent_prefix(indent_level: usize) -> String {
    " ".repeat(indent_level.saturating_mul(INDENT_WIDTH_SPACES))
}

fn format_f64_literal(numeric_value: f64) -> String {
    let finite_value = if numeric_value.is_finite() {
        numeric_value
    } else {
        0.0
    };
    let formatted = format!("{finite_value:?}");
    if formatted.contains('.') || formatted.contains('e') || formatted.contains('E') {
        format!("{formatted}_f64")
    } else {
        format!("{formatted}.0_f64")
    }
}

fn sanitize_single_line_comment(raw_text: &str) -> String {
    raw_text
        .chars()
        .map(|character| {
            if character == '\n' || character == '\r' {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use clickforge_sdk::KeyCode;

    use super::*;

    #[test]
    fn generates_clean_rust_source_for_building_upgrade_template() {
        let template_script = ScriptDocument::new_building_upgrade_template();
        let generated_source = generate_rust_source_from_script(&template_script);

        assert!(
            generated_source.contains(
                "pub fn run_script(script_context: &mut ScriptContext) -> ScriptResult<()>"
            )
        );
        assert!(
            generated_source
                .contains("GaussianRangeMs::new(10.0_f64, 20.0_f64, 15.0_f64, 2.5_f64)?")
        );
        assert!(generated_source.contains("wait_base_plus_gaussian_ms(45, gaussian_range_ms)?"));
        assert!(generated_source.contains("mouse_click(MouseButton::Left, 12)?"));
        assert!(!generated_source.contains(".unwrap()"));
        assert!(!generated_source.contains(".expect("));
    }

    #[test]
    fn generates_variables_random_chance_and_nested_control_flow() {
        let mut script = ScriptDocument::new_building_upgrade_template();
        script.visual_program = VisualProgram::new(TriggerBlock::OnInputToggle {
            trigger_input: InputTrigger::Key(KeyCode::F7),
        });
        script
            .visual_program
            .append_root_block(StatementKind::SetVariable {
                variable_name: "step_count".to_string(),
                value: 0.0,
            });
        let tap_block = script.visual_program.create_block(StatementKind::KeyTap {
            key_code: KeyCode::E,
            hold_duration_ms: 10,
        });
        script
            .visual_program
            .append_root_block(StatementKind::IfThen {
                condition: ConditionExpr::And {
                    left_condition: Box::new(ConditionExpr::RandomChance {
                        chance_percent: 50.0,
                    }),
                    right_condition: Box::new(ConditionExpr::CompareVariable {
                        variable_name: "step_count".to_string(),
                        operator: ComparisonOperator::LessThan,
                        target_value: 5.0,
                    }),
                },
                then_blocks: vec![tap_block],
            });

        let generated_source = generate_rust_source_from_script(&script);
        assert!(generated_source.contains("use std::collections::HashMap;"));
        assert!(generated_source.contains("use rand::RngExt;"));
        assert!(generated_source.contains("let mut is_toggle_active = false;"));
        assert!(generated_source.contains("KeyCode::F7"));
        assert!(generated_source.contains("KeyCode::E"));
        assert!(!generated_source.contains(".unwrap()"));
    }
}
