use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::thread::{self, JoinHandle};

use crate::engine::block_interpreter::{
    BlockExecutionHooks, ProgramStopReason, execute_visual_program,
};
use crate::engine::rust_script_runner::{
    RustExecutionHooks, compile_rust_script, run_compiled_rust_script,
};
use crate::models::{
    ActiveWindowSnapshot, BlockId, CompileResultSnapshot, InputSink, PhysicalInputReader,
    ScriptDocument, ScriptExecutionPhase, ScriptRuntimeStatus, TrayRuntimeState,
};

pub(crate) enum WorkerEvent {
    ActiveBlockChanged {
        script_id: String,
        block_id: Option<BlockId>,
    },
    DelaySampled {
        script_id: String,
        delay_ms: u64,
    },
    ActionExecuted {
        script_id: String,
    },
    LogMessage {
        script_id: String,
        message: String,
    },
    WorkerExited {
        script_id: String,
        revision_hash: u64,
        stopped_by_block: bool,
        error_message: Option<String>,
    },
    CompileFinished {
        script_id: String,
        source_hash: u64,
        result: CompileResultSnapshot,
    },
}

struct RunningWorkerHandle {
    revision_hash: u64,
    abort_flag: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl RunningWorkerHandle {
    fn stop_and_join(&mut self) {
        self.abort_flag.store(true, Ordering::SeqCst);
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

struct CachedBinaryEntry {
    source_hash: u64,
    binary_path: PathBuf,
}

/// Manages active Visual Block / Rust worker threads and background script compilation state.
pub(crate) struct ScriptWorkerSupervisor {
    running_workers: HashMap<String, RunningWorkerHandle>,
    compiled_binaries_by_id: HashMap<String, CachedBinaryEntry>,
    compiling_hash_by_id: HashMap<String, u64>,
    failed_compile_hash_by_id: HashMap<String, u64>,
    script_build_cache_dir: PathBuf,
    worker_event_sender: SyncSender<WorkerEvent>,
}

impl ScriptWorkerSupervisor {
    pub(crate) fn new(
        script_build_cache_dir: PathBuf,
        worker_event_sender: SyncSender<WorkerEvent>,
    ) -> Self {
        Self {
            running_workers: HashMap::new(),
            compiled_binaries_by_id: HashMap::new(),
            compiling_hash_by_id: HashMap::new(),
            failed_compile_hash_by_id: HashMap::new(),
            script_build_cache_dir,
            worker_event_sender,
        }
    }

    pub(crate) fn is_running_revision(&self, script_id: &str, revision_hash: u64) -> bool {
        self.running_workers
            .get(script_id)
            .is_some_and(|worker| worker.revision_hash == revision_hash)
    }

    pub(crate) fn is_compiling(&self, script_id: &str) -> bool {
        self.compiling_hash_by_id.contains_key(script_id)
    }

    pub(crate) fn stop_worker_for_script(&mut self, script_id: &str) {
        if let Some(mut worker) = self.running_workers.remove(script_id) {
            worker.stop_and_join();
        }
    }

    pub(crate) fn signal_abort_all(&self) {
        for worker in self.running_workers.values() {
            worker.abort_flag.store(true, Ordering::SeqCst);
        }
    }

    pub(crate) fn join_all_running(&mut self) {
        for mut worker in self.running_workers.drain().map(|(_, handle)| handle) {
            worker.stop_and_join();
        }
    }

    pub(crate) fn clear_script_cache(&mut self, script_id: &str) {
        self.stop_worker_for_script(script_id);
        self.compiled_binaries_by_id.remove(script_id);
        self.compiling_hash_by_id.remove(script_id);
        self.failed_compile_hash_by_id.remove(script_id);
    }

    pub(crate) fn clear_failed_compile(&mut self, script_id: &str) {
        self.failed_compile_hash_by_id.remove(script_id);
    }

    pub(crate) fn record_compile_outcome(
        &mut self,
        script_id: &str,
        source_hash: u64,
        result: &CompileResultSnapshot,
    ) {
        self.compiling_hash_by_id.remove(script_id);
        if result.is_success {
            self.failed_compile_hash_by_id.remove(script_id);
            if let Some(binary_path_text) = result.binary_path.as_ref() {
                self.compiled_binaries_by_id.insert(
                    script_id.to_string(),
                    CachedBinaryEntry {
                        source_hash,
                        binary_path: PathBuf::from(binary_path_text),
                    },
                );
            }
        } else {
            self.failed_compile_hash_by_id
                .insert(script_id.to_string(), source_hash);
        }
    }

    pub(crate) fn spawn_visual_worker(
        &mut self,
        script: &ScriptDocument,
        revision_hash: u64,
        input_sink: Arc<dyn InputSink>,
        physical_input: Arc<dyn PhysicalInputReader>,
    ) {
        self.stop_worker_for_script(&script.id);
        let abort_flag = Arc::new(AtomicBool::new(false));
        let worker_abort = Arc::clone(&abort_flag);
        let event_sender = self.worker_event_sender.clone();
        let script_id = script.id.clone();
        let visual_program = script.visual_program.clone();

        let join_handle = thread::spawn(move || {
            let mut hooks = build_visual_hooks(&event_sender, &script_id);
            let outcome = execute_visual_program(
                &visual_program,
                input_sink,
                physical_input,
                &worker_abort,
                &mut hooks,
            );
            let (stopped_by_block, error_message) = match outcome {
                Ok(ProgramStopReason::Aborted) => (false, None),
                Ok(ProgramStopReason::StoppedByBlock) => (true, None),
                Err(runtime_error) => (false, Some(runtime_error)),
            };
            let _ = event_sender.try_send(WorkerEvent::WorkerExited {
                script_id,
                revision_hash,
                stopped_by_block,
                error_message,
            });
        });

        self.running_workers.insert(
            script.id.clone(),
            RunningWorkerHandle {
                revision_hash,
                abort_flag,
                join_handle: Some(join_handle),
            },
        );
    }

    /// Ensures a compiled Rust script is running; returns `true` if running, `false` if compiling/failed.
    pub(crate) fn ensure_rust_worker(
        &mut self,
        script: &ScriptDocument,
        revision_hash: u64,
        input_sink: Arc<dyn InputSink>,
        physical_input: Arc<dyn PhysicalInputReader>,
    ) -> RustWorkerLaunchOutcome {
        if self.is_running_revision(&script.id, revision_hash) {
            return RustWorkerLaunchOutcome::AlreadyRunning;
        }
        let source_hash = compute_text_hash(&script.rust_source);
        if self.failed_compile_hash_by_id.get(&script.id) == Some(&source_hash) {
            self.stop_worker_for_script(&script.id);
            return RustWorkerLaunchOutcome::CompileFailed;
        }

        let cached_binary = self
            .compiled_binaries_by_id
            .get(&script.id)
            .filter(|entry| entry.source_hash == source_hash && entry.binary_path.is_file())
            .map(|entry| entry.binary_path.clone());

        let Some(binary_path) = cached_binary else {
            self.stop_worker_for_script(&script.id);
            if self.compiling_hash_by_id.get(&script.id) != Some(&source_hash) {
                self.spawn_background_compile(script.clone());
                return RustWorkerLaunchOutcome::CompileStarted;
            }
            return RustWorkerLaunchOutcome::CompilingInProgress;
        };

        self.stop_worker_for_script(&script.id);
        let abort_flag = Arc::new(AtomicBool::new(false));
        let worker_abort = Arc::clone(&abort_flag);
        let event_sender = self.worker_event_sender.clone();
        let script_id = script.id.clone();

        let join_handle = thread::spawn(move || {
            let mut hooks = build_rust_hooks(&event_sender, &script_id);
            let error_message = run_compiled_rust_script(
                &binary_path,
                input_sink,
                physical_input,
                &worker_abort,
                &mut hooks,
            )
            .err();
            let did_exit_cleanly = error_message.is_none() && !worker_abort.load(Ordering::SeqCst);
            let _ = event_sender.try_send(WorkerEvent::WorkerExited {
                script_id,
                revision_hash,
                stopped_by_block: did_exit_cleanly,
                error_message,
            });
        });

        self.running_workers.insert(
            script.id.clone(),
            RunningWorkerHandle {
                revision_hash,
                abort_flag,
                join_handle: Some(join_handle),
            },
        );
        RustWorkerLaunchOutcome::Spawned
    }

    pub(crate) fn spawn_background_compile(&mut self, script: ScriptDocument) {
        let source_hash = compute_text_hash(&script.rust_source);
        let script_id = script.id.clone();
        self.compiling_hash_by_id
            .insert(script_id.clone(), source_hash);

        let cache_directory = self.script_build_cache_dir.clone();
        let event_sender = self.worker_event_sender.clone();
        thread::spawn(move || {
            let compile_result = compile_rust_script(&script, &cache_directory);
            let _ = event_sender.try_send(WorkerEvent::CompileFinished {
                script_id,
                source_hash,
                result: compile_result,
            });
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RustWorkerLaunchOutcome {
    AlreadyRunning,
    Spawned,
    CompileStarted,
    CompilingInProgress,
    CompileFailed,
}

fn build_visual_hooks(
    event_sender: &SyncSender<WorkerEvent>,
    script_id: &str,
) -> BlockExecutionHooks {
    let block_sender = event_sender.clone();
    let block_script_id = script_id.to_string();
    let delay_sender = event_sender.clone();
    let delay_script_id = script_id.to_string();
    let action_sender = event_sender.clone();
    let action_script_id = script_id.to_string();

    BlockExecutionHooks {
        on_active_block_changed: Some(Box::new(move |block_id| {
            let _ = block_sender.try_send(WorkerEvent::ActiveBlockChanged {
                script_id: block_script_id.clone(),
                block_id,
            });
        })),
        on_delay_sampled_ms: Some(Box::new(move |delay_ms| {
            let _ = delay_sender.try_send(WorkerEvent::DelaySampled {
                script_id: delay_script_id.clone(),
                delay_ms,
            });
        })),
        on_action_executed: Some(Box::new(move || {
            let _ = action_sender.try_send(WorkerEvent::ActionExecuted {
                script_id: action_script_id.clone(),
            });
        })),
    }
}

fn build_rust_hooks(event_sender: &SyncSender<WorkerEvent>, script_id: &str) -> RustExecutionHooks {
    let delay_sender = event_sender.clone();
    let delay_script_id = script_id.to_string();
    let action_sender = event_sender.clone();
    let action_script_id = script_id.to_string();
    let log_sender = event_sender.clone();
    let log_script_id = script_id.to_string();

    RustExecutionHooks {
        on_delay_sampled_ms: Some(Box::new(move |delay_ms| {
            let _ = delay_sender.try_send(WorkerEvent::DelaySampled {
                script_id: delay_script_id.clone(),
                delay_ms,
            });
        })),
        on_action_executed: Some(Box::new(move || {
            let _ = action_sender.try_send(WorkerEvent::ActionExecuted {
                script_id: action_script_id.clone(),
            });
        })),
        on_log_message: Some(Box::new(move |message| {
            let _ = log_sender.try_send(WorkerEvent::LogMessage {
                script_id: log_script_id.clone(),
                message,
            });
        })),
    }
}

pub(crate) fn build_tray_runtime_state(
    is_emergency_stopped: bool,
    scripts: &[ScriptDocument],
    statuses: &[ScriptRuntimeStatus],
    focused_window: Option<&ActiveWindowSnapshot>,
) -> TrayRuntimeState {
    let enabled_script_count = scripts.iter().filter(|script| script.is_enabled).count();
    let active_script_count = statuses
        .iter()
        .filter(|status| {
            matches!(
                status.phase,
                ScriptExecutionPhase::Armed | ScriptExecutionPhase::Running
            )
        })
        .count();

    let overall_phase = if is_emergency_stopped {
        ScriptExecutionPhase::StoppedByKillSwitch
    } else if statuses
        .iter()
        .any(|status| status.phase == ScriptExecutionPhase::Running)
    {
        ScriptExecutionPhase::Running
    } else if statuses
        .iter()
        .any(|status| status.phase == ScriptExecutionPhase::Armed)
    {
        ScriptExecutionPhase::Armed
    } else if statuses
        .iter()
        .any(|status| status.phase == ScriptExecutionPhase::Compiling)
    {
        ScriptExecutionPhase::Compiling
    } else if enabled_script_count > 0 {
        ScriptExecutionPhase::WaitingForFocus
    } else {
        ScriptExecutionPhase::Disabled
    };

    let focused_window_title = focused_window
        .map(|window| window.title.trim())
        .filter(|title| !title.is_empty())
        .map(ToString::to_string);

    let status_tooltip = format!(
        "ClickForge: {} ({} active / {} enabled)",
        overall_phase.display_label(),
        active_script_count,
        enabled_script_count
    );

    TrayRuntimeState {
        overall_phase,
        enabled_script_count,
        active_script_count,
        focused_window_title,
        status_tooltip,
    }
}

pub(crate) fn compute_script_revision_hash(script: &ScriptDocument) -> u64 {
    let mut hasher = DefaultHasher::new();
    script.mode.hash(&mut hasher);
    script.rust_source.hash(&mut hasher);
    if let Ok(serialized_program) = serde_json::to_vec(&script.visual_program) {
        serialized_program.hash(&mut hasher);
    }
    hasher.finish()
}

pub(crate) fn compute_text_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}
