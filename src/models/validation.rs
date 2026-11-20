use clickforge_sdk::{GaussianRangeMs, InputTrigger, KeyCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::models::blocks::{
    BlockId, ConditionExpr, MAX_TRIGGER_AND_INPUTS, StatementBlock, StatementKind,
};
use crate::models::script::{ScriptDocument, ScriptMode};
use crate::models::target::TargetScope;

/// Maximum script name length in Unicode scalar values.
pub const MAX_SCRIPT_NAME_CHARS: usize = 120;

/// Maximum allowed control-flow nesting depth inside a Visual Block program.
pub const MAX_BLOCK_NESTING_DEPTH: usize = 16;

/// Maximum allowed total statement blocks inside a single Visual Block program.
pub const MAX_BLOCKS_PER_SCRIPT: usize = 500;

/// Maximum allowed single wait duration in milliseconds (10 minutes).
pub const MAX_WAIT_DELAY_MS: u64 = 600_000;

/// Maximum allowed key or mouse hold duration in milliseconds (1 minute).
pub const MAX_HOLD_DURATION_MS: u64 = 60_000;

/// Maximum allowed iterations for a `RepeatCount` block.
pub const MAX_REPEAT_COUNT: u32 = 100_000;

/// Maximum allowed character count for a single `TypeText` block.
pub const MAX_TYPE_TEXT_CHARS: usize = 4_096;

/// Maximum allowed Advanced Rust source size in bytes (256 KiB).
pub const MAX_RUST_SOURCE_BYTES: usize = 262_144;

/// Machine-readable category for every script safety and parameter validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationIssueCode {
    EmptyScriptName,
    ScriptNameTooLong,
    MissingKillSwitchKey,
    KillSwitchCollidesWithTriggerKey,
    KillSwitchCollidesWithArmToggleKey,
    KillSwitchCollidesWithEmittedKey,
    KillSwitchCollidesWithConditionKey,
    ArmToggleCollidesWithTriggerKey,
    InvalidSelectedGameTarget,
    InvalidTimingRange,
    InvalidGaussianParameters,
    InvalidProbabilityRange,
    InvalidRepeatCount,
    EmptyVariableName,
    NonFiniteNumericValue,
    ExcessiveBlockNesting,
    TooManyBlocks,
    TooManyTriggerInputs,
    EmptyRustSource,
    RustSourceTooLarge,
    TypeTextTooLong,
}

/// Single actionable validation finding attached optionally to a specific visual block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub code: ValidationIssueCode,
    pub block_id: Option<BlockId>,
    pub message: String,
}

/// Complete list of validation findings for a `ScriptDocument`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ValidationReport {
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    /// Returns true when zero validation issues were detected.
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns true when at least one issue carries the specified `code`.
    pub fn has_issue(&self, code: ValidationIssueCode) -> bool {
        self.issues.iter().any(|issue| issue.code == code)
    }

    /// Collects all human-readable issue descriptions in order.
    pub fn summary_messages(&self) -> Vec<String> {
        self.issues
            .iter()
            .map(|issue| issue.message.clone())
            .collect()
    }

    fn push_issue(
        &mut self,
        code: ValidationIssueCode,
        block_id: Option<BlockId>,
        message: impl Into<String>,
    ) {
        self.issues.push(ValidationIssue {
            code,
            block_id,
            message: message.into(),
        });
    }
}

/// Structured error returned when a script fails the mandatory safety and integrity gate.
#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Script validation failed ({issue_count} issue(s)): {first_message}")]
    Failed {
        issue_count: usize,
        first_message: String,
        report: ValidationReport,
    },
}

/// Inspects a script and returns every safety, key-collision, timing, and target issue found.
pub fn inspect_script_validation(script: &ScriptDocument) -> ValidationReport {
    let mut report = ValidationReport::default();

    validate_script_name(&script.name, &mut report);
    validate_target_scope(&script.target_scope, &mut report);
    validate_kill_switch_and_key_bindings(script, &mut report);

    match script.mode {
        ScriptMode::VisualBlocks => validate_visual_program(script, &mut report),
        ScriptMode::AdvancedRust => validate_rust_source(&script.rust_source, &mut report),
    }

    report
}

/// Enforces the mandatory validation gate before saving, compiling, enabling, or running a script.
pub fn validate_script_for_persistence_and_execution(
    script: &ScriptDocument,
) -> Result<(), ValidationError> {
    let report = inspect_script_validation(script);
    if report.is_valid() {
        return Ok(());
    }
    let first_message = report
        .issues
        .first()
        .map(|issue| issue.message.clone())
        .unwrap_or_else(|| "Unknown validation failure".to_string());
    Err(ValidationError::Failed {
        issue_count: report.issues.len(),
        first_message,
        report,
    })
}

