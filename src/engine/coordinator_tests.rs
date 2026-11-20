use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};

use super::*;
use crate::models::{
    GameTargetRule, InputError, ScriptExecutionPhase, ScriptMode, StatementKind, TargetMatchMode,
    TargetScope, TriggerBlock, TriggerInterceptSpec, VisualProgram,
};

#[derive(Default)]
struct RecordingSink {
    click_count: AtomicU64,
    release_all_count: AtomicU64,
}

impl InputSink for RecordingSink {
    fn mouse_down(&self, _mouse_button: MouseButton) -> Result<(), InputError> {
        self.click_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn mouse_up(&self, _mouse_button: MouseButton) -> Result<(), InputError> {
        Ok(())
    }
    fn mouse_move_relative(&self, _delta_x_px: i32, _delta_y_px: i32) -> Result<(), InputError> {
        Ok(())
    }
    fn mouse_scroll(&self, _vertical_steps: i32) -> Result<(), InputError> {
        Ok(())
    }
    fn key_down(&self, _key_code: KeyCode) -> Result<(), InputError> {
        Ok(())
    }
    fn key_up(&self, _key_code: KeyCode) -> Result<(), InputError> {
        Ok(())
    }
    fn release_all_pressed(&self) -> Result<(), InputError> {
        self.release_all_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Default)]
struct TestPhysicalInput {
    is_left_mouse_held: AtomicBool,
}

impl PhysicalInputReader for TestPhysicalInput {
    fn is_mouse_pressed(&self, mouse_button: MouseButton) -> bool {
        mouse_button == MouseButton::Left && self.is_left_mouse_held.load(Ordering::SeqCst)
    }
    fn is_key_pressed(&self, _key_code: KeyCode) -> bool {
        false
    }
}

#[derive(Default)]
struct TrackingIntercept {
    active_spec: Mutex<Option<TriggerInterceptSpec>>,
}

impl TriggerInterceptControl for TrackingIntercept {
    fn set_intercept(&self, spec: Option<TriggerInterceptSpec>) -> Result<(), InputError> {
        if let Ok(mut guard) = self.active_spec.lock() {
            *guard = spec;
        }
        Ok(())
    }
    fn is_intercept_active(&self) -> bool {
        self.active_spec
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }
}

fn build_focus_gated_game_script() -> ScriptDocument {
    let mut game_script = ScriptDocument::new_building_upgrade_template();
    game_script.is_enabled = true;
    game_script.target_scope = TargetScope::SelectedGame(GameTargetRule {
        display_name: "Here Comes the Horde".to_string(),
        match_mode: TargetMatchMode::PersistentAppAndTitle,
        app_id: Some("steam_app_2560650".to_string()),
        window_title_contains: Some("Here Comes the Horde".to_string()),
        executable_name: None,
        steam_app_id: Some("2560650".to_string()),
        strict_pid: None,
    });
    game_script.visual_program = VisualProgram::new(TriggerBlock::WhileInputHeld {
        trigger_input: InputTrigger::Mouse(MouseButton::Left),
    });
    game_script
        .visual_program
        .append_root_block(StatementKind::WaitFixed { delay_ms: 5 });
    game_script
        .visual_program
        .append_root_block(StatementKind::MouseClick {
            mouse_button: MouseButton::Left,
            hold_duration_ms: 1,
        });
    game_script
}

#[test]
fn coordinator_enforces_focus_gating_kill_switch_and_key_capture() -> Result<(), String> {
    let game_script = build_focus_gated_game_script();
    let (input_sender, input_receiver) = mpsc::sync_channel(32);
    let (focus_sender, focus_receiver) = mpsc::sync_channel(16);
    let shared_window = Arc::new(Mutex::new(None::<ActiveWindowSnapshot>));
    let provider_window = Arc::clone(&shared_window);
    let sink = Arc::new(RecordingSink::default());
    let physical = Arc::new(TestPhysicalInput::default());
    let intercept = Arc::new(TrackingIntercept::default());
    let tray_states = Arc::new(Mutex::new(Vec::<TrayRuntimeState>::new()));
    let recorded_trays = Arc::clone(&tray_states);

    let handle = start_coordinator(CoordinatorStartConfig {
        initial_scripts: vec![game_script.clone()],
        input_sink: Arc::clone(&sink) as Arc<dyn InputSink>,
        physical_input: Arc::clone(&physical) as Arc<dyn PhysicalInputReader>,
        trigger_intercept: Arc::clone(&intercept) as Arc<dyn TriggerInterceptControl>,
        input_status: InputSubsystemStatus::default(),
        window_snapshot_provider: Arc::new(move || {
            let current = provider_window.lock().ok().and_then(|guard| guard.clone());
            (current, Vec::new(), Vec::new())
        }),
        input_event_receiver: Some(input_receiver),
        focus_event_receiver: Some(focus_receiver),
        on_tray_state_changed: Some(Arc::new(move |tray_state| {
            if let Ok(mut guard) = recorded_trays.lock() {
                guard.push(tray_state);
            }
        })),
        on_ui_repaint_needed: None,
        shutdown_flag: Arc::new(AtomicBool::new(false)),
        script_build_cache_dir: None,
    });

    thread::sleep(Duration::from_millis(35));
    assert!(!intercept.is_intercept_active());
    let initial_status = handle
        .snapshot()
        .status_for_script(&game_script.id)
        .map(|status| status.phase);
    assert_eq!(initial_status, Some(ScriptExecutionPhase::WaitingForFocus));

    let focused_game = ActiveWindowSnapshot {
        window_id: Some(1),
        title: "Here Comes the Horde".to_string(),
        app_id: "steam_app_2560650".to_string(),
        pid: Some(1200),
        is_focused: true,
        compositor_name: "niri".to_string(),
    };
    if let Ok(mut guard) = shared_window.lock() {
        *guard = Some(focused_game.clone());
    }
    let _ = focus_sender.send(focused_game);
    physical.is_left_mouse_held.store(true, Ordering::SeqCst);
    thread::sleep(Duration::from_millis(70));

    assert!(intercept.is_intercept_active());
    assert!(sink.click_count.load(Ordering::SeqCst) >= 1);

    let _ = input_sender.send((InputTrigger::Key(KeyCode::F8), true));
    thread::sleep(Duration::from_millis(50));

    assert!(handle.snapshot().is_emergency_stopped);
    assert!(!intercept.is_intercept_active());
    assert!(sink.release_all_count.load(Ordering::SeqCst) >= 1);
    let stopped_phase = handle
        .snapshot()
        .status_for_script(&game_script.id)
        .map(|status| status.phase);
    assert_eq!(
        stopped_phase,
        Some(ScriptExecutionPhase::StoppedByKillSwitch)
    );

    handle.begin_key_capture();
    let _ = input_sender.send((InputTrigger::Key(KeyCode::F9), true));
    thread::sleep(Duration::from_millis(40));
    assert_eq!(handle.take_captured_key_code(), Some(KeyCode::F9));

    handle.shutdown();
    let saw_emergency_tray = tray_states
        .lock()
        .map(|guard| {
            guard
                .iter()
                .any(|state| state.overall_phase == ScriptExecutionPhase::StoppedByKillSwitch)
        })
        .unwrap_or(false);
    assert!(saw_emergency_tray);
    Ok(())
}

#[test]
fn coordinator_refuses_to_arm_script_without_kill_switch() {
    let mut invalid_script = ScriptDocument::new_blank(
        "Unprotected Script",
        TargetScope::Global,
        ScriptMode::VisualBlocks,
    );
    invalid_script.is_enabled = true;

    let sink = Arc::new(RecordingSink::default());
    let physical = Arc::new(TestPhysicalInput::default());
    let intercept = Arc::new(TrackingIntercept::default());

    let handle = start_coordinator(CoordinatorStartConfig {
        initial_scripts: vec![invalid_script.clone()],
        input_sink: sink,
        physical_input: physical,
        trigger_intercept: Arc::clone(&intercept) as Arc<dyn TriggerInterceptControl>,
        input_status: InputSubsystemStatus::default(),
        window_snapshot_provider: Arc::new(|| (None, Vec::new(), Vec::new())),
        input_event_receiver: None,
        focus_event_receiver: None,
        on_tray_state_changed: None,
        on_ui_repaint_needed: None,
        shutdown_flag: Arc::new(AtomicBool::new(false)),
        script_build_cache_dir: None,
    });

    thread::sleep(Duration::from_millis(40));
    let snapshot = handle.snapshot();
    let status = snapshot.status_for_script(&invalid_script.id);
    assert_eq!(
        status.map(|entry| entry.phase),
        Some(ScriptExecutionPhase::Error)
    );
    assert!(!intercept.is_intercept_active());
    handle.shutdown();
}

#[test]
fn coordinator_toggles_script_armed_state_via_arm_toggle_key() {
    let mut script = ScriptDocument::new_building_upgrade_template();
    script.is_enabled = true;
    script.target_scope = TargetScope::Global;
    script.arm_toggle_key = Some(KeyCode::F6);

    let (input_sender, input_receiver) = mpsc::sync_channel(16);
    let sink = Arc::new(RecordingSink::default());
    let physical = Arc::new(TestPhysicalInput::default());
    let intercept = Arc::new(TrackingIntercept::default());

    let handle = start_coordinator(CoordinatorStartConfig {
        initial_scripts: vec![script.clone()],
        input_sink: sink,
        physical_input: physical,
        trigger_intercept: Arc::clone(&intercept) as Arc<dyn TriggerInterceptControl>,
        input_status: InputSubsystemStatus::default(),
        window_snapshot_provider: Arc::new(|| (None, Vec::new(), Vec::new())),
        input_event_receiver: Some(input_receiver),
        focus_event_receiver: None,
        on_tray_state_changed: None,
        on_ui_repaint_needed: None,
        shutdown_flag: Arc::new(AtomicBool::new(false)),
        script_build_cache_dir: None,
    });

    thread::sleep(Duration::from_millis(40));
    assert!(intercept.is_intercept_active());

    let _ = input_sender.send((InputTrigger::Key(KeyCode::F6), true));
    thread::sleep(Duration::from_millis(40));
    assert!(!intercept.is_intercept_active());

    let _ = input_sender.send((InputTrigger::Key(KeyCode::F6), true));
    thread::sleep(Duration::from_millis(40));
    assert!(intercept.is_intercept_active());

    handle.shutdown();
}

#[test]
fn manual_compile_on_disabled_script_preserves_compiling_phase() -> Result<(), String> {
    let mut script = ScriptDocument::new_building_upgrade_template();
    script.is_enabled = false;
    script.mode = ScriptMode::AdvancedRust;

    let cache_directory = std::env::temp_dir().join(format!(
        "clickforge-coord-compile-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&cache_directory);

    let handle = start_coordinator(CoordinatorStartConfig {
        initial_scripts: vec![script.clone()],
        input_sink: Arc::new(RecordingSink::default()),
        physical_input: Arc::new(TestPhysicalInput::default()),
        trigger_intercept: Arc::new(TrackingIntercept::default()),
        input_status: InputSubsystemStatus::default(),
        window_snapshot_provider: Arc::new(|| (None, Vec::new(), Vec::new())),
        input_event_receiver: None,
        focus_event_receiver: None,
        on_tray_state_changed: None,
        on_ui_repaint_needed: None,
        shutdown_flag: Arc::new(AtomicBool::new(false)),
        script_build_cache_dir: Some(cache_directory.clone()),
    });

    handle.send_command(CoordinatorCommand::CompileRustScript(script.clone()))?;
    let mut observed_compiling_phase = false;
    let mut observed_finished_result = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);

    while std::time::Instant::now() < deadline {
        let snapshot = handle.snapshot();
        if snapshot
            .status_for_script(&script.id)
            .is_some_and(|status| status.phase == ScriptExecutionPhase::Compiling)
        {
            observed_compiling_phase = true;
        }
        if let Some(compile_result) = handle.last_compile_result(&script.id) {
            assert!(
                compile_result.is_success,
                "compile failed: {:?}",
                compile_result.diagnostics
            );
            observed_finished_result = true;
            break;
        }
        thread::sleep(Duration::from_millis(15));
    }

    handle.shutdown();
    let _ = std::fs::remove_dir_all(&cache_directory);

    assert!(
        observed_compiling_phase,
        "expected disabled script to retain Compiling phase across coordinator ticks"
    );
    assert!(observed_finished_result);
    Ok(())
}
