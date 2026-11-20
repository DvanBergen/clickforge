use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use clickforge::engine::generate_rust_source_from_script;
use clickforge::engine::rust_script_runner::{
    RustExecutionHooks, compile_rust_script, run_compiled_rust_script,
};
use clickforge::models::{
    InputError, InputSink, PhysicalInputReader, ScriptDocument, ScriptMode, TargetScope,
};
use clickforge_sdk::{KeyCode, MouseButton};

struct EphemeralCacheGuard {
    cache_directory: PathBuf,
}

impl EphemeralCacheGuard {
    fn create(test_label: &str) -> Result<Self, String> {
        let timestamp_ms = ScriptDocument::current_unix_timestamp_ms();
        let random_suffix: u32 = rand::random();
        let process_id = std::process::id();
        let cache_directory = std::env::temp_dir().join(format!(
            "clickforge-codegen-it-{test_label}-{process_id}-{timestamp_ms}-{random_suffix:08x}"
        ));
        fs::create_dir_all(&cache_directory)
            .map_err(|io_error| format!("failed creating temp cache dir: {io_error}"))?;
        Ok(Self { cache_directory })
    }
}

impl Drop for EphemeralCacheGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.cache_directory);
    }
}

#[derive(Default)]
struct CountingMockSink {
    left_mouse_down_count: AtomicUsize,
    left_mouse_up_count: AtomicUsize,
    release_all_count: AtomicUsize,
}

impl CountingMockSink {
    fn completed_left_clicks(&self) -> usize {
        self.left_mouse_down_count
            .load(Ordering::SeqCst)
            .min(self.left_mouse_up_count.load(Ordering::SeqCst))
    }

    fn release_all_calls(&self) -> usize {
        self.release_all_count.load(Ordering::SeqCst)
    }
}

