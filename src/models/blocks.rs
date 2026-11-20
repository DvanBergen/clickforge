use clickforge_sdk::{GaussianRangeMs, InputTrigger, KeyCode, MouseButton, ScriptResult};
use serde::{Deserialize, Serialize};

use crate::models::block_tree;

/// Stable unique identifier for a statement block inside a `VisualProgram`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlockId(pub u64);

/// Visual grouping category used by the block palette and canvas renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockCategory {
    Trigger,
    Control,
    Action,
    Timing,
    Variable,
}

/// Maximum number of additional simultaneous chord inputs allowed on a `TriggerBlock`.
pub const MAX_TRIGGER_AND_INPUTS: usize = 4;

/// Top-level trigger controlling when a Visual Block program runs its body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TriggerBlock {
    WhileInputHeld {
        trigger_input: InputTrigger,
        #[serde(default)]
        additional_held_inputs: Vec<InputTrigger>,
    },
    OnInputToggle {
        trigger_input: InputTrigger,
        #[serde(default)]
        additional_held_inputs: Vec<InputTrigger>,
    },
    OnInputPressed {
        trigger_input: InputTrigger,
        #[serde(default)]
        additional_held_inputs: Vec<InputTrigger>,
    },
    LoopWhileActive,
}

impl TriggerBlock {
    /// Returns the physical input trigger bound to this header block, if any.
    pub fn trigger_input(&self) -> Option<InputTrigger> {
        match self {
            Self::WhileInputHeld { trigger_input, .. }
            | Self::OnInputToggle { trigger_input, .. }
            | Self::OnInputPressed { trigger_input, .. } => Some(*trigger_input),
            Self::LoopWhileActive => None,
        }
    }

    /// Returns any additional chord inputs that must be held simultaneously with the primary input.
    pub fn additional_held_inputs(&self) -> &[InputTrigger] {
        match self {
            Self::WhileInputHeld {
                additional_held_inputs,
                ..
            }
            | Self::OnInputToggle {
                additional_held_inputs,
                ..
            }
            | Self::OnInputPressed {
                additional_held_inputs,
                ..
            } => additional_held_inputs.as_slice(),
            Self::LoopWhileActive => &[],
        }
    }

    /// Returns a mutable handle to the additional chord inputs slice when supported by this trigger.
    pub fn additional_held_inputs_mut(&mut self) -> Option<&mut Vec<InputTrigger>> {
        match self {
            Self::WhileInputHeld {
                additional_held_inputs,
                ..
            }
            | Self::OnInputToggle {
                additional_held_inputs,
                ..
            }
            | Self::OnInputPressed {
                additional_held_inputs,
                ..
            } => Some(additional_held_inputs),
            Self::LoopWhileActive => None,
        }
    }

    /// Collects the primary trigger input followed by all additional chord inputs.
    pub fn all_trigger_inputs(&self) -> Vec<InputTrigger> {
        let Some(primary_input) = self.trigger_input() else {
            return Vec::new();
        };
        let additional_inputs = self.additional_held_inputs();
        let mut combined_inputs =
            Vec::with_capacity(1_usize.saturating_add(additional_inputs.len()));
        combined_inputs.push(primary_input);
        combined_inputs.extend_from_slice(additional_inputs);
        combined_inputs
    }

    /// Formats the trigger chord labels joined with ` + `, or `None` for `LoopWhileActive`.
    pub fn chord_display_label(&self) -> Option<String> {
        let combined_inputs = self.all_trigger_inputs();
        if combined_inputs.is_empty() {
            return None;
        }
        let labels: Vec<&'static str> = combined_inputs
            .into_iter()
            .map(InputTrigger::display_label)
            .collect();
        Some(labels.join(" + "))
    }

    /// Returns a readable summary for the block canvas header.
    pub fn display_label(&self) -> String {
        let has_chord_inputs = !self.additional_held_inputs().is_empty();
        let chord_label = self.chord_display_label().unwrap_or_default();
        match self {
            Self::WhileInputHeld { .. } => {
                if has_chord_inputs {
                    format!("While {chord_label} are held")
                } else {
                    format!("While {chord_label} is held")
                }
            }
            Self::OnInputToggle { .. } => {
                if has_chord_inputs {
                    format!("Toggle loop on {chord_label}")
                } else {
                    format!("Toggle loop on {chord_label} press")
                }
            }
            Self::OnInputPressed { .. } => {
                if has_chord_inputs {
                    format!("Once on {chord_label}")
                } else {
                    format!("Once when {chord_label} is pressed")
                }
            }
            Self::LoopWhileActive => "Loop continuously while active".to_string(),
        }
    }
}

