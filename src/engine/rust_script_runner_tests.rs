use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clickforge_sdk::{InputTrigger, KeyCode, MouseButton};
use rand::RngExt;

use super::*;
use crate::engine::rust_codegen::generate_rust_source_from_script;
use crate::models::{
    ComparisonOperator, ConditionExpr, DiagnosticSeverity, InputError, ScriptDocument, ScriptMode,
    StatementKind, TargetScope, TriggerBlock, VisualProgram,
};

#[derive(Default)]
struct CountingInputSink {
    mouse_downs: Mutex<Vec<MouseButton>>,
    release_all_count: Mutex<u32>,
}

impl InputSink for CountingInputSink {
    fn mouse_down(&self, mouse_button: MouseButton) -> Result<(), InputError> {
        if let Ok(mut guard) = self.mouse_downs.lock() {
            guard.push(mouse_button);
        }
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
        if let Ok(mut guard) = self.release_all_count.lock() {
            *guard = guard.saturating_add(1);
        }
        Ok(())
    }
}

#[derive(Default)]
struct StaticPhysicalInput {
    is_left_pressed: AtomicBool,
}

impl PhysicalInputReader for StaticPhysicalInput {
    fn is_mouse_pressed(&self, mouse_button: MouseButton) -> bool {
        mouse_button == MouseButton::Left && self.is_left_pressed.load(Ordering::SeqCst)
    }
    fn is_key_pressed(&self, _key_code: KeyCode) -> bool {
        false
    }
}

fn temp_cache_dir(test_label: &str) -> PathBuf {
    let random_suffix: u32 = rand::rng().random();
    std::env::temp_dir().join(format!(
        "clickforge-test-cache-{test_label}-{}-{random_suffix:08x}",
        std::process::id()
    ))
}

#[test]
fn refuses_compilation_when_kill_switch_is_missing() {
    let blank_script = ScriptDocument::new_blank(
        "Missing Kill Switch",
        TargetScope::Global,
        ScriptMode::AdvancedRust,
    );
    let cache_directory = temp_cache_dir("validation");
    let snapshot = compile_rust_script(&blank_script, &cache_directory);
    assert!(!snapshot.is_success);
    assert!(snapshot.binary_path.is_none());
    assert!(!snapshot.diagnostics.is_empty());
    assert!(!cache_directory.exists());
}

#[test]
fn maps_compiler_error_line_number_back_to_user_source() {
    let mut broken_script = ScriptDocument::new_building_upgrade_template();
    broken_script.mode = ScriptMode::AdvancedRust;
    broken_script.rust_source = [
        "use clickforge_sdk::{ScriptContext, ScriptResult};",
        "",
        "pub fn run_script(_script_context: &mut ScriptContext) -> ScriptResult<()> {",
        "    let bad_value: u32 = \"not_a_number\";",
        "    Ok(())",
        "}",
    ]
    .join("\n");

    let cache_directory = temp_cache_dir("line-map");
    let snapshot = compile_rust_script(&broken_script, &cache_directory);
    let _ = fs::remove_dir_all(&cache_directory);

    assert!(!snapshot.is_success);
    let first_error = snapshot
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error);
    assert!(first_error.is_some());
    if let Some(diagnostic) = first_error {
        assert_eq!(diagnostic.line_number, Some(4));
        assert_eq!(diagnostic.file_name.as_deref(), Some("script.rs"));
    }
}

#[test]
fn compiles_generated_rust_script_and_runs_over_unix_socket_with_clean_abort() -> Result<(), String>
{
    let mut script = ScriptDocument::new_building_upgrade_template();
    script.rust_source = generate_rust_source_from_script(&script);
    script.mode = ScriptMode::AdvancedRust;

    let cache_directory = temp_cache_dir("end-to-end");
    let compile_result = compile_rust_script(&script, &cache_directory);
    assert!(
        compile_result.is_success,
        "compilation failed: {:?}",
        compile_result.diagnostics
    );
    let Some(binary_path_text) = compile_result.binary_path else {
        let _ = fs::remove_dir_all(&cache_directory);
        return Err("compile succeeded without binary_path".to_string());
    };

    let mock_sink = Arc::new(CountingInputSink::default());
    let mock_physical = Arc::new(StaticPhysicalInput::default());
    mock_physical.is_left_pressed.store(true, Ordering::SeqCst);

    let abort_flag = Arc::new(AtomicBool::new(false));
    let worker_abort = Arc::clone(&abort_flag);
    let worker_sink = Arc::clone(&mock_sink);
    let worker_physical = Arc::clone(&mock_physical);
    let binary_path = PathBuf::from(binary_path_text);

    let runner_handle = std::thread::spawn(move || {
        let mut hooks = RustExecutionHooks::default();
        run_compiled_rust_script(
            &binary_path,
            worker_sink,
            worker_physical,
            &worker_abort,
            &mut hooks,
        )
    });

    std::thread::sleep(Duration::from_millis(220));
    abort_flag.store(true, Ordering::SeqCst);

    let run_outcome = match runner_handle.join() {
        Ok(result) => result,
        Err(_) => Err("runner test thread panicked".to_string()),
    };
    let _ = fs::remove_dir_all(&cache_directory);
    run_outcome?;

    let recorded_clicks = mock_sink
        .mouse_downs
        .lock()
        .map(|guard| guard.len())
        .unwrap_or(0);
    let release_calls = mock_sink
        .release_all_count
        .lock()
        .map(|guard| *guard)
        .unwrap_or(0);
    assert!(
        recorded_clicks >= 1,
        "expected at least one mouse click over IPC, got {recorded_clicks}"
    );
    assert!(release_calls >= 1);
    Ok(())
}