fn validate_script_name(script_name: &str, report: &mut ValidationReport) {
    let trimmed_name = script_name.trim();
    if trimmed_name.is_empty() {
        report.push_issue(
            ValidationIssueCode::EmptyScriptName,
            None,
            "Script name cannot be empty.",
        );
        return;
    }
    if trimmed_name.chars().count() > MAX_SCRIPT_NAME_CHARS {
        report.push_issue(
            ValidationIssueCode::ScriptNameTooLong,
            None,
            format!("Script name cannot exceed {MAX_SCRIPT_NAME_CHARS} characters."),
        );
    }
}

fn validate_target_scope(target_scope: &TargetScope, report: &mut ValidationReport) {
    let TargetScope::SelectedGame(target_rule) = target_scope else {
        return;
    };
    if target_rule.has_match_criteria() {
        return;
    }
    report.push_issue(
        ValidationIssueCode::InvalidSelectedGameTarget,
        None,
        "Selected Game scope requires attaching a target window, executable, or PID.",
    );
}

fn validate_kill_switch_and_key_bindings(script: &ScriptDocument, report: &mut ValidationReport) {
    let Some(kill_switch_key) = script.kill_switch_key else {
        report.push_issue(
            ValidationIssueCode::MissingKillSwitchKey,
            None,
            "Assign a dedicated Hard Kill-Switch key (for example F8) before saving, compiling, or running.",
        );
        return;
    };

    if script.arm_toggle_key == Some(kill_switch_key) {
        report.push_issue(
            ValidationIssueCode::KillSwitchCollidesWithArmToggleKey,
            None,
            format!(
                "Hard Kill-Switch key ({}) cannot match the Arm Toggle key.",
                kill_switch_key.display_label()
            ),
        );
    }

    if script.mode != ScriptMode::VisualBlocks {
        return;
    }

    let all_trigger_keys: Vec<KeyCode> = script
        .visual_program
        .trigger
        .all_trigger_inputs()
        .into_iter()
        .filter_map(InputTrigger::key_code)
        .collect();
    if all_trigger_keys.contains(&kill_switch_key) {
        report.push_issue(
            ValidationIssueCode::KillSwitchCollidesWithTriggerKey,
            None,
            format!(
                "Hard Kill-Switch key ({}) cannot match any script trigger key.",
                kill_switch_key.display_label()
            ),
        );
    }
    if let Some(arm_key) = script.arm_toggle_key
        && all_trigger_keys.contains(&arm_key)
    {
        report.push_issue(
            ValidationIssueCode::ArmToggleCollidesWithTriggerKey,
            None,
            format!(
                "Arm Toggle key ({}) cannot match any script trigger key.",
                arm_key.display_label()
            ),
        );
    }

    let emitted_keys = script.visual_program.collect_emitted_keys();
    if emitted_keys.contains(&kill_switch_key) {
        report.push_issue(
            ValidationIssueCode::KillSwitchCollidesWithEmittedKey,
            None,
            format!(
                "Hard Kill-Switch key ({}) cannot be emitted by any key or Type Text block.",
                kill_switch_key.display_label()
            ),
        );
    }

    let condition_keys = script.visual_program.collect_condition_keys();
    if condition_keys.contains(&kill_switch_key) {
        report.push_issue(
            ValidationIssueCode::KillSwitchCollidesWithConditionKey,
            None,
            format!(
                "Hard Kill-Switch key ({}) cannot be checked inside an input condition.",
                kill_switch_key.display_label()
            ),
        );
    }
}

fn validate_rust_source(rust_source: &str, report: &mut ValidationReport) {
    if rust_source.trim().is_empty() {
        report.push_issue(
            ValidationIssueCode::EmptyRustSource,
            None,
            "Advanced Rust script source cannot be empty.",
        );
        return;
    }
    if rust_source.len() > MAX_RUST_SOURCE_BYTES {
        report.push_issue(
            ValidationIssueCode::RustSourceTooLarge,
            None,
            format!("Advanced Rust script source exceeds limit of {MAX_RUST_SOURCE_BYTES} bytes."),
        );
    }
}

