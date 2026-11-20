use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use clickforge_sdk::{InputTrigger, KeyCode};

use crate::engine::coordinator::{CoordinatorStartConfig, KeyCaptureState, WindowSnapshotProvider};
use crate::engine::coordinator_telemetry::CoordinatorTelemetryStore;
use crate::engine::rust_script_runner::default_script_build_cache_dir;
use crate::engine::worker_supervisor::{
    RustWorkerLaunchOutcome, ScriptWorkerSupervisor, WorkerEvent, compute_script_revision_hash,
};
use crate::models::{
    ActiveWindowSnapshot, CompileResultSnapshot, CoordinatorCommand, CoordinatorSnapshot,
    InputSink, PhysicalInputReader, ScriptDocument, ScriptExecutionPhase, ScriptMode,
    TelemetryLevel, TriggerInterceptControl, TriggerInterceptSpec,
    validate_script_for_persistence_and_execution,
};

const MAX_WORKER_EVENT_QUEUE_CAPACITY: usize = 512;
const COORDINATOR_TICK_INTERVAL_MS: u64 = 10;
const WINDOW_REFRESH_INTERVAL_MS: u64 = 750;

pub(crate) struct CoordinatorLoop {
    scripts: Vec<ScriptDocument>,
    arm_toggle_by_id: HashMap<String, bool>,
    self_stopped_revision_by_id: HashMap<String, u64>,
    polled_arm_key_state: HashMap<KeyCode, bool>,
    supervisor: ScriptWorkerSupervisor,
    telemetry: CoordinatorTelemetryStore,
    current_intercept: Option<TriggerInterceptSpec>,
    input_sink: Arc<dyn InputSink>,
    physical_input: Arc<dyn PhysicalInputReader>,
    trigger_intercept: Arc<dyn TriggerInterceptControl>,
    window_snapshot_provider: WindowSnapshotProvider,
    input_event_receiver: Option<Receiver<(InputTrigger, bool)>>,
    focus_event_receiver: Option<Receiver<ActiveWindowSnapshot>>,
    command_receiver: Receiver<CoordinatorCommand>,
    worker_event_receiver: Receiver<WorkerEvent>,
    shutdown_flag: Arc<AtomicBool>,
    key_capture: Arc<KeyCaptureState>,
}

impl CoordinatorLoop {
    pub(crate) fn new(
        config: CoordinatorStartConfig,
        command_receiver: Receiver<CoordinatorCommand>,
        shared_snapshot: Arc<RwLock<CoordinatorSnapshot>>,
        shared_compile_results: Arc<RwLock<HashMap<String, CompileResultSnapshot>>>,
        key_capture: Arc<KeyCaptureState>,
    ) -> Self {
        let (worker_event_sender, worker_event_receiver) =
            mpsc::sync_channel::<WorkerEvent>(MAX_WORKER_EVENT_QUEUE_CAPACITY);
        let cache_dir = config
            .script_build_cache_dir
            .unwrap_or_else(default_script_build_cache_dir);
        let supervisor = ScriptWorkerSupervisor::new(cache_dir, worker_event_sender);
        let telemetry = CoordinatorTelemetryStore::new(
            config.input_status,
            shared_snapshot,
            shared_compile_results,
            config.on_tray_state_changed,
            config.on_ui_repaint_needed,
        );

        let mut coordinator = Self {
            scripts: Vec::new(),
            arm_toggle_by_id: HashMap::new(),
            self_stopped_revision_by_id: HashMap::new(),
            polled_arm_key_state: HashMap::new(),
            supervisor,
            telemetry,
            current_intercept: None,
            input_sink: config.input_sink,
            physical_input: config.physical_input,
            trigger_intercept: config.trigger_intercept,
            window_snapshot_provider: config.window_snapshot_provider,
            input_event_receiver: config.input_event_receiver,
            focus_event_receiver: config.focus_event_receiver,
            command_receiver,
            worker_event_receiver,
            shutdown_flag: config.shutdown_flag,
            key_capture,
        };
        coordinator.apply_sync_scripts(config.initial_scripts);
        coordinator.refresh_window_and_process_snapshot();
        coordinator
    }

