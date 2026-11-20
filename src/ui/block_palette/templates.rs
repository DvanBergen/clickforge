use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};

use crate::models::{BlockCategory, ConditionExpr, StatementKind, TriggerBlock};
use crate::ui::block_palette::{BlockDragPayload, PaletteAction};

#[derive(Clone, Copy)]
pub(super) enum PaletteTemplateId {
    WhileHeld,
    ToggleInput,
    OncePressed,
    LoopActive,
    IfThen,
    IfThenElse,
    WhileCond,
    RepeatCount,
    BreakLoop,
    StopScript,
    DoNothing,
    MouseClick,
    MouseDown,
    MouseUp,
    MouseMove,
    MouseScroll,
    KeyTap,
    KeyDown,
    KeyUp,
    TypeText,
    WaitFixed,
    WaitUniform,
    WaitGaussian,
    SetVariable,
    ChangeVariable,
}

pub(super) struct PaletteTemplateSpec {
    pub id: PaletteTemplateId,
    pub title: &'static str,
    pub summary: &'static str,
    pub category: BlockCategory,
    pub is_container: bool,
}

const fn define_template(
    id: PaletteTemplateId,
    title: &'static str,
    summary: &'static str,
    category: BlockCategory,
    is_container: bool,
) -> PaletteTemplateSpec {
    PaletteTemplateSpec {
        id,
        title,
        summary,
        category,
        is_container,
    }
}

pub(super) const PALETTE_TEMPLATES: [PaletteTemplateSpec; 25] = [
    define_template(
        PaletteTemplateId::WhileHeld,
        "While Input Held",
        "Repeats body while mouse button or key is held down",
        BlockCategory::Trigger,
        false,
    ),
    define_template(
        PaletteTemplateId::ToggleInput,
        "Toggle on Input",
        "Starts or stops the loop each time the trigger is pressed",
        BlockCategory::Trigger,
        false,
    ),
    define_template(
        PaletteTemplateId::OncePressed,
        "Once on Press",
        "Executes the sequence once per trigger press",
        BlockCategory::Trigger,
        false,
    ),
    define_template(
        PaletteTemplateId::LoopActive,
        "Loop While Active",
        "Loops continuously while target window is focused and script is armed",
        BlockCategory::Trigger,
        false,
    ),
    define_template(
        PaletteTemplateId::IfThen,
        "If",
        "Runs nested blocks when condition is true",
        BlockCategory::Control,
        true,
    ),
    define_template(
        PaletteTemplateId::IfThenElse,
        "If / Else",
        "Branches between Then and Else child block stacks",
        BlockCategory::Control,
        true,
    ),
    define_template(
        PaletteTemplateId::WhileCond,
        "While",
        "Repeats nested blocks while condition stays true",
        BlockCategory::Control,
        true,
    ),
    define_template(
        PaletteTemplateId::RepeatCount,
        "Repeat N Times",
        "Repeats nested blocks a fixed number of iterations",
        BlockCategory::Control,
        true,
    ),
    define_template(
        PaletteTemplateId::BreakLoop,
        "Break Loop",
        "Exits the innermost Repeat or While loop",
        BlockCategory::Control,
        false,
    ),
    define_template(
        PaletteTemplateId::StopScript,
        "Stop Script",
        "Stops the current script pass immediately",
        BlockCategory::Control,
        false,
    ),
    define_template(
        PaletteTemplateId::DoNothing,
        "Do Nothing",
        "Explicit placeholder block that performs no action (useful inside If / Else branches)",
        BlockCategory::Control,
        false,
    ),
    define_template(
        PaletteTemplateId::MouseClick,
        "Mouse Click",
        "Presses and releases a mouse button with hold duration",
        BlockCategory::Action,
        false,
    ),
    define_template(
        PaletteTemplateId::MouseDown,
        "Mouse Down",
        "Holds down a mouse button until released",
        BlockCategory::Action,
        false,
    ),
    define_template(
        PaletteTemplateId::MouseUp,
        "Mouse Up",
        "Releases a held mouse button",
        BlockCategory::Action,
        false,
    ),
    define_template(
        PaletteTemplateId::MouseMove,
        "Mouse Move",
        "Moves the pointer by a relative pixel offset",
        BlockCategory::Action,
        false,
    ),
    define_template(
        PaletteTemplateId::MouseScroll,
        "Mouse Scroll",
        "Scrolls mouse wheel vertically (positive = up, negative = down)",
        BlockCategory::Action,
        false,
    ),
    define_template(
        PaletteTemplateId::KeyTap,
        "Key Tap",
        "Presses and releases a keyboard key with hold duration",
        BlockCategory::Action,
        false,
    ),
    define_template(
        PaletteTemplateId::KeyDown,
        "Key Down",
        "Holds down a keyboard key until released",
        BlockCategory::Action,
        false,
    ),
    define_template(
        PaletteTemplateId::KeyUp,
        "Key Up",
        "Releases a held keyboard key",
        BlockCategory::Action,
        false,
    ),
    define_template(
        PaletteTemplateId::TypeText,
        "Type Text",
        "Types ASCII text with per-keystroke delay and jitter",
        BlockCategory::Action,
        false,
    ),
    define_template(
        PaletteTemplateId::WaitFixed,
        "Wait Fixed",
        "Pauses for a constant duration in milliseconds",
        BlockCategory::Timing,
        false,
    ),
    define_template(
        PaletteTemplateId::WaitUniform,
        "Wait Uniform",
        "Pauses for a random uniform delay between min and max ms",
        BlockCategory::Timing,
        false,
    ),
    define_template(
        PaletteTemplateId::WaitGaussian,
        "Wait Gaussian",
        "Pauses for base ms plus a clamped Gaussian jitter offset",
        BlockCategory::Timing,
        false,
    ),
    define_template(
        PaletteTemplateId::SetVariable,
        "Set Variable",
        "Assigns a numeric value to a local script variable",
        BlockCategory::Variable,
        false,
    ),
    define_template(
        PaletteTemplateId::ChangeVariable,
        "Change Variable",
        "Adds a delta to a local script variable",
        BlockCategory::Variable,
        false,
    ),
];

