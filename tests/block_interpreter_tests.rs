use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use clickforge::engine::block_interpreter::{
    BlockExecutionHooks, ProgramStopReason, execute_visual_program,
};
use clickforge::models::{
    BlockId, ComparisonOperator, ConditionExpr, InputError, InputSink, PhysicalInputReader,
    ScriptDocument, StatementKind, TriggerBlock, VisualProgram,
};
use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecordedSinkEvent {
    MouseDown(MouseButton),
    MouseUp(MouseButton),
    MouseMove(i32, i32),
    MouseScroll(i32),
    KeyDown(KeyCode),
    KeyUp(KeyCode),
    ReleaseAllPressed,
}

#[derive(Default)]
struct RecordingInputSink {
    events: Mutex<Vec<RecordedSinkEvent>>,
    release_all_count: AtomicUsize,
}

impl RecordingInputSink {
    fn snapshot_events(&self) -> Vec<RecordedSinkEvent> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn left_click_pair_count(&self) -> usize {
        self.snapshot_events()
            .iter()
            .filter(|event| **event == RecordedSinkEvent::MouseUp(MouseButton::Left))
            .count()
    }

    fn release_all_calls(&self) -> usize {
        self.release_all_count.load(Ordering::SeqCst)
    }

    fn push_event(&self, event: RecordedSinkEvent) {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(event);
    }
}

impl InputSink for RecordingInputSink {
    fn mouse_down(&self, mouse_button: MouseButton) -> Result<(), InputError> {
        self.push_event(RecordedSinkEvent::MouseDown(mouse_button));
        Ok(())
    }

    fn mouse_up(&self, mouse_button: MouseButton) -> Result<(), InputError> {
        self.push_event(RecordedSinkEvent::MouseUp(mouse_button));
        Ok(())
    }

    fn mouse_move_relative(&self, delta_x_px: i32, delta_y_px: i32) -> Result<(), InputError> {
        self.push_event(RecordedSinkEvent::MouseMove(delta_x_px, delta_y_px));
        Ok(())
    }

    fn mouse_scroll(&self, vertical_steps: i32) -> Result<(), InputError> {
        self.push_event(RecordedSinkEvent::MouseScroll(vertical_steps));
        Ok(())
    }

    fn key_down(&self, key_code: KeyCode) -> Result<(), InputError> {
        self.push_event(RecordedSinkEvent::KeyDown(key_code));
        Ok(())
    }

    fn key_up(&self, key_code: KeyCode) -> Result<(), InputError> {
        self.push_event(RecordedSinkEvent::KeyUp(key_code));
        Ok(())
    }

    fn release_all_pressed(&self) -> Result<(), InputError> {
        self.release_all_count.fetch_add(1, Ordering::SeqCst);
        self.push_event(RecordedSinkEvent::ReleaseAllPressed);
        Ok(())
    }
}

#[derive(Default)]
struct ControlledPhysicalInput {
    pressed_buttons: Mutex<HashSet<MouseButton>>,
    pressed_keys: Mutex<HashSet<KeyCode>>,
}

impl ControlledPhysicalInput {
    fn set_mouse_pressed(&self, mouse_button: MouseButton, is_pressed: bool) {
        let mut guard = self
            .pressed_buttons
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if is_pressed {
            guard.insert(mouse_button);
        } else {
            guard.remove(&mouse_button);
        }
    }
}

impl PhysicalInputReader for ControlledPhysicalInput {
    fn is_mouse_pressed(&self, mouse_button: MouseButton) -> bool {
        self.pressed_buttons
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&mouse_button)
    }

    fn is_key_pressed(&self, key_code: KeyCode) -> bool {
        self.pressed_keys
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&key_code)
    }
}

