use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};

use super::*;
use crate::models::{ComparisonOperator, InputError};

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecordedEvent {
    MouseDown(MouseButton),
    MouseUp(MouseButton),
    MouseMove(i32, i32),
    MouseScroll(i32),
    KeyDown(KeyCode),
    KeyUp(KeyCode),
    ReleaseAll,
}

#[derive(Default)]
struct MockInputSink {
    events: Mutex<Vec<RecordedEvent>>,
}

impl MockInputSink {
    fn snapshot_events(&self) -> Vec<RecordedEvent> {
        match self.events.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

impl InputSink for MockInputSink {
    fn mouse_down(&self, mouse_button: MouseButton) -> Result<(), InputError> {
        if let Ok(mut guard) = self.events.lock() {
            guard.push(RecordedEvent::MouseDown(mouse_button));
        }
        Ok(())
    }

    fn mouse_up(&self, mouse_button: MouseButton) -> Result<(), InputError> {
        if let Ok(mut guard) = self.events.lock() {
            guard.push(RecordedEvent::MouseUp(mouse_button));
        }
        Ok(())
    }

    fn mouse_move_relative(&self, delta_x_px: i32, delta_y_px: i32) -> Result<(), InputError> {
        if let Ok(mut guard) = self.events.lock() {
            guard.push(RecordedEvent::MouseMove(delta_x_px, delta_y_px));
        }
        Ok(())
    }

    fn mouse_scroll(&self, vertical_steps: i32) -> Result<(), InputError> {
        if let Ok(mut guard) = self.events.lock() {
            guard.push(RecordedEvent::MouseScroll(vertical_steps));
        }
        Ok(())
    }

    fn key_down(&self, key_code: KeyCode) -> Result<(), InputError> {
        if let Ok(mut guard) = self.events.lock() {
            guard.push(RecordedEvent::KeyDown(key_code));
        }
        Ok(())
    }

    fn key_up(&self, key_code: KeyCode) -> Result<(), InputError> {
        if let Ok(mut guard) = self.events.lock() {
            guard.push(RecordedEvent::KeyUp(key_code));
        }
        Ok(())
    }

    fn release_all_pressed(&self) -> Result<(), InputError> {
        if let Ok(mut guard) = self.events.lock() {
            guard.push(RecordedEvent::ReleaseAll);
        }
        Ok(())
    }
}

#[derive(Default)]
struct MockPhysicalInput {
    is_left_mouse_held: AtomicBool,
    is_space_held: AtomicBool,
}

impl PhysicalInputReader for MockPhysicalInput {
    fn is_mouse_pressed(&self, mouse_button: MouseButton) -> bool {
        mouse_button == MouseButton::Left && self.is_left_mouse_held.load(Ordering::SeqCst)
    }

    fn is_key_pressed(&self, key_code: KeyCode) -> bool {
        key_code == KeyCode::Space && self.is_space_held.load(Ordering::SeqCst)
    }
}

#[test]
fn executes_variables_repeat_branching_type_text_and_stops_cleanly() -> Result<(), String> {
    let mut program = VisualProgram::new(TriggerBlock::LoopWhileActive);
    program.append_root_block(StatementKind::SetVariable {
        variable_name: "clicks".to_string(),
        value: 0.0,
    });
    let increment_block = program.create_block(StatementKind::ChangeVariable {
        variable_name: "clicks".to_string(),
        delta_value: 1.0,
    });
    let click_block = program.create_block(StatementKind::MouseClick {
        mouse_button: MouseButton::Left,
        hold_duration_ms: 1,
    });
    program.append_root_block(StatementKind::RepeatCount {
        repeat_count: 3,
        body_blocks: vec![increment_block, click_block],
    });
    let then_block = program.create_block(StatementKind::TypeText {
        text: "Hi".to_string(),
        keystroke_delay_ms: 0,
        jitter_ms: 0,
    });
    let else_block = program.create_block(StatementKind::KeyTap {
        key_code: KeyCode::Escape,
        hold_duration_ms: 1,
    });
    program.append_root_block(StatementKind::IfThenElse {
        condition: ConditionExpr::CompareVariable {
            variable_name: "clicks".to_string(),
            operator: ComparisonOperator::Equal,
            target_value: 3.0,
        },
        then_blocks: vec![then_block],
        else_blocks: vec![else_block],
    });
    program.append_root_block(StatementKind::StopScript);

    let mock_sink = Arc::new(MockInputSink::default());
    let mock_physical = Arc::new(MockPhysicalInput::default());
    let abort_flag = AtomicBool::new(false);
    let executed_action_count = Arc::new(AtomicU64::new(0));
    let hook_action_counter = Arc::clone(&executed_action_count);
    let mut hooks = BlockExecutionHooks {
        on_active_block_changed: None,
        on_delay_sampled_ms: None,
        on_action_executed: Some(Box::new(move || {
            hook_action_counter.fetch_add(1, Ordering::SeqCst);
        })),
    };

    let stop_reason = execute_visual_program(
        &program,
        Arc::clone(&mock_sink) as Arc<dyn InputSink>,
        mock_physical,
        &abort_flag,
        &mut hooks,
    )?;
    assert_eq!(stop_reason, ProgramStopReason::StoppedByBlock);

    let events = mock_sink.snapshot_events();
    let left_click_downs = events
        .iter()
        .filter(|event| **event == RecordedEvent::MouseDown(MouseButton::Left))
        .count();
    assert_eq!(left_click_downs, 3);
    assert!(events.contains(&RecordedEvent::KeyDown(KeyCode::LeftShift)));
    assert!(events.contains(&RecordedEvent::KeyDown(KeyCode::H)));
    assert!(events.contains(&RecordedEvent::KeyDown(KeyCode::I)));
    assert!(!events.contains(&RecordedEvent::KeyDown(KeyCode::Escape)));
    assert_eq!(events.last(), Some(&RecordedEvent::ReleaseAll));
    assert_eq!(executed_action_count.load(Ordering::SeqCst), 5);
    Ok(())
}

#[test]
fn while_input_held_aborts_immediately_and_releases_inputs() -> Result<(), String> {
    let mut program = VisualProgram::new(TriggerBlock::WhileInputHeld {
        trigger_input: InputTrigger::Mouse(MouseButton::Left),
    });
    program.append_root_block(StatementKind::WaitRandomGaussian {
        base_delay_ms: 5,
        min_offset_ms: 1.0,
        max_offset_ms: 3.0,
        mean_offset_ms: 2.0,
        std_dev_ms: 0.5,
    });
    program.append_root_block(StatementKind::MouseClick {
        mouse_button: MouseButton::Left,
        hold_duration_ms: 1,
    });

    let mock_sink = Arc::new(MockInputSink::default());
    let mock_physical = Arc::new(MockPhysicalInput::default());
    mock_physical
        .is_left_mouse_held
        .store(true, Ordering::SeqCst);

    let abort_flag = Arc::new(AtomicBool::new(false));
    let worker_abort = Arc::clone(&abort_flag);
    let worker_sink = Arc::clone(&mock_sink);
    let worker_physical = Arc::clone(&mock_physical);

    let handle = std::thread::spawn(move || {
        let mut hooks = BlockExecutionHooks::default();
        execute_visual_program(
            &program,
            worker_sink,
            worker_physical,
            &worker_abort,
            &mut hooks,
        )
    });

    std::thread::sleep(Duration::from_millis(35));
    abort_flag.store(true, Ordering::SeqCst);

    let stop_reason = match handle.join() {
        Ok(result) => result?,
        Err(_) => return Err("interpreter test thread panicked".to_string()),
    };
    assert_eq!(stop_reason, ProgramStopReason::Aborted);

    let events = mock_sink.snapshot_events();
    assert!(events.contains(&RecordedEvent::MouseDown(MouseButton::Left)));
    assert_eq!(events.last(), Some(&RecordedEvent::ReleaseAll));
    Ok(())
}

#[test]
fn on_input_pressed_fires_once_per_rising_edge_and_break_loop_exits_repeat() -> Result<(), String> {
    let mut program = VisualProgram::new(TriggerBlock::OnInputPressed {
        trigger_input: InputTrigger::Key(KeyCode::Space),
    });
    let click_block = program.create_block(StatementKind::MouseClick {
        mouse_button: MouseButton::Right,
        hold_duration_ms: 1,
    });
    let break_block = program.create_block(StatementKind::BreakLoop);
    program.append_root_block(StatementKind::RepeatCount {
        repeat_count: 5,
        body_blocks: vec![click_block, break_block],
    });
    program.append_root_block(StatementKind::StopScript);

    let mock_sink = Arc::new(MockInputSink::default());
    let mock_physical = Arc::new(MockPhysicalInput::default());
    mock_physical.is_space_held.store(true, Ordering::SeqCst);
    let abort_flag = AtomicBool::new(false);
    let mut hooks = BlockExecutionHooks::default();

    let stop_reason = execute_visual_program(
        &program,
        Arc::clone(&mock_sink) as Arc<dyn InputSink>,
        mock_physical,
        &abort_flag,
        &mut hooks,
    )?;
    assert_eq!(stop_reason, ProgramStopReason::StoppedByBlock);

    let events = mock_sink.snapshot_events();
    let right_downs = events
        .iter()
        .filter(|event| **event == RecordedEvent::MouseDown(MouseButton::Right))
        .count();
    assert_eq!(right_downs, 1);
    Ok(())
}

#[test]
fn variable_map_enforces_maximum_variable_cap() {
    let mut program = VisualProgram::new(TriggerBlock::LoopWhileActive);
    for variable_index in 0..=MAX_SCRIPT_VARIABLES {
        program.append_root_block(StatementKind::SetVariable {
            variable_name: format!("counter_{variable_index}"),
            value: variable_index as f64,
        });
    }
    let mock_sink = Arc::new(MockInputSink::default());
    let mock_physical = Arc::new(MockPhysicalInput::default());
    let abort_flag = AtomicBool::new(false);
    let mut hooks = BlockExecutionHooks::default();

    let result =
        execute_visual_program(&program, mock_sink, mock_physical, &abort_flag, &mut hooks);
    assert!(result.is_err());
}
