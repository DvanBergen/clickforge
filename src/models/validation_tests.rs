use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};

use super::*;
use crate::models::blocks::TriggerBlock;
use crate::models::custom_blocks::{CustomBlockColor, CustomBlockDefinition};
use crate::models::target::GameTargetRule;

#[test]
fn building_upgrade_template_passes_validation_gate() {
    let template_script = ScriptDocument::new_building_upgrade_template();
    assert!(validate_script_for_persistence_and_execution(&template_script).is_ok());
}

#[test]
fn blank_script_without_kill_switch_is_rejected() {
    let blank_script = ScriptDocument::new_blank(
        "Unnamed Clicker",
        TargetScope::Global,
        ScriptMode::VisualBlocks,
    );
    let report = inspect_script_validation(&blank_script);
    assert!(report.has_issue(ValidationIssueCode::MissingKillSwitchKey));
    assert!(validate_script_for_persistence_and_execution(&blank_script).is_err());
}

#[test]
fn kill_switch_collisions_with_trigger_arm_and_emitted_keys_are_rejected() {
    let mut script = ScriptDocument::new_building_upgrade_template();
    script.kill_switch_key = Some(KeyCode::F6);
    let arm_report = inspect_script_validation(&script);
    assert!(arm_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithArmToggleKey));

    script.arm_toggle_key = Some(KeyCode::F7);
    script.visual_program.trigger = TriggerBlock::WhileInputHeld {
        trigger_input: InputTrigger::Key(KeyCode::F6),
        additional_held_inputs: Vec::new(),
    };
    let trigger_report = inspect_script_validation(&script);
    assert!(trigger_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithTriggerKey));

    script.visual_program.trigger = TriggerBlock::WhileInputHeld {
        trigger_input: InputTrigger::Mouse(MouseButton::Left),
        additional_held_inputs: Vec::new(),
    };
    script.kill_switch_key = Some(KeyCode::E);
    script
        .visual_program
        .append_root_block(StatementKind::TypeText {
            text: "hello".to_string(),
            keystroke_delay_ms: 10,
            jitter_ms: 2,
        });
    let emitted_report = inspect_script_validation(&script);
    assert!(emitted_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithEmittedKey));
}

#[test]
fn additional_chord_inputs_cannot_collide_with_kill_switch_or_exceed_cap() {
    let mut script = ScriptDocument::new_building_upgrade_template();
    script.kill_switch_key = Some(KeyCode::F8);
    script.arm_toggle_key = Some(KeyCode::F6);
    script.visual_program.trigger = TriggerBlock::WhileInputHeld {
        trigger_input: InputTrigger::Mouse(MouseButton::Left),
        additional_held_inputs: vec![
            InputTrigger::Key(KeyCode::LeftShift),
            InputTrigger::Key(KeyCode::F8),
            InputTrigger::Key(KeyCode::F6),
        ],
    };

    let collision_report = inspect_script_validation(&script);
    assert!(collision_report.has_issue(ValidationIssueCode::KillSwitchCollidesWithTriggerKey));
    assert!(collision_report.has_issue(ValidationIssueCode::ArmToggleCollidesWithTriggerKey));

    script.visual_program.trigger = TriggerBlock::WhileInputHeld {
        trigger_input: InputTrigger::Mouse(MouseButton::Left),
        additional_held_inputs: vec![
            InputTrigger::Key(KeyCode::LeftShift),
            InputTrigger::Key(KeyCode::LeftCtrl),
            InputTrigger::Key(KeyCode::LeftAlt),
            InputTrigger::Key(KeyCode::Space),
            InputTrigger::Key(KeyCode::Tab),
        ],
    };
    let cap_report = inspect_script_validation(&script);
    assert!(cap_report.has_issue(ValidationIssueCode::TooManyTriggerInputs));
}