impl InputSink for CountingMockSink {
    fn mouse_down(&self, mouse_button: MouseButton) -> Result<(), InputError> {
        if mouse_button == MouseButton::Left {
            self.left_mouse_down_count.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }

    fn mouse_up(&self, mouse_button: MouseButton) -> Result<(), InputError> {
        if mouse_button == MouseButton::Left {
            self.left_mouse_up_count.fetch_add(1, Ordering::SeqCst);
        }
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
struct MockPhysicalState {
    pressed_buttons: Mutex<HashSet<MouseButton>>,
}

impl MockPhysicalState {
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

impl PhysicalInputReader for MockPhysicalState {
    fn is_mouse_pressed(&self, mouse_button: MouseButton) -> bool {
        self.pressed_buttons
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&mouse_button)
    }

    fn is_key_pressed(&self, _key_code: KeyCode) -> bool {
        false
    }
}

#[test]
fn missing_kill_switch_blocks_rust_compilation() -> Result<(), String> {
    let cache_guard = EphemeralCacheGuard::create("missing-kill")?;
    let invalid_script = ScriptDocument::new_blank(
        "Unprotected Rust Script",
        TargetScope::Global,
        ScriptMode::AdvancedRust,
    );

    let compile_result = compile_rust_script(&invalid_script, &cache_guard.cache_directory);
    assert!(!compile_result.is_success);
    assert!(compile_result.binary_path.is_none());
    assert!(
        compile_result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("Kill-Switch"))
    );
    Ok(())
}

#[test]
fn exported_visual_blocks_compile_run_over_unix_ipc_and_sigkill_unresponsive_child()
-> Result<(), String> {
    let cache_guard = EphemeralCacheGuard::create("full-pipeline")?;

    // 1. Export Visual Blocks template to clean native Rust source.
    let mut exported_script = ScriptDocument::new_building_upgrade_template();
    let generated_source = generate_rust_source_from_script(&exported_script);
    assert!(!generated_source.contains(".unwrap()"));
    assert!(!generated_source.contains(".expect("));
    assert!(generated_source.contains("wait_base_plus_gaussian_ms(45"));
    assert!(generated_source.contains("mouse_click(MouseButton::Left, 12)"));

    exported_script.mode = ScriptMode::AdvancedRust;
    exported_script.rust_source = generated_source;

    // 2. Compile exported Rust source with real cargo into the temporary cache dir.
    let compile_snapshot = compile_rust_script(&exported_script, &cache_guard.cache_directory);
    assert!(
        compile_snapshot.is_success,
        "expected exported Rust code to compile cleanly, got diagnostics: {:?}",
        compile_snapshot.diagnostics
    );
    let binary_path_text = compile_snapshot
        .binary_path
        .ok_or_else(|| "missing compiled binary path".to_string())?;
    let binary_path = PathBuf::from(binary_path_text);
    assert!(binary_path.is_file());

    // 3. Run compiled binary over real Unix domain socket with LMB physically held.
    let input_sink = Arc::new(CountingMockSink::default());
    let physical_input = Arc::new(MockPhysicalState::default());
    physical_input.set_mouse_pressed(MouseButton::Left, true);
    let abort_flag = Arc::new(AtomicBool::new(false));

    let runner_sink: Arc<dyn InputSink> = Arc::clone(&input_sink) as Arc<dyn InputSink>;
    let runner_physical: Arc<dyn PhysicalInputReader> =
        Arc::clone(&physical_input) as Arc<dyn PhysicalInputReader>;
    let runner_abort = Arc::clone(&abort_flag);
    let runner_binary = binary_path.clone();

    let runner_thread = thread::spawn(move || {
        let mut hooks = RustExecutionHooks::default();
        run_compiled_rust_script(
            &runner_binary,
            runner_sink,
            runner_physical,
            &runner_abort,
            &mut hooks,
        )
    });

    let wait_deadline = Instant::now() + Duration::from_secs(3);
    while input_sink.completed_left_clicks() < 2 && Instant::now() < wait_deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        input_sink.completed_left_clicks() >= 2,
        "expected compiled binary to emit >= 2 Left clicks over Unix IPC, observed {}",
        input_sink.completed_left_clicks()
    );

    abort_flag.store(true, Ordering::SeqCst);
    match runner_thread.join() {
        Ok(run_result) => run_result?,
        Err(_) => return Err("compiled script runner thread panicked".to_string()),
    }
    assert!(input_sink.release_all_calls() >= 1);

    // 4. Verify an unresponsive infinite-loop child terminates immediately via SIGKILL on abort.
    let mut hung_script = ScriptDocument::new_blank(
        "Unresponsive Loop Test",
        TargetScope::Global,
        ScriptMode::AdvancedRust,
    );
    hung_script.kill_switch_key = Some(KeyCode::F8);
    hung_script.rust_source = [
        "use std::time::Duration;",
        "use clickforge_sdk::{MouseButton, ScriptContext, ScriptResult};",
        "",
        "pub fn run_script(script_context: &mut ScriptContext) -> ScriptResult<()> {",
        "    script_context.mouse_click(MouseButton::Left, 5)?;",
        "    loop {",
        "        std::thread::sleep(Duration::from_secs(60));",
        "    }",
        "}",
    ]
    .join("\n");

    let hung_compile = compile_rust_script(&hung_script, &cache_guard.cache_directory);
    assert!(
        hung_compile.is_success,
        "expected hung test script to compile: {:?}",
        hung_compile.diagnostics
    );
    let hung_binary = PathBuf::from(
        hung_compile
            .binary_path
            .ok_or_else(|| "missing hung test binary path".to_string())?,
    );

    let hung_sink = Arc::new(CountingMockSink::default());
    let hung_physical = Arc::new(MockPhysicalState::default());
    let hung_abort = Arc::new(AtomicBool::new(false));

    let hung_runner_sink: Arc<dyn InputSink> = Arc::clone(&hung_sink) as Arc<dyn InputSink>;
    let hung_runner_physical: Arc<dyn PhysicalInputReader> =
        Arc::clone(&hung_physical) as Arc<dyn PhysicalInputReader>;
    let hung_runner_abort = Arc::clone(&hung_abort);

    let hung_thread = thread::spawn(move || {
        let mut hooks = RustExecutionHooks::default();
        run_compiled_rust_script(
            &hung_binary,
            hung_runner_sink,
            hung_runner_physical,
            &hung_runner_abort,
            &mut hooks,
        )
    });

    let click_deadline = Instant::now() + Duration::from_secs(3);
    while hung_sink.completed_left_clicks() < 1 && Instant::now() < click_deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(hung_sink.completed_left_clicks() >= 1);

    let sigkill_started_at = Instant::now();
    hung_abort.store(true, Ordering::SeqCst);
    match hung_thread.join() {
        Ok(run_result) => run_result?,
        Err(_) => return Err("hung child runner thread panicked".to_string()),
    }
    let sigkill_elapsed_ms = sigkill_started_at.elapsed().as_millis();

    assert!(
        sigkill_elapsed_ms < 500,
        "SIGKILL termination of unresponsive child took {sigkill_elapsed_ms} ms"
    );
    assert!(hung_sink.release_all_calls() >= 1);
    Ok(())
}
