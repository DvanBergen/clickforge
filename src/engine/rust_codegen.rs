mod plan;

use clickforge_sdk::InputTrigger;

use crate::engine::rust_codegen::plan::{
    CustomCodegenPlan, ProgramFeatureFlags, ResolvedCustomTile, inspect_program_features,
    slice_contains_wait_action, slice_ends_with_terminator,
};
use crate::models::{
    ComparisonOperator, ConditionExpr, CustomBlockDefinition, ScriptDocument, StatementBlock,
    StatementKind, TriggerBlock, VisualProgram,
};

const IDLE_POLL_DELAY_MS: u64 = 4;
const HOLD_POLL_DELAY_MS: u64 = 2;
const ANTI_SPIN_YIELD_DELAY_MS: u64 = 1;
const INDENT_WIDTH_SPACES: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum VariableBindingMode {
    Owned,
    MutRef,
    SharedRef,
}

/// Translates any Visual Block script into compiling native Rust using `clickforge_sdk`.
pub fn generate_rust_source_from_script(script: &ScriptDocument) -> String {
    generate_rust_source_with_custom_blocks(script, &[])
}

/// Translates a Visual Block script and its referenced custom tiles into compiling native Rust.
pub fn generate_rust_source_with_custom_blocks(
    script: &ScriptDocument,
    custom_blocks: &[CustomBlockDefinition],
) -> String {
    let program = &script.visual_program;
    let custom_plan = CustomCodegenPlan::build(program, custom_blocks);
    let feature_flags = inspect_program_features(program, &custom_plan);
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
            "    {mutability_keyword} script_variables: HashMap<&'static str, f64> = HashMap::new();"
        ));
    }

    emit_trigger_body(program, &custom_plan, &mut output_lines);

    output_lines.push("    Ok(())".to_string());
    output_lines.push("}".to_string());

    for resolved_tile in &custom_plan.ordered_tiles {
        output_lines.push(String::new());
        emit_custom_tile_helper(resolved_tile, &custom_plan, &mut output_lines);
    }

    output_lines.join("\n")
}