#[test]
fn building_upgrade_template_clicks_while_lmb_held_and_aborts_cleanly_on_kill_switch()
-> Result<(), String> {
    let template_script = ScriptDocument::new_building_upgrade_template();
    let input_sink = Arc::new(RecordingInputSink::default());
    let physical_input = Arc::new(ControlledPhysicalInput::default());
    let abort_flag = Arc::new(AtomicBool::new(false));
    let sampled_delays_ms = Arc::new(Mutex::new(Vec::<u64>::new()));
    let observed_blocks = Arc::new(Mutex::new(Vec::<BlockId>::new()));

    let worker_sink: Arc<dyn InputSink> = Arc::clone(&input_sink) as Arc<dyn InputSink>;
    let worker_physical: Arc<dyn PhysicalInputReader> =
        Arc::clone(&physical_input) as Arc<dyn PhysicalInputReader>;
    let worker_abort = Arc::clone(&abort_flag);
    let worker_delays = Arc::clone(&sampled_delays_ms);
    let worker_blocks = Arc::clone(&observed_blocks);
    let visual_program = template_script.visual_program.clone();

    let runner_thread = thread::spawn(move || {
        let mut hooks = BlockExecutionHooks {
            on_active_block_changed: Some(Box::new(move |maybe_block_id| {
                if let Some(block_id) = maybe_block_id {
                    worker_blocks
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(block_id);
                }
            })),
            on_delay_sampled_ms: Some(Box::new(move |delay_ms| {
                worker_delays
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(delay_ms);
            })),
            on_action_executed: None,
        };
        execute_visual_program(
            &visual_program,
            worker_sink,
            worker_physical,
            &worker_abort,
            &mut hooks,
        )
    });

    // While physical LMB is released, zero clicks should fire.
    thread::sleep(Duration::from_millis(30));
    assert_eq!(input_sink.left_click_pair_count(), 0);

    // Hold LMB and wait until at least 2 humanized rapid clicks occur.
    physical_input.set_mouse_pressed(MouseButton::Left, true);
    let wait_deadline = Instant::now() + Duration::from_millis(1_000);
    while input_sink.left_click_pair_count() < 2 && Instant::now() < wait_deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        input_sink.left_click_pair_count() >= 2,
        "expected at least 2 rapid clicks while LMB held"
    );

    // Simulate Hard Kill-Switch abort and verify rapid shutdown and input release.
    let abort_started_at = Instant::now();
    abort_flag.store(true, Ordering::SeqCst);
    let outcome = match runner_thread.join() {
        Ok(result) => result?,
        Err(_) => return Err("interpreter thread panicked".to_string()),
    };
    let abort_elapsed_ms = abort_started_at.elapsed().as_millis();

    assert_eq!(outcome, ProgramStopReason::Aborted);
    assert!(
        abort_elapsed_ms < 250,
        "interpreter abort took {abort_elapsed_ms} ms"
    );
    assert!(input_sink.release_all_calls() >= 1);

    let recorded_delays = sampled_delays_ms
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert!(!recorded_delays.is_empty());
    for delay_ms in recorded_delays {
        assert!(
            (55..=65).contains(&delay_ms),
            "template delay {delay_ms} ms fell outside [55, 65] ms"
        );
    }

    let highlighted_blocks = observed_blocks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert!(!highlighted_blocks.is_empty());
    Ok(())
}