/// Numeric comparison operator used by `ConditionExpr::CompareVariable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonOperator {
    Equal,
    NotEqual,
    LessThan,
    LessOrEqual,
    GreaterThan,
    GreaterOrEqual,
}

impl ComparisonOperator {
    /// Returns the compact symbol for block UI dropdowns.
    pub fn display_symbol(self) -> &'static str {
        match self {
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::LessThan => "<",
            Self::LessOrEqual => "<=",
            Self::GreaterThan => ">",
            Self::GreaterOrEqual => ">=",
        }
    }

    /// Evaluates this comparison against two finite floating-point numbers.
    pub fn evaluate(self, left_value: f64, right_value: f64) -> bool {
        const FLOAT_TOLERANCE: f64 = 1e-9;
        match self {
            Self::Equal => (left_value - right_value).abs() <= FLOAT_TOLERANCE,
            Self::NotEqual => (left_value - right_value).abs() > FLOAT_TOLERANCE,
            Self::LessThan => left_value < right_value,
            Self::LessOrEqual => left_value <= right_value,
            Self::GreaterThan => left_value > right_value,
            Self::GreaterOrEqual => left_value >= right_value,
        }
    }
}

/// Boolean condition tree evaluated by `IfThen`, `IfThenElse`, and `WhileCondition` blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConditionExpr {
    IsInputPressed {
        input: InputTrigger,
    },
    RandomChance {
        chance_percent: f64,
    },
    CompareVariable {
        variable_name: String,
        operator: ComparisonOperator,
        target_value: f64,
    },
    Not {
        inner_condition: Box<ConditionExpr>,
    },
    And {
        left_condition: Box<ConditionExpr>,
        right_condition: Box<ConditionExpr>,
    },
    Or {
        left_condition: Box<ConditionExpr>,
        right_condition: Box<ConditionExpr>,
    },
}

impl ConditionExpr {
    /// Formats a readable summary for block condition pills.
    pub fn display_label(&self) -> String {
        match self {
            Self::IsInputPressed { input } => format!("{} is pressed", input.display_label()),
            Self::RandomChance { chance_percent } => format!("{chance_percent:.1}% chance"),
            Self::CompareVariable {
                variable_name,
                operator,
                target_value,
            } => format!(
                "{} {} {}",
                variable_name,
                operator.display_symbol(),
                target_value
            ),
            Self::Not { inner_condition } => format!("NOT ({})", inner_condition.display_label()),
            Self::And {
                left_condition,
                right_condition,
            } => format!(
                "({}) AND ({})",
                left_condition.display_label(),
                right_condition.display_label()
            ),
            Self::Or {
                left_condition,
                right_condition,
            } => format!(
                "({}) OR ({})",
                left_condition.display_label(),
                right_condition.display_label()
            ),
        }
    }

    /// Collects keyboard keys referenced inside `IsInputPressed` condition nodes.
    pub fn collect_referenced_keys(&self, collected_keys: &mut Vec<KeyCode>) {
        match self {
            Self::IsInputPressed {
                input: InputTrigger::Key(key_code),
            } => block_tree::push_unique_key(collected_keys, *key_code),
            Self::IsInputPressed {
                input: InputTrigger::Mouse(_),
            }
            | Self::RandomChance { .. }
            | Self::CompareVariable { .. } => {}
            Self::Not { inner_condition } => {
                inner_condition.collect_referenced_keys(collected_keys);
            }
            Self::And {
                left_condition,
                right_condition,
            }
            | Self::Or {
                left_condition,
                right_condition,
            } => {
                left_condition.collect_referenced_keys(collected_keys);
                right_condition.collect_referenced_keys(collected_keys);
            }
        }
    }
}

