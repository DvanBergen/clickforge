use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};

use clickforge_sdk::{InputTrigger, KeyCode};

use crate::engine::coordinator_loop::CoordinatorLoop;
use crate::models::{
    ActiveWindowSnapshot, CompileResultSnapshot, CoordinatorCommand, CoordinatorSnapshot,
    InputSink, InputSubsystemStatus, PhysicalInputReader, RunningProcessEntry, ScriptDocument,
    TrayRuntimeState, TriggerInterceptControl,
};

const MAX_COMMAND_QUEUE_CAPACITY: usize = 128;

/// Callback returning `(focused_window, open_windows, running_processes)` on demand.
pub type WindowSnapshotProvider = Arc<
    dyn Fn() -> (
            Option<ActiveWindowSnapshot>,
            Vec<ActiveWindowSnapshot>,
            Vec<RunningProcessEntry>,
        ) + Send
        + Sync,
>;

/// Dependency injection bundle used to launch the background script coordinator thread.
pub struct CoordinatorStartConfig {
    pub initial_scripts: Vec<ScriptDocument>,
    pub input_sink: Arc<dyn InputSink>,
    pub physical_input: Arc<dyn PhysicalInputReader>,
    pub trigger_intercept: Arc<dyn TriggerInterceptControl>,
    pub input_status: InputSubsystemStatus,
    pub window_snapshot_provider: WindowSnapshotProvider,
    pub input_event_receiver: Option<Receiver<(InputTrigger, bool)>>,
    pub focus_event_receiver: Option<Receiver<ActiveWindowSnapshot>>,
    pub on_tray_state_changed: Option<Arc<dyn Fn(TrayRuntimeState) + Send + Sync>>,
    pub on_ui_repaint_needed: Option<Arc<dyn Fn() + Send + Sync>>,
    pub shutdown_flag: Arc<AtomicBool>,
    pub script_build_cache_dir: Option<PathBuf>,
}

pub(crate) struct KeyCaptureState {
    pub(crate) is_capturing: AtomicBool,
    pub(crate) baseline_sequence: AtomicU64,
    pub(crate) captured_key: Mutex<Option<KeyCode>>,
}

/// Thread-safe handle for sending commands and reading live snapshots from the coordinator.
pub struct CoordinatorHandle {
    command_sender: SyncSender<CoordinatorCommand>,
    shared_snapshot: Arc<RwLock<CoordinatorSnapshot>>,
    shared_compile_results: Arc<RwLock<HashMap<String, CompileResultSnapshot>>>,
    key_capture: Arc<KeyCaptureState>,
    physical_input: Arc<dyn PhysicalInputReader>,
    shutdown_flag: Arc<AtomicBool>,
    worker_thread: Mutex<Option<JoinHandle<()>>>,
}

impl CoordinatorHandle {
    /// Enqueues a bounded command for the background coordinator loop.
    pub fn send_command(&self, command: CoordinatorCommand) -> Result<(), String> {
        match self.command_sender.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                Err("Coordinator command queue is full; try again shortly.".to_string())
            }
            Err(TrySendError::Disconnected(_)) => {
                Err("Coordinator thread is no longer running.".to_string())
            }
        }
    }

    /// Returns the latest immutable runtime snapshot for the UI workbench.
    pub fn snapshot(&self) -> CoordinatorSnapshot {
        match self.shared_snapshot.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned_guard) => poisoned_guard.into_inner().clone(),
        }
    }

    /// Returns the most recent compilation snapshot recorded for `script_id`, if any.
    pub fn last_compile_result(&self, script_id: &str) -> Option<CompileResultSnapshot> {
        match self.shared_compile_results.read() {
            Ok(guard) => guard.get(script_id).cloned(),
            Err(poisoned_guard) => poisoned_guard.into_inner().get(script_id).cloned(),
        }
    }

    /// Begins capturing the next physical key press from `evdev` for Kill-Switch binding.
    pub fn begin_key_capture(&self) {
        let current_sequence = self
            .physical_input
            .last_pressed_key_snapshot()
            .map(|(_, sequence)| sequence)
            .unwrap_or(0);
        self.key_capture
            .baseline_sequence
            .store(current_sequence, Ordering::SeqCst);
        if let Ok(mut guard) = self.key_capture.captured_key.lock() {
            *guard = None;
        }
        self.key_capture.is_capturing.store(true, Ordering::SeqCst);
    }

    /// Cancels any active physical key capture session.
    pub fn cancel_key_capture(&self) {
        self.key_capture.is_capturing.store(false, Ordering::SeqCst);
        if let Ok(mut guard) = self.key_capture.captured_key.lock() {
            *guard = None;
        }
    }

    /// Consumes and returns a physical key captured since `begin_key_capture()`.
    pub fn take_captured_key_code(&self) -> Option<KeyCode> {
        match self.key_capture.captured_key.lock() {
            Ok(mut guard) => guard.take(),
            Err(poisoned_guard) => poisoned_guard.into_inner().take(),
        }
    }

    /// Signals a clean shutdown, releases grabs/inputs, and joins the coordinator thread.
    pub fn shutdown(&self) {
        self.shutdown_flag.store(true, Ordering::SeqCst);
        let _ = self.command_sender.try_send(CoordinatorCommand::Shutdown);
        let thread_handle = match self.worker_thread.lock() {
            Ok(mut guard) => guard.take(),
            Err(poisoned_guard) => poisoned_guard.into_inner().take(),
        };
        if let Some(handle) = thread_handle {
            let _ = handle.join();
        }
    }
}

impl Drop for CoordinatorHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Spawns the background coordinator thread managing focus switching, execution, and kill-switches.
pub fn start_coordinator(config: CoordinatorStartConfig) -> CoordinatorHandle {
    let (command_sender, command_receiver) =
        mpsc::sync_channel::<CoordinatorCommand>(MAX_COMMAND_QUEUE_CAPACITY);
    let shared_snapshot = Arc::new(RwLock::new(CoordinatorSnapshot {
        input_status: config.input_status.clone(),
        ..CoordinatorSnapshot::default()
    }));
    let shared_compile_results = Arc::new(RwLock::new(HashMap::new()));
    let key_capture = Arc::new(KeyCaptureState {
        is_capturing: AtomicBool::new(false),
        baseline_sequence: AtomicU64::new(0),
        captured_key: Mutex::new(None),
    });
    let physical_input = Arc::clone(&config.physical_input);
    let shutdown_flag = Arc::clone(&config.shutdown_flag);

    let loop_snapshot = Arc::clone(&shared_snapshot);
    let loop_compile_results = Arc::clone(&shared_compile_results);
    let loop_key_capture = Arc::clone(&key_capture);

    let join_handle = thread::spawn(move || {
        let mut coordinator = CoordinatorLoop::new(
            config,
            command_receiver,
            loop_snapshot,
            loop_compile_results,
            loop_key_capture,
        );
        coordinator.run();
    });

    CoordinatorHandle {
        command_sender,
        shared_snapshot,
        shared_compile_results,
        key_capture,
        physical_input,
        shutdown_flag,
        worker_thread: Mutex::new(Some(join_handle)),
    }
}

#[cfg(test)]
#[path = "coordinator_tests.rs"]
mod tests;