fn validate_visual_program(script: &ScriptDocument, report: &mut ValidationReport) {
    let additional_input_count = script.visual_program.trigger.additional_held_inputs().len();
    if additional_input_count > MAX_TRIGGER_AND_INPUTS {
        report.push_issue(
            ValidationIssueCode::TooManyTriggerInputs,
            None,
            format!(
                "Trigger chord has {additional_input_count} additional inputs, exceeding limit of {MAX_TRIGGER_AND_INPUTS}."
            ),
        );
    }
    let block_count = script.visual_program.total_block_count();
    if block_count > MAX_BLOCKS_PER_SCRIPT {
        report.push_issue(
            ValidationIssueCode::TooManyBlocks,
            None,
            format!(
                "Visual program has {block_count} blocks, exceeding limit of {MAX_BLOCKS_PER_SCRIPT}."
            ),
        );
    }
    let nesting_depth = script.visual_program.max_nesting_depth();
    if nesting_depth > MAX_BLOCK_NESTING_DEPTH {
        report.push_issue(
            ValidationIssueCode::ExcessiveBlockNesting,
            None,
            format!(
                "Visual program nesting depth {nesting_depth} exceeds limit of {MAX_BLOCK_NESTING_DEPTH}."
            ),
        );
    }
    validate_statement_slice(
        &script.visual_program.statements,
        script.kill_switch_key,
        report,
    );
}

fn validate_statement_slice(
    statements: &[StatementBlock],
    kill_switch_key: Option<KeyCode>,
    report: &mut ValidationReport,
) {
    for block in statements {
        validate_single_statement(block, kill_switch_key, report);
    }
}

fn validate_single_statement(
    block: &StatementBlock,
    kill_switch_key: Option<KeyCode>,
    report: &mut ValidationReport,
) {
    let block_id = Some(block.id);
    match &block.kind {
        StatementKind::IfThen {
            condition,
            then_blocks,
        } => {
            validate_condition(condition, block_id, report);
            validate_statement_slice(then_blocks, kill_switch_key, report);
        }
        StatementKind::IfThenElse {
            condition,
            then_blocks,
            else_blocks,
        } => {
            validate_condition(condition, block_id, report);
            validate_statement_slice(then_blocks, kill_switch_key, report);
            validate_statement_slice(else_blocks, kill_switch_key, report);
        }
        StatementKind::WhileCondition {
            condition,
            body_blocks,
        } => {
            validate_condition(condition, block_id, report);
            validate_statement_slice(body_blocks, kill_switch_key, report);
        }
        StatementKind::RepeatCount {
            repeat_count,
            body_blocks,
        } => {
            if *repeat_count == 0 || *repeat_count > MAX_REPEAT_COUNT {
                report.push_issue(
                    ValidationIssueCode::InvalidRepeatCount,
                    block_id,
                    format!("Repeat count must be between 1 and {MAX_REPEAT_COUNT}."),
                );
            }
            validate_statement_slice(body_blocks, kill_switch_key, report);
        }
        StatementKind::MouseClick {
            hold_duration_ms, ..
        }
        | StatementKind::KeyTap {
            hold_duration_ms, ..
        } => {
            if *hold_duration_ms > MAX_HOLD_DURATION_MS {
                report.push_issue(
                    ValidationIssueCode::InvalidTimingRange,
                    block_id,
                    format!(
                        "Hold duration {hold_duration_ms} ms exceeds limit of {MAX_HOLD_DURATION_MS} ms."
                    ),
                );
            }
        }
        StatementKind::TypeText {
            text,
            keystroke_delay_ms,
            jitter_ms,
        } => {
            if text.chars().count() > MAX_TYPE_TEXT_CHARS {
                report.push_issue(
                    ValidationIssueCode::TypeTextTooLong,
                    block_id,
                    format!("Type Text length exceeds limit of {MAX_TYPE_TEXT_CHARS} characters."),
                );
            }
            if *keystroke_delay_ms > MAX_WAIT_DELAY_MS || *jitter_ms > MAX_WAIT_DELAY_MS {
                report.push_issue(
                    ValidationIssueCode::InvalidTimingRange,
                    block_id,
                    format!("Type Text delay and jitter must be at most {MAX_WAIT_DELAY_MS} ms."),
                );
            }
        }
        StatementKind::WaitFixed { delay_ms } => {
            if *delay_ms > MAX_WAIT_DELAY_MS {
                report.push_issue(
                    ValidationIssueCode::InvalidTimingRange,
                    block_id,
                    format!("Fixed wait {delay_ms} ms exceeds limit of {MAX_WAIT_DELAY_MS} ms."),
                );
            }
        }
        StatementKind::WaitRandomUniform {
            min_delay_ms,
            max_delay_ms,
        } => {
            if min_delay_ms > max_delay_ms || *max_delay_ms > MAX_WAIT_DELAY_MS {
                report.push_issue(
                    ValidationIssueCode::InvalidTimingRange,
                    block_id,
                    format!(
                        "Uniform wait range [{min_delay_ms}..{max_delay_ms}] ms is invalid or exceeds {MAX_WAIT_DELAY_MS} ms."
                    ),
                );
            }
        }
        StatementKind::WaitRandomGaussian {
            base_delay_ms,
            min_offset_ms,
            max_offset_ms,
            mean_offset_ms,
            std_dev_ms,
        } => validate_gaussian_wait_block(
            *base_delay_ms,
            *min_offset_ms,
            *max_offset_ms,
            *mean_offset_ms,
            *std_dev_ms,
            block_id,
            report,
        ),
        StatementKind::SetVariable {
            variable_name,
            value,
        } => validate_variable_mutation(variable_name, *value, block_id, report),
        StatementKind::ChangeVariable {
            variable_name,
            delta_value,
        } => validate_variable_mutation(variable_name, *delta_value, block_id, report),
        StatementKind::BreakLoop
        | StatementKind::StopScript
        | StatementKind::DoNothing
        | StatementKind::MouseDown { .. }
        | StatementKind::MouseUp { .. }
        | StatementKind::MouseMoveRelative { .. }
        | StatementKind::MouseScroll { .. }
        | StatementKind::KeyDown { .. }
        | StatementKind::KeyUp { .. } => {}
    }
}