/// Complete Scratch-style statement AST supported by ClickForge v1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StatementKind {
    IfThen {
        condition: ConditionExpr,
        then_blocks: Vec<StatementBlock>,
    },
    IfThenElse {
        condition: ConditionExpr,
        then_blocks: Vec<StatementBlock>,
        else_blocks: Vec<StatementBlock>,
    },
    WhileCondition {
        condition: ConditionExpr,
        body_blocks: Vec<StatementBlock>,
    },
    RepeatCount {
        repeat_count: u32,
        body_blocks: Vec<StatementBlock>,
    },
    BreakLoop,
    StopScript,
    DoNothing,
    MouseClick {
        mouse_button: MouseButton,
        hold_duration_ms: u64,
    },
    MouseDown {
        mouse_button: MouseButton,
    },
    MouseUp {
        mouse_button: MouseButton,
    },
    MouseMoveRelative {
        delta_x_px: i32,
        delta_y_px: i32,
    },
    MouseScroll {
        vertical_steps: i32,
    },
    KeyTap {
        key_code: KeyCode,
        hold_duration_ms: u64,
    },
    KeyDown {
        key_code: KeyCode,
    },
    KeyUp {
        key_code: KeyCode,
    },
    TypeText {
        text: String,
        keystroke_delay_ms: u64,
        jitter_ms: u64,
    },
    WaitFixed {
        delay_ms: u64,
    },
    WaitRandomUniform {
        min_delay_ms: u64,
        max_delay_ms: u64,
    },
    WaitRandomGaussian {
        base_delay_ms: u64,
        min_offset_ms: f64,
        max_offset_ms: f64,
        mean_offset_ms: f64,
        std_dev_ms: f64,
    },
    SetVariable {
        variable_name: String,
        value: f64,
    },
    ChangeVariable {
        variable_name: String,
        delta_value: f64,
    },
}

impl StatementKind {
    /// Returns the visual block category used for color coding and palette grouping.
    pub fn category(&self) -> BlockCategory {
        match self {
            Self::IfThen { .. }
            | Self::IfThenElse { .. }
            | Self::WhileCondition { .. }
            | Self::RepeatCount { .. }
            | Self::BreakLoop
            | Self::StopScript
            | Self::DoNothing => BlockCategory::Control,
            Self::MouseClick { .. }
            | Self::MouseDown { .. }
            | Self::MouseUp { .. }
            | Self::MouseMoveRelative { .. }
            | Self::MouseScroll { .. }
            | Self::KeyTap { .. }
            | Self::KeyDown { .. }
            | Self::KeyUp { .. }
            | Self::TypeText { .. } => BlockCategory::Action,
            Self::WaitFixed { .. }
            | Self::WaitRandomUniform { .. }
            | Self::WaitRandomGaussian { .. } => BlockCategory::Timing,
            Self::SetVariable { .. } | Self::ChangeVariable { .. } => BlockCategory::Variable,
        }
    }

    /// Returns a concise title suitable for palette entries and canvas headers.
    pub fn title(&self) -> &'static str {
        match self {
            Self::IfThen { .. } => "If",
            Self::IfThenElse { .. } => "If / Else",
            Self::WhileCondition { .. } => "While",
            Self::RepeatCount { .. } => "Repeat",
            Self::BreakLoop => "Break Loop",
            Self::StopScript => "Stop Script",
            Self::DoNothing => "Do Nothing",
            Self::MouseClick { .. } => "Mouse Click",
            Self::MouseDown { .. } => "Mouse Down",
            Self::MouseUp { .. } => "Mouse Up",
            Self::MouseMoveRelative { .. } => "Mouse Move",
            Self::MouseScroll { .. } => "Mouse Scroll",
            Self::KeyTap { .. } => "Key Tap",
            Self::KeyDown { .. } => "Key Down",
            Self::KeyUp { .. } => "Key Up",
            Self::TypeText { .. } => "Type Text",
            Self::WaitFixed { .. } => "Wait Fixed",
            Self::WaitRandomUniform { .. } => "Wait Uniform",
            Self::WaitRandomGaussian { .. } => "Wait Gaussian",
            Self::SetVariable { .. } => "Set Variable",
            Self::ChangeVariable { .. } => "Change Variable",
        }
    }

    /// Builds a validated `GaussianRangeMs` when this block is `WaitRandomGaussian`.
    pub fn gaussian_offset_range_ms(&self) -> Option<ScriptResult<GaussianRangeMs>> {
        match self {
            Self::WaitRandomGaussian {
                min_offset_ms,
                max_offset_ms,
                mean_offset_ms,
                std_dev_ms,
                ..
            } => Some(GaussianRangeMs::new(
                *min_offset_ms,
                *max_offset_ms,
                *mean_offset_ms,
                *std_dev_ms,
            )),
            _ => None,
        }
    }
}

/// Single uniquely identified statement block node inside a `VisualProgram`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatementBlock {
    pub id: BlockId,
    pub kind: StatementKind,
}