fn emit_custom_tile_helper(
    resolved_tile: &ResolvedCustomTile<'_>,
    custom_plan: &CustomCodegenPlan<'_>,
    output_lines: &mut Vec<String>,
) {
    let (parameter_suffix, binding_mode) = if resolved_tile.mutates_variables {
        (
            ", script_variables: &mut HashMap<&'static str, f64>",
            VariableBindingMode::MutRef,
        )
    } else if resolved_tile.uses_variables {
        (
            ", script_variables: &HashMap<&'static str, f64>",
            VariableBindingMode::SharedRef,
        )
    } else {
        ("", VariableBindingMode::Owned)
    };

    output_lines.push(format!(
        "// Custom tile: {}",
        sanitize_single_line_comment(&resolved_tile.definition.name)
    ));
    output_lines.push(format!(
        "fn {}(script_context: &mut ScriptContext{parameter_suffix}) -> ScriptResult<()> {{",
        resolved_tile.function_name
    ));
    emit_statement_slice(
        &resolved_tile.definition.statements,
        1,
        custom_plan,
        binding_mode,
        output_lines,
    );
    if !slice_ends_with_terminator(&resolved_tile.definition.statements) {
        output_lines.push("    Ok(())".to_string());
    }
    output_lines.push("}".to_string());
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

fn emit_trigger_body(
    program: &VisualProgram,
    custom_plan: &CustomCodegenPlan<'_>,
    output_lines: &mut Vec<String>,
) {
    let body_has_wait = slice_contains_wait_action(&program.statements, custom_plan, 0);

    match &program.trigger {
        TriggerBlock::WhileInputHeld {
            trigger_input,
            additional_held_inputs,
        } => {
            let input_expr = format_chord_pressed_expr(*trigger_input, additional_held_inputs);
            output_lines.push("    while !script_context.should_stop() {".to_string());
            output_lines.push(format!("        if {input_expr} {{"));
            emit_statement_slice(
                &program.statements,
                3,
                custom_plan,
                VariableBindingMode::Owned,
                output_lines,
            );
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
        TriggerBlock::OnInputToggle {
            trigger_input,
            additional_held_inputs,
        } => {
            let input_expr = format_chord_pressed_expr(*trigger_input, additional_held_inputs);
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
            emit_statement_slice(
                &program.statements,
                3,
                custom_plan,
                VariableBindingMode::Owned,
                output_lines,
            );
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
        TriggerBlock::OnInputPressed {
            trigger_input,
            additional_held_inputs,
        } => {
            let input_expr = format_chord_pressed_expr(*trigger_input, additional_held_inputs);
            output_lines.push("    let mut was_trigger_pressed = false;".to_string());
            output_lines.push("    while !script_context.should_stop() {".to_string());
            output_lines.push(format!("        let is_trigger_pressed = {input_expr};"));
            output_lines
                .push("        if is_trigger_pressed && !was_trigger_pressed {".to_string());
            output_lines.push("            was_trigger_pressed = true;".to_string());
            emit_statement_slice(
                &program.statements,
                3,
                custom_plan,
                VariableBindingMode::Owned,
                output_lines,
            );
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
            emit_statement_slice(
                &program.statements,
                2,
                custom_plan,
                VariableBindingMode::Owned,
                output_lines,
            );
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
    custom_plan: &CustomCodegenPlan<'_>,
    binding_mode: VariableBindingMode,
    output_lines: &mut Vec<String>,
) {
    for block in statements {
        emit_single_statement(
            &block.kind,
            indent_level,
            custom_plan,
            binding_mode,
            output_lines,
        );
    }
}

fn emit_single_statement(
    kind: &StatementKind,
    indent_level: usize,
    custom_plan: &CustomCodegenPlan<'_>,
    binding_mode: VariableBindingMode,
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
            emit_statement_slice(
                then_blocks,
                indent_level.saturating_add(1),
                custom_plan,
                binding_mode,
                output_lines,
            );
            output_lines.push(format!("{prefix}}}"));
        }
        StatementKind::IfThenElse {
            condition,
            then_blocks,
            else_blocks,
        } => {
            let condition_code = format_condition_expr(condition);
            output_lines.push(format!("{prefix}if {condition_code} {{"));
            emit_statement_slice(
                then_blocks,
                indent_level.saturating_add(1),
                custom_plan,
                binding_mode,
                output_lines,
            );
            output_lines.push(format!("{prefix}}} else {{"));
            emit_statement_slice(
                else_blocks,
                indent_level.saturating_add(1),
                custom_plan,
                binding_mode,
                output_lines,
            );
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
            emit_statement_slice(
                body_blocks,
                indent_level.saturating_add(1),
                custom_plan,
                binding_mode,
                output_lines,
            );
            if !slice_contains_wait_action(body_blocks, custom_plan, 0)
                && !slice_ends_with_terminator(body_blocks)
            {
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
            emit_statement_slice(
                body_blocks,
                indent_level.saturating_add(1),
                custom_plan,
                binding_mode,
                output_lines,
            );
            output_lines.push(format!("{prefix}}}"));
        }
        StatementKind::BreakLoop => output_lines.push(format!("{prefix}break;")),
        StatementKind::StopScript => output_lines.push(format!("{prefix}return Ok(());")),
        StatementKind::DoNothing => {
            output_lines.push(format!("{prefix}// Do nothing (explicit no-op block)"));
        }
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
        StatementKind::MouseHoldWhilePressed {
            button,
            watch_input,
        } => {
            let watch_expr = format_input_pressed_expr(*watch_input);
            let inner_prefix = indent_prefix(indent_level.saturating_add(1));
            let loop_prefix = indent_prefix(indent_level.saturating_add(2));
            output_lines.push(format!("{prefix}if {watch_expr} {{"));
            output_lines.push(format!(
                "{inner_prefix}script_context.mouse_down(MouseButton::{button:?})?;"
            ));
            output_lines.push(format!(
                "{inner_prefix}while !script_context.should_stop() && {watch_expr} {{"
            ));
            output_lines.push(format!(
                "{loop_prefix}script_context.wait_fixed_ms({HOLD_POLL_DELAY_MS})?;"
            ));
            output_lines.push(format!("{inner_prefix}}}"));
            output_lines.push(format!(
                "{inner_prefix}script_context.mouse_up(MouseButton::{button:?})?;"
            ));
            output_lines.push(format!("{prefix}}}"));
        }
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
        StatementKind::RunCustomBlock {
            custom_block_id,
            cached_name,
        } => emit_custom_block_invocation(
            custom_block_id,
            cached_name,
            &prefix,
            custom_plan,
            binding_mode,
            output_lines,
        ),
    }
}

fn emit_custom_block_invocation(
    custom_block_id: &str,
    cached_name: &str,
    prefix: &str,
    custom_plan: &CustomCodegenPlan<'_>,
    binding_mode: VariableBindingMode,
    output_lines: &mut Vec<String>,
) {
    let Some(resolved_tile) = custom_plan.lookup_tile(custom_block_id) else {
        output_lines.push(format!(
            "{prefix}// Missing custom tile: {} ({})",
            sanitize_single_line_comment(cached_name),
            sanitize_single_line_comment(custom_block_id)
        ));
        return;
    };

    let variable_arg = if resolved_tile.mutates_variables {
        match binding_mode {
            VariableBindingMode::Owned => ", &mut script_variables",
            VariableBindingMode::MutRef | VariableBindingMode::SharedRef => ", script_variables",
        }
    } else if resolved_tile.uses_variables {
        match binding_mode {
            VariableBindingMode::Owned => ", &script_variables",
            VariableBindingMode::MutRef | VariableBindingMode::SharedRef => ", script_variables",
        }
    } else {
        ""
    };

    output_lines.push(format!(
        "{prefix}{}(script_context{variable_arg})?;",
        resolved_tile.function_name
    ));
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

fn format_chord_pressed_expr(
    trigger_input: InputTrigger,
    additional_held_inputs: &[InputTrigger],
) -> String {
    let mut expressions = Vec::with_capacity(1_usize.saturating_add(additional_held_inputs.len()));
    expressions.push(format_input_pressed_expr(trigger_input));
    for required_input in additional_held_inputs {
        expressions.push(format_input_pressed_expr(*required_input));
    }
    expressions.join(" && ")
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
#[path = "rust_codegen_tests.rs"]
mod tests;