pub(super) fn build_template_action_and_payload(
    template_id: PaletteTemplateId,
) -> (PaletteAction, BlockDragPayload) {
    match template_id {
        PaletteTemplateId::WhileHeld => {
            let trigger = TriggerBlock::WhileInputHeld {
                trigger_input: InputTrigger::Mouse(MouseButton::Left),
                additional_held_inputs: Vec::new(),
            };
            (
                PaletteAction::ApplyTrigger(trigger.clone()),
                BlockDragPayload::ReplaceTrigger(trigger),
            )
        }
        PaletteTemplateId::ToggleInput => {
            let trigger = TriggerBlock::OnInputToggle {
                trigger_input: InputTrigger::Key(KeyCode::F5),
                additional_held_inputs: Vec::new(),
            };
            (
                PaletteAction::ApplyTrigger(trigger.clone()),
                BlockDragPayload::ReplaceTrigger(trigger),
            )
        }
        PaletteTemplateId::OncePressed => {
            let trigger = TriggerBlock::OnInputPressed {
                trigger_input: InputTrigger::Mouse(MouseButton::Side),
                additional_held_inputs: Vec::new(),
            };
            (
                PaletteAction::ApplyTrigger(trigger.clone()),
                BlockDragPayload::ReplaceTrigger(trigger),
            )
        }
        PaletteTemplateId::LoopActive => (
            PaletteAction::ApplyTrigger(TriggerBlock::LoopWhileActive),
            BlockDragPayload::ReplaceTrigger(TriggerBlock::LoopWhileActive),
        ),
        other => {
            let statement_kind = build_default_statement_kind(other);
            (
                PaletteAction::AppendStatement(statement_kind.clone()),
                BlockDragPayload::NewStatement(statement_kind),
            )
        }
    }
}

fn build_default_statement_kind(template_id: PaletteTemplateId) -> StatementKind {
    let default_left_click = ConditionExpr::IsInputPressed {
        input: InputTrigger::Mouse(MouseButton::Left),
    };
    match template_id {
        PaletteTemplateId::IfThen => StatementKind::IfThen {
            condition: default_left_click,
            then_blocks: Vec::new(),
        },
        PaletteTemplateId::IfThenElse => StatementKind::IfThenElse {
            condition: ConditionExpr::RandomChance {
                chance_percent: 50.0,
            },
            then_blocks: Vec::new(),
            else_blocks: Vec::new(),
        },
        PaletteTemplateId::WhileCond => StatementKind::WhileCondition {
            condition: default_left_click,
            body_blocks: Vec::new(),
        },
        PaletteTemplateId::RepeatCount => StatementKind::RepeatCount {
            repeat_count: 5,
            body_blocks: Vec::new(),
        },
        PaletteTemplateId::BreakLoop => StatementKind::BreakLoop,
        PaletteTemplateId::StopScript => StatementKind::StopScript,
        PaletteTemplateId::DoNothing => StatementKind::DoNothing,
        PaletteTemplateId::MouseClick => StatementKind::MouseClick {
            mouse_button: MouseButton::Left,
            hold_duration_ms: 12,
        },
        PaletteTemplateId::MouseDown => StatementKind::MouseDown {
            mouse_button: MouseButton::Left,
        },
        PaletteTemplateId::MouseUp => StatementKind::MouseUp {
            mouse_button: MouseButton::Left,
        },
        PaletteTemplateId::MouseMove => StatementKind::MouseMoveRelative {
            delta_x_px: 0,
            delta_y_px: 0,
        },
        PaletteTemplateId::MouseScroll => StatementKind::MouseScroll { vertical_steps: 1 },
        PaletteTemplateId::KeyTap => StatementKind::KeyTap {
            key_code: KeyCode::E,
            hold_duration_ms: 18,
        },
        PaletteTemplateId::KeyDown => StatementKind::KeyDown {
            key_code: KeyCode::LeftShift,
        },
        PaletteTemplateId::KeyUp => StatementKind::KeyUp {
            key_code: KeyCode::LeftShift,
        },
        PaletteTemplateId::TypeText => StatementKind::TypeText {
            text: "gg".to_string(),
            keystroke_delay_ms: 35,
            jitter_ms: 10,
        },
        PaletteTemplateId::WaitFixed => StatementKind::WaitFixed { delay_ms: 50 },
        PaletteTemplateId::WaitUniform => StatementKind::WaitRandomUniform {
            min_delay_ms: 40,
            max_delay_ms: 80,
        },
        PaletteTemplateId::WaitGaussian => StatementKind::WaitRandomGaussian {
            base_delay_ms: 45,
            min_offset_ms: 10.0,
            max_offset_ms: 20.0,
            mean_offset_ms: 15.0,
            std_dev_ms: 2.5,
        },
        PaletteTemplateId::SetVariable => StatementKind::SetVariable {
            variable_name: "counter".to_string(),
            value: 0.0,
        },
        PaletteTemplateId::ChangeVariable => StatementKind::ChangeVariable {
            variable_name: "counter".to_string(),
            delta_value: 1.0,
        },
        _ => StatementKind::WaitFixed { delay_ms: 50 },
    }
}