#[test]
fn interpreter_executes_if_else_repeat_variables_and_type_text_in_order() -> Result<(), String> {
    let mut program = VisualProgram::new(TriggerBlock::LoopWhileActive);

    program.append_root_block(StatementKind::SetVariable {
        variable_name: "counter".to_string(),
        value: 0.0,
    });

    let increment_block = program.create_block(StatementKind::ChangeVariable {
        variable_name: "counter".to_string(),
        delta_value: 1.0,
    });
    let tap_even_block = program.create_block(StatementKind::KeyTap {
        key_code: KeyCode::A,
        hold_duration_ms: 0,
    });
    let tap_other_block = program.create_block(StatementKind::KeyTap {
        key_code: KeyCode::B,
        hold_duration_ms: 0,
    });
    let branch_block = program.create_block(StatementKind::IfThenElse {
        condition: ConditionExpr::CompareVariable {
            variable_name: "counter".to_string(),
            operator: ComparisonOperator::LessOrEqual,
            target_value: 2.0,
        },
        then_blocks: vec![tap_even_block],
        else_blocks: vec![tap_other_block],
    });
    program.append_root_block(StatementKind::RepeatCount {
        repeat_count: 3,
        body_blocks: vec![increment_block, branch_block],
    });
    program.append_root_block(StatementKind::TypeText {
        text: "Hi".to_string(),
        keystroke_delay_ms: 0,
        jitter_ms: 0,
    });
    program.append_root_block(StatementKind::StopScript);

    let input_sink = Arc::new(RecordingInputSink::default());
    let physical_input = Arc::new(ControlledPhysicalInput::default());
    let abort_flag = AtomicBool::new(false);
    let mut hooks = BlockExecutionHooks::default();

    let stop_reason = execute_visual_program(
        &program,
        Arc::clone(&input_sink) as Arc<dyn InputSink>,
        physical_input,
        &abort_flag,
        &mut hooks,
    )?;

    assert_eq!(stop_reason, ProgramStopReason::StoppedByBlock);
    assert_eq!(
        input_sink.snapshot_events(),
        vec![
            RecordedSinkEvent::KeyDown(KeyCode::A),
            RecordedSinkEvent::KeyUp(KeyCode::A),
            RecordedSinkEvent::KeyDown(KeyCode::A),
            RecordedSinkEvent::KeyUp(KeyCode::A),
            RecordedSinkEvent::KeyDown(KeyCode::B),
            RecordedSinkEvent::KeyUp(KeyCode::B),
            RecordedSinkEvent::KeyDown(KeyCode::LeftShift),
            RecordedSinkEvent::KeyDown(KeyCode::H),
            RecordedSinkEvent::KeyUp(KeyCode::H),
            RecordedSinkEvent::KeyUp(KeyCode::LeftShift),
            RecordedSinkEvent::KeyDown(KeyCode::I),
            RecordedSinkEvent::KeyUp(KeyCode::I),
            RecordedSinkEvent::ReleaseAllPressed,
        ]
    );
    Ok(())
}

#[test]
fn abort_during_long_wait_stops_immediately_and_releases_held_keys() -> Result<(), String> {
    let mut program = VisualProgram::new(TriggerBlock::WhileInputHeld {
        trigger_input: InputTrigger::Mouse(MouseButton::Left),
        additional_held_inputs: Vec::new(),
    });
    program.append_root_block(StatementKind::KeyDown {
        key_code: KeyCode::W,
    });
    program.append_root_block(StatementKind::WaitFixed { delay_ms: 5_000 });

    let input_sink = Arc::new(RecordingInputSink::default());
    let physical_input = Arc::new(ControlledPhysicalInput::default());
    physical_input.set_mouse_pressed(MouseButton::Left, true);

    let abort_flag = Arc::new(AtomicBool::new(false));
    let worker_sink: Arc<dyn InputSink> = Arc::clone(&input_sink) as Arc<dyn InputSink>;
    let worker_physical: Arc<dyn PhysicalInputReader> =
        Arc::clone(&physical_input) as Arc<dyn PhysicalInputReader>;
    let worker_abort = Arc::clone(&abort_flag);

    let runner_thread = thread::spawn(move || {
        let mut hooks = BlockExecutionHooks::default();
        execute_visual_program(
            &program,
            worker_sink,
            worker_physical,
            &worker_abort,
            &mut hooks,
        )
    });

    thread::sleep(Duration::from_millis(25));
    let abort_started_at = Instant::now();
    abort_flag.store(true, Ordering::SeqCst);

    let outcome = match runner_thread.join() {
        Ok(result) => result?,
        Err(_) => return Err("interpreter thread panicked".to_string()),
    };
    let abort_elapsed_ms = abort_started_at.elapsed().as_millis();

    assert_eq!(outcome, ProgramStopReason::Aborted);
    assert!(
        abort_elapsed_ms < 250,
        "abort during 5 s wait took {abort_elapsed_ms} ms"
    );
    let recorded_events = input_sink.snapshot_events();
    assert!(recorded_events.contains(&RecordedSinkEvent::KeyDown(KeyCode::W)));
    assert_eq!(
        recorded_events.last(),
        Some(&RecordedSinkEvent::ReleaseAllPressed)
    );
    Ok(())
}