/// Insertion target describing a position in the root statement list or a nested C-block branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockSlotTarget {
    Root {
        insert_index: usize,
    },
    ThenBranch {
        parent_block_id: BlockId,
        insert_index: usize,
    },
    ElseBranch {
        parent_block_id: BlockId,
        insert_index: usize,
    },
    LoopBody {
        parent_block_id: BlockId,
        insert_index: usize,
    },
}

/// Complete Visual Block script program consisting of a trigger header and nested statement tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualProgram {
    pub trigger: TriggerBlock,
    pub statements: Vec<StatementBlock>,
    pub next_block_id: u64,
}

impl VisualProgram {
    /// Creates an empty visual program with the supplied trigger header.
    pub fn new(trigger: TriggerBlock) -> Self {
        Self {
            trigger,
            statements: Vec::new(),
            next_block_id: 1,
        }
    }

    /// Allocates the next monotonic `BlockId` without integer overflow.
    pub fn allocate_block_id(&mut self) -> BlockId {
        let allocated_id = BlockId(self.next_block_id);
        self.next_block_id = self.next_block_id.saturating_add(1);
        allocated_id
    }

    /// Allocates a new `StatementBlock` with a fresh `BlockId`.
    pub fn create_block(&mut self, kind: StatementKind) -> StatementBlock {
        let block_id = self.allocate_block_id();
        StatementBlock { id: block_id, kind }
    }

    /// Appends a new statement block at the end of the root statement list and returns its ID.
    pub fn append_root_block(&mut self, kind: StatementKind) -> BlockId {
        let block = self.create_block(kind);
        let block_id = block.id;
        self.statements.push(block);
        block_id
    }

    /// Inserts an existing statement block into the specified root or nested container slot.
    pub fn insert_block(&mut self, target_slot: BlockSlotTarget, block: StatementBlock) -> bool {
        let Some((target_slice, insert_index)) =
            block_tree::resolve_target_slice_mut(&mut self.statements, target_slot)
        else {
            return false;
        };
        let clamped_index = insert_index.min(target_slice.len());
        target_slice.insert(clamped_index, block);
        true
    }

    /// Removes and returns the statement block with `target_block_id` anywhere in the tree.
    pub fn remove_block(&mut self, target_block_id: BlockId) -> Option<StatementBlock> {
        block_tree::remove_from_slice(&mut self.statements, target_block_id)
    }

    /// Moves a block from its current location to `target_slot`, preventing self-nesting cycles.
    pub fn move_block(&mut self, source_block_id: BlockId, target_slot: BlockSlotTarget) -> bool {
        if self.would_create_cycle(source_block_id, target_slot) {
            return false;
        }
        let adjusted_slot = block_tree::adjust_slot_for_same_parent_move(
            &self.statements,
            source_block_id,
            target_slot,
        );
        let Some(removed_block) = self.remove_block(source_block_id) else {
            return false;
        };
        if self.insert_block(adjusted_slot, removed_block.clone()) {
            return true;
        }
        self.statements.push(removed_block);
        false
    }

    /// Finds an immutable reference to a block anywhere in the tree.
    pub fn find_block(&self, target_block_id: BlockId) -> Option<&StatementBlock> {
        block_tree::find_in_slice(&self.statements, target_block_id)
    }

    /// Finds a mutable reference to a block anywhere in the tree.
    pub fn find_block_mut(&mut self, target_block_id: BlockId) -> Option<&mut StatementBlock> {
        block_tree::find_in_slice_mut(&mut self.statements, target_block_id)
    }

    /// Returns true if a block with `target_block_id` exists anywhere in the program.
    pub fn contains_block_id(&self, target_block_id: BlockId) -> bool {
        self.find_block(target_block_id).is_some()
    }

    /// Counts all statement blocks recursively across root and nested branches.
    pub fn total_block_count(&self) -> usize {
        block_tree::count_blocks_in_slice(&self.statements)
    }

    /// Computes the maximum nesting depth of control containers inside this program.
    pub fn max_nesting_depth(&self) -> usize {
        block_tree::max_depth_in_slice(&self.statements, 0)
    }

    /// Collects every distinct keyboard key emitted by key actions and `TypeText` blocks.
    pub fn collect_emitted_keys(&self) -> Vec<KeyCode> {
        let mut emitted_keys = Vec::new();
        block_tree::collect_emitted_keys_in_slice(&self.statements, &mut emitted_keys);
        emitted_keys
    }

    /// Collects every distinct keyboard key referenced inside `IsInputPressed` conditions.
    pub fn collect_condition_keys(&self) -> Vec<KeyCode> {
        let mut condition_keys = Vec::new();
        block_tree::collect_condition_keys_in_slice(&self.statements, &mut condition_keys);
        condition_keys
    }