    pub(crate) fn run(&mut self) {
        let mut last_window_refresh = Instant::now();
        self.reconcile_all_scripts();
        self.telemetry.publish_if_dirty(&self.scripts);

        while !self.shutdown_flag.load(Ordering::SeqCst) {
            if !self.drain_commands() {
                break;
            }
            self.drain_input_events();
            self.poll_physical_key_fallbacks();
            self.drain_focus_events();
            self.drain_worker_events();

            if last_window_refresh.elapsed() >= Duration::from_millis(WINDOW_REFRESH_INTERVAL_MS) {
                self.refresh_window_and_process_snapshot();
                last_window_refresh = Instant::now();
            }

            self.reconcile_all_scripts();
            self.telemetry.publish_if_dirty(&self.scripts);
            thread::sleep(Duration::from_millis(COORDINATOR_TICK_INTERVAL_MS));
        }

        self.stop_all_workers_and_release_inputs();
        self.telemetry.publish_if_dirty(&self.scripts);
    }

    fn drain_commands(&mut self) -> bool {
        loop {
            match self.command_receiver.try_recv() {
                Ok(CoordinatorCommand::Shutdown) => return false,
                Ok(command) => self.handle_command(command),
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
    }

    fn handle_command(&mut self, command: CoordinatorCommand) {
        match command {
            CoordinatorCommand::SyncScripts(incoming) => self.apply_sync_scripts(incoming),
            CoordinatorCommand::UpsertScript(script) => self.apply_upsert_script(script),
            CoordinatorCommand::RemoveScript { script_id } => self.apply_remove_script(&script_id),
            CoordinatorCommand::SetScriptEnabled {
                script_id,
                is_enabled,
            } => self.apply_set_script_enabled(&script_id, is_enabled),
            CoordinatorCommand::CompileRustScript(script) => {
                self.supervisor.clear_failed_compile(&script.id);
                self.telemetry.set_status_fields(
                    &script.id,
                    ScriptExecutionPhase::Compiling,
                    false,
                    false,
                    None,
                );
                self.supervisor.spawn_background_compile(script);
            }
            CoordinatorCommand::EmergencyStopAll { reason } => self.trigger_emergency_stop(&reason),
            CoordinatorCommand::DisableAllScripts => {
                for script in &mut self.scripts {
                    script.is_enabled = false;
                }
                self.telemetry.clear_emergency_stop();
                self.stop_all_workers_and_release_inputs();
                self.telemetry.record_log(
                    TelemetryLevel::Info,
                    None,
                    "All scripts disabled.".to_string(),
                );
            }
            CoordinatorCommand::ClearEmergencyStop => {
                self.telemetry.clear_emergency_stop();
                self.self_stopped_revision_by_id.clear();
                self.telemetry.record_log(
                    TelemetryLevel::Info,
                    None,
                    "Emergency stop cleared; valid enabled scripts may re-arm.".to_string(),
                );
            }
            CoordinatorCommand::RefreshWindowsAndProcesses => {
                self.refresh_window_and_process_snapshot();
            }
            CoordinatorCommand::Shutdown => {}
        }
        self.telemetry.mark_dirty();
    }

    fn apply_sync_scripts(&mut self, incoming_scripts: Vec<ScriptDocument>) {
        let incoming_ids: Vec<String> = incoming_scripts
            .iter()
            .map(|item| item.id.clone())
            .collect();
        let removed_ids: Vec<String> = self
            .scripts
            .iter()
            .filter(|existing| !incoming_ids.contains(&existing.id))
            .map(|existing| existing.id.clone())
            .collect();
        for removed_id in removed_ids {
            self.supervisor.clear_script_cache(&removed_id);
            self.telemetry.remove_script_entry(&removed_id);
            self.arm_toggle_by_id.remove(&removed_id);
            self.self_stopped_revision_by_id.remove(&removed_id);
        }

        self.scripts = incoming_scripts;
        for script in &self.scripts {
            self.telemetry.ensure_script_entry(&script.id);
            self.arm_toggle_by_id
                .entry(script.id.clone())
                .or_insert(true);
        }
    }

    fn apply_upsert_script(&mut self, script: ScriptDocument) {
        let script_id = script.id.clone();
        self.self_stopped_revision_by_id.remove(&script_id);
        if let Some(existing) = self.scripts.iter_mut().find(|item| item.id == script_id) {
            *existing = script;
        } else {
            self.scripts.push(script);
        }
        self.telemetry.ensure_script_entry(&script_id);
        self.arm_toggle_by_id.entry(script_id).or_insert(true);
    }

    fn apply_remove_script(&mut self, script_id: &str) {
        self.supervisor.clear_script_cache(script_id);
        self.scripts.retain(|script| script.id != script_id);
        self.telemetry.remove_script_entry(script_id);
        self.arm_toggle_by_id.remove(script_id);
        self.self_stopped_revision_by_id.remove(script_id);
    }

    fn apply_set_script_enabled(&mut self, script_id: &str, is_enabled: bool) {
        let Some(script_index) = self.scripts.iter().position(|item| item.id == script_id) else {
            return;
        };
        if !is_enabled {
            self.scripts[script_index].is_enabled = false;
            self.supervisor.stop_worker_for_script(script_id);
            self.self_stopped_revision_by_id.remove(script_id);
            return;
        }
        if let Err(validation_error) =
            validate_script_for_persistence_and_execution(&self.scripts[script_index])
        {
            let error_message = validation_error.to_string();
            self.scripts[script_index].is_enabled = false;
            self.telemetry
                .set_script_error(script_id, error_message.clone());
            self.telemetry.record_log(
                TelemetryLevel::Warning,
                Some(script_id.to_string()),
                format!("Blocked enabling invalid script: {error_message}"),
            );
            return;
        }
        self.scripts[script_index].is_enabled = true;
        self.arm_toggle_by_id.insert(script_id.to_string(), true);
        self.self_stopped_revision_by_id.remove(script_id);
    }

    fn drain_input_events(&mut self) {
        let mut received_events = Vec::new();
        if let Some(receiver) = self.input_event_receiver.as_ref() {
            while let Ok(event) = receiver.try_recv() {
                received_events.push(event);
            }
        }
        for (input_trigger, is_pressed) in received_events {
            if is_pressed && let InputTrigger::Key(pressed_key) = input_trigger {
                self.handle_physical_key_press(pressed_key);
            }
        }
    }

    fn poll_physical_key_fallbacks(&mut self) {
        if self.key_capture.is_capturing.load(Ordering::SeqCst)
            && let Some((captured_key, sequence)) = self.physical_input.last_pressed_key_snapshot()
            && sequence > self.key_capture.baseline_sequence.load(Ordering::SeqCst)
        {
            self.complete_key_capture(captured_key);
        }

        if !self.telemetry.is_emergency_stopped()
            && let Some((script_name, kill_key)) = self.find_pressed_kill_switch_polled()
        {
            self.trigger_emergency_stop(&format!(
                "Hard Kill-Switch ({}) pressed for '{}'",
                kill_key.display_label(),
                script_name
            ));
            return;
        }

        if self.input_event_receiver.is_some()
            || self.key_capture.is_capturing.load(Ordering::SeqCst)
        {
            return;
        }
        let arm_keys: Vec<KeyCode> = self
            .scripts
            .iter()
            .filter(|script| script.is_enabled)
            .filter_map(|script| script.arm_toggle_key)
            .collect();
        for arm_key in arm_keys {
            let is_down = self.physical_input.is_key_pressed(arm_key);
            let was_down = self
                .polled_arm_key_state
                .insert(arm_key, is_down)
                .unwrap_or(false);
            if is_down && !was_down {
                self.toggle_scripts_bound_to_arm_key(arm_key);
            }
        }
    }

    fn handle_physical_key_press(&mut self, pressed_key: KeyCode) {
        let kill_match = self
            .scripts
            .iter()
            .find(|script| script.is_enabled && script.kill_switch_key == Some(pressed_key))
            .map(|script| script.name.clone());
        if let Some(script_name) = kill_match {
            self.key_capture.is_capturing.store(false, Ordering::SeqCst);
            if !self.telemetry.is_emergency_stopped() {
                self.trigger_emergency_stop(&format!(
                    "Hard Kill-Switch ({}) pressed for '{}'",
                    pressed_key.display_label(),
                    script_name
                ));
            }
            return;
        }
        if self.key_capture.is_capturing.load(Ordering::SeqCst) {
            self.complete_key_capture(pressed_key);
            return;
        }
        self.toggle_scripts_bound_to_arm_key(pressed_key);
    }

    fn complete_key_capture(&mut self, pressed_key: KeyCode) {
        self.key_capture.is_capturing.store(false, Ordering::SeqCst);
        if let Ok(mut guard) = self.key_capture.captured_key.lock() {
            *guard = (pressed_key != KeyCode::Escape).then_some(pressed_key);
        }
        self.telemetry.notify_ui_repaint();
    }

    fn find_pressed_kill_switch_polled(&self) -> Option<(String, KeyCode)> {
        for script in &self.scripts {
            let Some(kill_key) = script.kill_switch_key.filter(|_| script.is_enabled) else {
                continue;
            };
            if self.physical_input.is_key_pressed(kill_key) {
                return Some((script.name.clone(), kill_key));
            }
        }
        None
    }

    fn toggle_scripts_bound_to_arm_key(&mut self, arm_key: KeyCode) {
        let matches: Vec<(String, String)> = self
            .scripts
            .iter()
            .filter(|script| script.is_enabled && script.arm_toggle_key == Some(arm_key))
            .map(|script| (script.id.clone(), script.name.clone()))
            .collect();
        for (script_id, script_name) in matches {
            let next_armed = !self
                .arm_toggle_by_id
                .get(&script_id)
                .copied()
                .unwrap_or(true);
            self.arm_toggle_by_id.insert(script_id.clone(), next_armed);
            let state_label = if next_armed { "armed" } else { "disarmed" };
            self.telemetry.record_log(
                TelemetryLevel::Info,
                Some(script_id),
                format!(
                    "Script '{script_name}' {state_label} via {}.",
                    arm_key.display_label()
                ),
            );
        }
    }

    fn drain_focus_events(&mut self) {
        let mut latest_window: Option<ActiveWindowSnapshot> = None;
        if let Some(receiver) = self.focus_event_receiver.as_ref() {
            while let Ok(window_snapshot) = receiver.try_recv() {
                latest_window = Some(window_snapshot);
            }
        }
        let Some(window_snapshot) = latest_window else {
            return;
        };
        let next_focused =
            (!window_snapshot.is_empty() && window_snapshot.is_focused).then_some(window_snapshot);
        self.telemetry.set_focused_window(next_focused);
    }

    fn refresh_window_and_process_snapshot(&mut self) {
        let (focused_window, open_windows, running_processes) = (self.window_snapshot_provider)();
        self.telemetry
            .set_windows_and_processes(focused_window, open_windows, running_processes);
    }

    fn drain_worker_events(&mut self) {
        while let Ok(worker_event) = self.worker_event_receiver.try_recv() {
            self.handle_worker_event(worker_event);
        }
    }

    fn handle_worker_event(&mut self, worker_event: WorkerEvent) {
        match worker_event {
            WorkerEvent::ActiveBlockChanged {
                script_id,
                block_id,
            } => self.telemetry.record_active_block(&script_id, block_id),
            WorkerEvent::DelaySampled {
                script_id,
                delay_ms,
            } => self.telemetry.record_delay_sample(&script_id, delay_ms),
            WorkerEvent::ActionExecuted { script_id } => {
                self.telemetry.record_action_executed(&script_id);
            }
            WorkerEvent::LogMessage { script_id, message } => {
                self.telemetry
                    .record_log(TelemetryLevel::Info, Some(script_id), message);
            }
            WorkerEvent::WorkerExited {
                script_id,
                revision_hash,
                stopped_by_block,
                error_message,
            } => {
                if self
                    .supervisor
                    .is_running_revision(&script_id, revision_hash)
                {
                    self.supervisor.stop_worker_for_script(&script_id);
                }
                if stopped_by_block {
                    self.self_stopped_revision_by_id
                        .insert(script_id.clone(), revision_hash);
                    self.telemetry.record_log(
                        TelemetryLevel::Info,
                        Some(script_id.clone()),
                        "Script halted cleanly via Stop Script block.".to_string(),
                    );
                }
                if let Some(error_text) = error_message {
                    self.self_stopped_revision_by_id
                        .insert(script_id.clone(), revision_hash);
                    self.telemetry
                        .set_script_error(&script_id, error_text.clone());
                    self.telemetry
                        .record_log(TelemetryLevel::Error, Some(script_id), error_text);
                }
            }
            WorkerEvent::CompileFinished {
                script_id,
                source_hash,
                result,
            } => {
                self.supervisor
                    .record_compile_outcome(&script_id, source_hash, &result);
                self.telemetry.record_compile_result(&script_id, result);
            }
        }
    }

    fn reconcile_all_scripts(&mut self) {
        let scripts_snapshot = self.scripts.clone();
        let mut selected_game_intercept: Option<(String, TriggerInterceptSpec)> = None;
        let mut global_intercept: Option<(String, TriggerInterceptSpec)> = None;

        for script in &scripts_snapshot {
            let Some(intercept_spec) = self.reconcile_single_script(script) else {
                continue;
            };
            if script.target_scope.is_global() {
                if global_intercept.is_none() {
                    global_intercept = Some((script.id.clone(), intercept_spec));
                }
            } else if selected_game_intercept.is_none() {
                selected_game_intercept = Some((script.id.clone(), intercept_spec));
            }
        }

        let chosen = selected_game_intercept.or(global_intercept);
        let desired_spec = chosen.as_ref().map(|(_, spec)| *spec);
        let owner_id = chosen.map(|(script_id, _)| script_id);
        self.apply_trigger_intercept(desired_spec, owner_id.as_deref());
    }

    fn reconcile_single_script(&mut self, script: &ScriptDocument) -> Option<TriggerInterceptSpec> {
        let script_id = &script.id;
        if !script.is_enabled {
            self.supervisor.stop_worker_for_script(script_id);
            let phase = if self.supervisor.is_compiling(script_id) {
                ScriptExecutionPhase::Compiling
            } else {
                ScriptExecutionPhase::Disabled
            };
            self.telemetry
                .set_status_fields(script_id, phase, false, false, None);
            return None;
        }
        if self.telemetry.is_emergency_stopped() {
            self.supervisor.stop_worker_for_script(script_id);
            self.telemetry.set_status_fields(
                script_id,
                ScriptExecutionPhase::StoppedByKillSwitch,
                false,
                false,
                None,
            );
            return None;
        }
        if let Err(validation_error) = validate_script_for_persistence_and_execution(script) {
            self.supervisor.stop_worker_for_script(script_id);
            self.telemetry
                .set_script_error(script_id, validation_error.to_string());
            return None;
        }

        let is_focus_matched = script.target_scope.matches_focused_window(
            self.telemetry.focused_window(),
            self.telemetry.running_processes(),
        );
        let is_armed_by_toggle = script.arm_toggle_key.is_none()
            || self
                .arm_toggle_by_id
                .get(script_id)
                .copied()
                .unwrap_or(true);
        if !is_focus_matched {
            self.supervisor.stop_worker_for_script(script_id);
            self.self_stopped_revision_by_id.remove(script_id);
            let phase = if self.supervisor.is_compiling(script_id) {
                ScriptExecutionPhase::Compiling
            } else {
                ScriptExecutionPhase::WaitingForFocus
            };
            self.telemetry
                .set_status_fields(script_id, phase, false, is_armed_by_toggle, None);
            return None;
        }
        if !is_armed_by_toggle {
            self.supervisor.stop_worker_for_script(script_id);
            let phase = if self.supervisor.is_compiling(script_id) {
                ScriptExecutionPhase::Compiling
            } else {
                ScriptExecutionPhase::Disabled
            };
            self.telemetry
                .set_status_fields(script_id, phase, true, false, None);
            return None;
        }

        let revision_hash = compute_script_revision_hash(script);
        if self.self_stopped_revision_by_id.get(script_id) == Some(&revision_hash) {
            return None;
        }
        if !self.ensure_script_worker_running(script, revision_hash) {
            return None;
        }
        script
            .intercept_trigger_input
            .then(|| script.effective_trigger_input())
            .flatten()
            .map(|trigger_input| TriggerInterceptSpec { trigger_input })
    }

    fn ensure_script_worker_running(
        &mut self,
        script: &ScriptDocument,
        revision_hash: u64,
    ) -> bool {
        if script.mode == ScriptMode::VisualBlocks {
            if !self
                .supervisor
                .is_running_revision(&script.id, revision_hash)
            {
                self.supervisor.spawn_visual_worker(
                    script,
                    revision_hash,
                    Arc::clone(&self.input_sink),
                    Arc::clone(&self.physical_input),
                );
                self.telemetry.set_status_fields(
                    &script.id,
                    ScriptExecutionPhase::Armed,
                    true,
                    true,
                    None,
                );
            }
            return true;
        }

        match self.supervisor.ensure_rust_worker(
            script,
            revision_hash,
            Arc::clone(&self.input_sink),
            Arc::clone(&self.physical_input),
        ) {
            RustWorkerLaunchOutcome::AlreadyRunning => true,
            RustWorkerLaunchOutcome::Spawned => {
                self.telemetry.set_status_fields(
                    &script.id,
                    ScriptExecutionPhase::Running,
                    true,
                    true,
                    None,
                );
                true
            }
            RustWorkerLaunchOutcome::CompileStarted
            | RustWorkerLaunchOutcome::CompilingInProgress => {
                self.telemetry.set_status_fields(
                    &script.id,
                    ScriptExecutionPhase::Compiling,
                    true,
                    true,
                    None,
                );
                false
            }
            RustWorkerLaunchOutcome::CompileFailed => false,
        }
    }

    fn trigger_emergency_stop(&mut self, reason: &str) {
        self.stop_all_workers_and_release_inputs();
        let enabled_ids: Vec<String> = self
            .scripts
            .iter()
            .filter(|script| script.is_enabled)
            .map(|script| script.id.clone())
            .collect();
        self.telemetry.mark_emergency_stopped(&enabled_ids, reason);
    }

    fn stop_all_workers_and_release_inputs(&mut self) {
        self.supervisor.signal_abort_all();
        self.apply_trigger_intercept(None, None);
        let _ = self.input_sink.release_all_pressed();
        self.supervisor.join_all_running();
        let _ = self.input_sink.release_all_pressed();
    }

    fn apply_trigger_intercept(
        &mut self,
        desired_spec: Option<TriggerInterceptSpec>,
        owner_script_id: Option<&str>,
    ) {
        if self.current_intercept != desired_spec {
            match self.trigger_intercept.set_intercept(desired_spec) {
                Ok(()) => self.current_intercept = desired_spec,
                Err(intercept_error) => {
                    self.current_intercept = None;
                    self.telemetry.record_log(
                        TelemetryLevel::Warning,
                        owner_script_id.map(ToString::to_string),
                        format!("Trigger intercept update failed: {intercept_error}"),
                    );
                }
            }
            self.telemetry.mark_dirty();
        }
        let is_grab_active = self.trigger_intercept.is_intercept_active();
        self.telemetry
            .update_intercept_flags(is_grab_active, owner_script_id);
    }
}
