use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};

use super::*;
use crate::models::{
    BlockId, CustomBlockColor, CustomBlockDefinition, StatementBlock, StatementKind, TriggerBlock,
    VisualProgram,
};

#[test]
fn generates_clean_rust_source_for_building_upgrade_template() {
    let template_script = ScriptDocument::new_building_upgrade_template();
    let generated_source = generate_rust_source_from_script(&template_script);

    assert!(
        generated_source
            .contains("pub fn run_script(script_context: &mut ScriptContext) -> ScriptResult<()>")
    );
    assert!(
        generated_source.contains("GaussianRangeMs::new(10.0_f64, 20.0_f64, 15.0_f64, 2.5_f64)?")
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
        additional_held_inputs: Vec::new(),
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

#[test]
fn generates_chord_trigger_conditions_and_do_nothing_comment() {
    let mut script = ScriptDocument::new_building_upgrade_template();
    script.visual_program = VisualProgram::new(TriggerBlock::WhileInputHeld {
        trigger_input: InputTrigger::Mouse(MouseButton::Left),
        additional_held_inputs: vec![InputTrigger::Key(KeyCode::LeftShift)],
    });
    let noop_block = script.visual_program.create_block(StatementKind::DoNothing);
    let click_block = script
        .visual_program
        .create_block(StatementKind::MouseClick {
            mouse_button: MouseButton::Left,
            hold_duration_ms: 12,
        });
    script
        .visual_program
        .append_root_block(StatementKind::IfThenElse {
            condition: ConditionExpr::RandomChance {
                chance_percent: 25.0,
            },
            then_blocks: vec![noop_block],
            else_blocks: vec![click_block],
        });

    let generated_source = generate_rust_source_from_script(&script);
    assert!(generated_source.contains("KeyCode"));
    assert!(generated_source.contains("MouseButton"));
    assert!(generated_source.contains(
        "script_context.is_mouse_pressed(MouseButton::Left)? && script_context.is_key_pressed(KeyCode::LeftShift)?"
    ));
    assert!(generated_source.contains("// Do nothing (explicit no-op block)"));
    assert!(!generated_source.contains(".unwrap()"));
    assert!(!generated_source.contains(".expect("));
}

#[test]
fn generates_mouse_hold_while_pressed_and_nested_custom_tile_helpers() {
    let mut inner_tile = CustomBlockDefinition::from_statements(
        "Drag Hold!",
        "Holds left mouse while physically held",
        CustomBlockColor::Coral,
        vec![
            StatementBlock {
                id: BlockId(1),
                kind: StatementKind::ChangeVariable {
                    variable_name: "drags".to_string(),
                    delta_value: 1.0,
                },
            },
            StatementBlock {
                id: BlockId(2),
                kind: StatementKind::MouseHoldWhilePressed {
                    button: MouseButton::Left,
                    watch_input: InputTrigger::Mouse(MouseButton::Left),
                },
            },
        ],
    );
    inner_tile.id = "tile-drag-hold".to_string();

    let mut outer_tile = CustomBlockDefinition::from_statements(
        "Smart Action",
        "Runs Drag Hold then taps E",
        CustomBlockColor::Blue,
        vec![
            StatementBlock {
                id: BlockId(1),
                kind: StatementKind::RunCustomBlock {
                    custom_block_id: "tile-drag-hold".to_string(),
                    cached_name: "Drag Hold!".to_string(),
                },
            },
            StatementBlock {
                id: BlockId(2),
                kind: StatementKind::KeyTap {
                    key_code: KeyCode::E,
                    hold_duration_ms: 10,
                },
            },
        ],
    );
    outer_tile.id = "tile-smart-action".to_string();

    let mut script = ScriptDocument::new_building_upgrade_template();
    script.visual_program = VisualProgram::new(TriggerBlock::OnInputPressed {
        trigger_input: InputTrigger::Mouse(MouseButton::Left),
        additional_held_inputs: Vec::new(),
    });
    script
        .visual_program
        .append_root_block(StatementKind::RunCustomBlock {
            custom_block_id: "tile-smart-action".to_string(),
            cached_name: "Smart Action".to_string(),
        });

    let generated_source =
        generate_rust_source_with_custom_blocks(&script, &[outer_tile, inner_tile]);

    assert!(generated_source.contains("use std::collections::HashMap;"));
    assert!(generated_source.contains("fn run_custom_tile_smart_action("));
    assert!(generated_source.contains("fn run_custom_tile_drag_hold("));
    assert!(generated_source.contains("script_context.mouse_down(MouseButton::Left)?;"));
    assert!(generated_source.contains(
        "while !script_context.should_stop() && script_context.is_mouse_pressed(MouseButton::Left)? {"
    ));
    assert!(generated_source.contains("script_context.wait_fixed_ms(2)?;"));
    assert!(generated_source.contains("script_context.mouse_up(MouseButton::Left)?;"));
    assert!(!generated_source.contains(".unwrap()"));
    assert!(!generated_source.contains(".expect("));
}