    /// Collects every distinct mouse button emitted by mouse action blocks.
    pub fn collect_emitted_mouse_buttons(&self) -> Vec<MouseButton> {
        let mut emitted_buttons = Vec::new();
        block_tree::collect_emitted_buttons_in_slice(&self.statements, &mut emitted_buttons);
        emitted_buttons
    }

    fn would_create_cycle(&self, source_block_id: BlockId, target_slot: BlockSlotTarget) -> bool {
        let Some(target_parent_id) = block_tree::slot_parent_block_id(target_slot) else {
            return false;
        };
        if target_parent_id == source_block_id {
            return true;
        }
        self.find_block(source_block_id)
            .is_some_and(|source_block| {
                block_tree::block_subtree_contains_id(source_block, target_parent_id)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_program_supports_nested_insert_move_remove_and_emitted_keys() {
        let mut program = VisualProgram::new(TriggerBlock::WhileInputHeld {
            trigger_input: InputTrigger::Mouse(MouseButton::Left),
            additional_held_inputs: Vec::new(),
        });
        let loop_id = program.append_root_block(StatementKind::RepeatCount {
            repeat_count: 3,
            body_blocks: Vec::new(),
        });
        let tap_block = program.create_block(StatementKind::KeyTap {
            key_code: KeyCode::E,
            hold_duration_ms: 15,
        });
        let tap_id = tap_block.id;
        assert!(program.insert_block(
            BlockSlotTarget::LoopBody {
                parent_block_id: loop_id,
                insert_index: 0,
            },
            tap_block,
        ));

        let type_id = program.append_root_block(StatementKind::TypeText {
            text: "Go!".to_string(),
            keystroke_delay_ms: 15,
            jitter_ms: 5,
        });

        assert!(!program.move_block(
            loop_id,
            BlockSlotTarget::LoopBody {
                parent_block_id: loop_id,
                insert_index: 0,
            }
        ));

        assert!(program.move_block(
            type_id,
            BlockSlotTarget::LoopBody {
                parent_block_id: loop_id,
                insert_index: 1,
            }
        ));
        assert_eq!(program.total_block_count(), 3);
        assert_eq!(program.max_nesting_depth(), 1);

        let emitted = program.collect_emitted_keys();
        assert!(emitted.contains(&KeyCode::E));
        assert!(emitted.contains(&KeyCode::G));
        assert!(emitted.contains(&KeyCode::O));
        assert!(emitted.contains(&KeyCode::Digit1));
        assert!(emitted.contains(&KeyCode::LeftShift));

        assert!(program.remove_block(tap_id).is_some());
        assert!(!program.contains_block_id(tap_id));
    }

    #[test]
    fn trigger_block_deserializes_legacy_json_and_round_trips_chords_and_do_nothing()
    -> Result<(), serde_json::Error> {
        let legacy_json =
            r#"{"kind":"while_input_held","trigger_input":{"kind":"mouse","code":"left"}}"#;
        let legacy_trigger: TriggerBlock = serde_json::from_str(legacy_json)?;
        assert_eq!(
            legacy_trigger,
            TriggerBlock::WhileInputHeld {
                trigger_input: InputTrigger::Mouse(MouseButton::Left),
                additional_held_inputs: Vec::new(),
            }
        );
        assert_eq!(legacy_trigger.display_label(), "While Left Mouse is held");

        let mut chord_program = VisualProgram::new(TriggerBlock::WhileInputHeld {
            trigger_input: InputTrigger::Mouse(MouseButton::Left),
            additional_held_inputs: vec![InputTrigger::Key(KeyCode::LeftShift)],
        });
        chord_program.append_root_block(StatementKind::DoNothing);
        assert_eq!(
            chord_program.trigger.display_label(),
            "While Left Mouse + Left Shift are held"
        );
        assert_eq!(
            chord_program.trigger.all_trigger_inputs(),
            vec![
                InputTrigger::Mouse(MouseButton::Left),
                InputTrigger::Key(KeyCode::LeftShift),
            ]
        );
        assert_eq!(StatementKind::DoNothing.category(), BlockCategory::Control);
        assert_eq!(StatementKind::DoNothing.title(), "Do Nothing");

        let serialized_program = serde_json::to_string(&chord_program)?;
        let deserialized_program: VisualProgram = serde_json::from_str(&serialized_program)?;
        assert_eq!(deserialized_program, chord_program);
        Ok(())
    }
}