#[test]
fn kills_unresponsive_infinite_loop_child_promptly_on_abort() -> Result<(), String> {
    let mut script = ScriptDocument::new_building_upgrade_template();
    script.mode = ScriptMode::AdvancedRust;
    script.rust_source = [
        "use clickforge_sdk::{ScriptContext, ScriptResult};",
        "",
        "pub fn run_script(script_context: &mut ScriptContext) -> ScriptResult<()> {",
        "    script_context.log_info(\"entering unresponsive loop\")?;",
        "    loop {",
        "        std::thread::sleep(std::time::Duration::from_millis(25));",
        "    }",
        "}",
    ]
    .join("\n");

    let cache_directory = temp_cache_dir("sigkill-loop");
    let compile_result = compile_rust_script(&script, &cache_directory);
    assert!(
        compile_result.is_success,
        "compile failed: {:?}",
        compile_result.diagnostics
    );
    let Some(binary_path_text) = compile_result.binary_path else {
        let _ = fs::remove_dir_all(&cache_directory);
        return Err("missing compiled binary_path".to_string());
    };

    let mock_sink = Arc::new(CountingInputSink::default());
    let mock_physical = Arc::new(StaticPhysicalInput::default());
    let abort_flag = Arc::new(AtomicBool::new(false));
    let worker_abort = Arc::clone(&abort_flag);
    let worker_sink = Arc::clone(&mock_sink);
    let binary_path = PathBuf::from(binary_path_text);

    let runner_handle = std::thread::spawn(move || {
        let mut hooks = RustExecutionHooks::default();
        run_compiled_rust_script(
            &binary_path,
            worker_sink,
            mock_physical,
            &worker_abort,
            &mut hooks,
        )
    });

    std::thread::sleep(Duration::from_millis(60));
    let abort_started_at = Instant::now();
    abort_flag.store(true, Ordering::SeqCst);

    let run_outcome = match runner_handle.join() {
        Ok(result) => result,
        Err(_) => Err("runner test thread panicked".to_string()),
    };
    let abort_elapsed_ms = abort_started_at.elapsed().as_millis();
    let _ = fs::remove_dir_all(&cache_directory);

    run_outcome?;
    assert!(
        abort_elapsed_ms < 400,
        "unresponsive child process took too long to terminate: {abort_elapsed_ms} ms"
    );
    Ok(())
}

#[test]
fn compiles_complex_codegen_program_with_variables_and_random_chance() {
    let mut script = ScriptDocument::new_building_upgrade_template();
    script.visual_program = VisualProgram::new(TriggerBlock::OnInputToggle {
        trigger_input: InputTrigger::Key(KeyCode::F7),
    });
    script
        .visual_program
        .append_root_block(StatementKind::SetVariable {
            variable_name: "wave".to_string(),
            value: 0.0,
        });
    let increment_block = script
        .visual_program
        .create_block(StatementKind::ChangeVariable {
            variable_name: "wave".to_string(),
            delta_value: 1.0,
        });
    let type_block = script.visual_program.create_block(StatementKind::TypeText {
        text: "ok".to_string(),
        keystroke_delay_ms: 10,
        jitter_ms: 2,
    });
    script
        .visual_program
        .append_root_block(StatementKind::IfThen {
            condition: ConditionExpr::And {
                left_condition: Box::new(ConditionExpr::RandomChance {
                    chance_percent: 50.0,
                }),
                right_condition: Box::new(ConditionExpr::CompareVariable {
                    variable_name: "wave".to_string(),
                    operator: ComparisonOperator::LessThan,
                    target_value: 3.0,
                }),
            },
            then_blocks: vec![increment_block, type_block],
        });
    script.rust_source = generate_rust_source_from_script(&script);
    script.mode = ScriptMode::AdvancedRust;

    let cache_directory = temp_cache_dir("complex-codegen");
    let compile_result = compile_rust_script(&script, &cache_directory);
    let _ = fs::remove_dir_all(&cache_directory);
    assert!(
        compile_result.is_success,
        "complex generated Rust failed to compile: {:?}",
        compile_result.diagnostics
    );
}