#[test]
fn invalid_timing_and_empty_selected_game_rule_are_rejected() {
    let mut script = ScriptDocument::new_building_upgrade_template();
    script.target_scope = TargetScope::SelectedGame(GameTargetRule::default());
    script
        .visual_program
        .append_root_block(StatementKind::WaitRandomUniform {
            min_delay_ms: 200,
            max_delay_ms: 100,
        });
    script
        .visual_program
        .append_root_block(StatementKind::WaitRandomGaussian {
            base_delay_ms: 20,
            min_offset_ms: 50.0,
            max_offset_ms: 10.0,
            mean_offset_ms: 30.0,
            std_dev_ms: 2.0,
        });

    let report = inspect_script_validation(&script);
    assert!(report.has_issue(ValidationIssueCode::InvalidSelectedGameTarget));
    assert!(report.has_issue(ValidationIssueCode::InvalidTimingRange));
    assert!(report.has_issue(ValidationIssueCode::InvalidGaussianParameters));
}

#[test]
fn custom_block_validation_catches_missing_cyclic_and_kill_switch_collision() {
    let mut script = ScriptDocument::new_building_upgrade_template();
    script.kill_switch_key = Some(KeyCode::F8);
    script
        .visual_program
        .append_root_block(StatementKind::RunCustomBlock {
            custom_block_id: "tile-outer".to_string(),
            cached_name: "Outer Tile".to_string(),
        });

    let missing_report = inspect_script_validation_with_custom_blocks(&script, &[]);
    assert!(missing_report.has_issue(ValidationIssueCode::MissingCustomBlock));

    let mut inner_kill_collision = CustomBlockDefinition::from_statements(
        "Inner Collision",
        "Emits F8",
        CustomBlockColor::Coral,
        vec![StatementBlock {
            id: BlockId(1),
            kind: StatementKind::KeyTap {
                key_code: KeyCode::F8,
                hold_duration_ms: 10,
            },
        }],
    );
    inner_kill_collision.id = "tile-inner".to_string();

    let mut outer_tile = CustomBlockDefinition::from_statements(
        "Outer Tile",
        "Calls inner",
        CustomBlockColor::Blue,
        vec![StatementBlock {
            id: BlockId(1),
            kind: StatementKind::RunCustomBlock {
                custom_block_id: "tile-inner".to_string(),
                cached_name: "Inner Collision".to_string(),
            },
        }],
    );
    outer_tile.id = "tile-outer".to_string();

    let collision_report = inspect_script_validation_with_custom_blocks(
        &script,
        &[outer_tile.clone(), inner_kill_collision],
    );
    assert!(collision_report.has_issue(ValidationIssueCode::KillSwitchCollision));
    assert!(validate_script_with_custom_blocks(&script, &[outer_tile.clone()]).is_err());

    let mut cyclic_inner = CustomBlockDefinition::from_statements(
        "Cyclic Inner",
        "Calls outer",
        CustomBlockColor::Gold,
        vec![StatementBlock {
            id: BlockId(1),
            kind: StatementKind::RunCustomBlock {
                custom_block_id: "tile-outer".to_string(),
                cached_name: "Outer Tile".to_string(),
            },
        }],
    );
    cyclic_inner.id = "tile-inner".to_string();

    let cycle_report =
        inspect_script_validation_with_custom_blocks(&script, &[outer_tile.clone(), cyclic_inner]);
    assert!(cycle_report.has_issue(ValidationIssueCode::CyclicCustomBlock));

    let mut clean_inner = CustomBlockDefinition::from_statements(
        "Safe Click",
        "Left click",
        CustomBlockColor::Teal,
        vec![StatementBlock {
            id: BlockId(1),
            kind: StatementKind::MouseClick {
                mouse_button: MouseButton::Left,
                hold_duration_ms: 12,
            },
        }],
    );
    clean_inner.id = "tile-inner".to_string();

    let valid_library = [outer_tile, clean_inner];
    assert!(validate_script_with_custom_blocks(&script, &valid_library).is_ok());
}