fn validate_gaussian_wait_block(
    base_delay_ms: u64,
    min_offset_ms: f64,
    max_offset_ms: f64,
    mean_offset_ms: f64,
    std_dev_ms: f64,
    block_id: Option<BlockId>,
    report: &mut ValidationReport,
) {
    if let Err(error) =
        GaussianRangeMs::new(min_offset_ms, max_offset_ms, mean_offset_ms, std_dev_ms)
    {
        report.push_issue(
            ValidationIssueCode::InvalidGaussianParameters,
            block_id,
            format!("Invalid Gaussian wait block: {error}"),
        );
        return;
    }
    let max_total_ms = base_delay_ms.saturating_add(max_offset_ms.ceil().max(0.0) as u64);
    if max_total_ms > MAX_WAIT_DELAY_MS {
        report.push_issue(
            ValidationIssueCode::InvalidTimingRange,
            block_id,
            format!(
                "Base wait plus Gaussian upper bound ({max_total_ms} ms) exceeds limit of {MAX_WAIT_DELAY_MS} ms."
            ),
        );
    }
}

fn validate_variable_mutation(
    variable_name: &str,
    numeric_value: f64,
    block_id: Option<BlockId>,
    report: &mut ValidationReport,
) {
    if variable_name.trim().is_empty() {
        report.push_issue(
            ValidationIssueCode::EmptyVariableName,
            block_id,
            "Variable name cannot be empty.",
        );
    }
    if !numeric_value.is_finite() {
        report.push_issue(
            ValidationIssueCode::NonFiniteNumericValue,
            block_id,
            "Variable numeric value must be finite.",
        );
    }
}

fn validate_condition(
    condition: &ConditionExpr,
    block_id: Option<BlockId>,
    report: &mut ValidationReport,
) {
    match condition {
        ConditionExpr::IsInputPressed { .. } => {}
        ConditionExpr::RandomChance { chance_percent } => {
            if !chance_percent.is_finite() || *chance_percent < 0.0 || *chance_percent > 100.0 {
                report.push_issue(
                    ValidationIssueCode::InvalidProbabilityRange,
                    block_id,
                    "Random chance percentage must fall between 0 and 100.",
                );
            }
        }
        ConditionExpr::CompareVariable {
            variable_name,
            target_value,
            ..
        } => validate_variable_mutation(variable_name, *target_value, block_id, report),
        ConditionExpr::Not { inner_condition } => {
            validate_condition(inner_condition, block_id, report);
        }
        ConditionExpr::And {
            left_condition,
            right_condition,
        }
        | ConditionExpr::Or {
            left_condition,
            right_condition,
        } => {
            validate_condition(left_condition, block_id, report);
            validate_condition(right_condition, block_id, report);
        }
    }
}

#[cfg(test)]
mod tests {
    use clickforge_sdk::MouseButton;

    use super::*;
    use crate::models::blocks::TriggerBlock;
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
}
