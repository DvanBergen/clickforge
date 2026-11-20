use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};

use crate::engine::worker_supervisor::build_tray_runtime_state;
use crate::models::{
    ActiveWindowSnapshot, BlockId, CompileResultSnapshot, CoordinatorSnapshot,
    InputSubsystemStatus, MAX_DELAY_HISTORY_SAMPLES, MAX_TELEMETRY_ENTRIES, RunningProcessEntry,
    ScriptDocument, ScriptExecutionPhase, ScriptRuntimeStatus, TelemetryLevel, TelemetryLogEntry,
    TrayRuntimeState,
};

/// Owns bounded telemetry logs, delay history, script runtime statuses, and UI/tray publishing.
pub(crate) struct CoordinatorTelemetryStore {
    statuses_by_id: HashMap<String, ScriptRuntimeStatus>,
    focused_window: Option<ActiveWindowSnapshot>,
    open_windows: Vec<ActiveWindowSnapshot>,
    running_processes: Vec<RunningProcessEntry>,
    telemetry_logs: VecDeque<TelemetryLogEntry>,
    recent_delay_samples_ms: VecDeque<u64>,
    next_log_sequence: u64,
    emergency_stop_count: u64,
    is_emergency_stopped: bool,
    input_status: InputSubsystemStatus,
    last_compile_result: Option<CompileResultSnapshot>,
    last_tray_state: Option<TrayRuntimeState>,
    shared_snapshot: Arc<RwLock<CoordinatorSnapshot>>,
    shared_compile_results: Arc<RwLock<HashMap<String, CompileResultSnapshot>>>,
    on_tray_state_changed: Option<Arc<dyn Fn(TrayRuntimeState) + Send + Sync>>,
    on_ui_repaint_needed: Option<Arc<dyn Fn() + Send + Sync>>,
    is_snapshot_dirty: bool,
}

impl CoordinatorTelemetryStore {
    pub(crate) fn new(
        input_status: InputSubsystemStatus,
        shared_snapshot: Arc<RwLock<CoordinatorSnapshot>>,
        shared_compile_results: Arc<RwLock<HashMap<String, CompileResultSnapshot>>>,
        on_tray_state_changed: Option<Arc<dyn Fn(TrayRuntimeState) + Send + Sync>>,
        on_ui_repaint_needed: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        Self {
            statuses_by_id: HashMap::new(),
            focused_window: None,
            open_windows: Vec::new(),
            running_processes: Vec::new(),
            telemetry_logs: VecDeque::with_capacity(MAX_TELEMETRY_ENTRIES),
            recent_delay_samples_ms: VecDeque::with_capacity(MAX_DELAY_HISTORY_SAMPLES),
            next_log_sequence: 1,
            emergency_stop_count: 0,
            is_emergency_stopped: false,
            input_status,
            last_compile_result: None,
            last_tray_state: None,
            shared_snapshot,
            shared_compile_results,
            on_tray_state_changed,
            on_ui_repaint_needed,
            is_snapshot_dirty: true,
        }
    }

    pub(crate) fn is_emergency_stopped(&self) -> bool {
        self.is_emergency_stopped
    }

    pub(crate) fn focused_window(&self) -> Option<&ActiveWindowSnapshot> {
        self.focused_window.as_ref()
    }

    pub(crate) fn running_processes(&self) -> &[RunningProcessEntry] {
        &self.running_processes
    }

    pub(crate) fn mark_dirty(&mut self) {
        self.is_snapshot_dirty = true;
    }

    pub(crate) fn ensure_script_entry(&mut self, script_id: &str) {
        self.statuses_by_id
            .entry(script_id.to_string())
            .or_insert_with(|| ScriptRuntimeStatus::disabled(script_id));
        self.is_snapshot_dirty = true;
    }

    pub(crate) fn remove_script_entry(&mut self, script_id: &str) {
        self.statuses_by_id.remove(script_id);
        self.is_snapshot_dirty = true;
    }

    pub(crate) fn set_focused_window(&mut self, next_focused: Option<ActiveWindowSnapshot>) {
        if self.focused_window != next_focused {
            self.focused_window = next_focused;
            self.is_snapshot_dirty = true;
        }
    }

    pub(crate) fn set_windows_and_processes(
        &mut self,
        focused_window: Option<ActiveWindowSnapshot>,
        open_windows: Vec<ActiveWindowSnapshot>,
        running_processes: Vec<RunningProcessEntry>,
    ) {
        let has_changed = self.focused_window != focused_window
            || self.open_windows != open_windows
            || self.running_processes != running_processes;
        if !has_changed {
            return;
        }
        self.focused_window = focused_window;
        self.open_windows = open_windows;
        self.running_processes = running_processes;
        self.is_snapshot_dirty = true;
    }

    pub(crate) fn set_status_fields(
        &mut self,
        script_id: &str,
        phase: ScriptExecutionPhase,
        is_focus_matched: bool,
        is_armed_by_toggle: bool,
        last_error: Option<String>,
    ) {
        let status = self
            .statuses_by_id
            .entry(script_id.to_string())
            .or_insert_with(|| ScriptRuntimeStatus::disabled(script_id));
        let did_change = status.phase != phase
            || status.is_focus_matched != is_focus_matched
            || status.is_armed_by_toggle != is_armed_by_toggle
            || status.last_error != last_error;
        if !did_change {
            return;
        }
        status.phase = phase;
        status.is_focus_matched = is_focus_matched;
        status.is_armed_by_toggle = is_armed_by_toggle;
        if !matches!(phase, ScriptExecutionPhase::Running) {
            status.active_block_id = None;
        }
        status.last_error = last_error;
        status.updated_at_unix_ms = ScriptDocument::current_unix_timestamp_ms();
        self.is_snapshot_dirty = true;
    }

    pub(crate) fn set_script_error(&mut self, script_id: &str, error_message: String) {
        self.set_status_fields(
            script_id,
            ScriptExecutionPhase::Error,
            false,
            false,
            Some(error_message),
        );
    }

    pub(crate) fn record_active_block(&mut self, script_id: &str, block_id: Option<BlockId>) {
        let Some(status) = self.statuses_by_id.get_mut(script_id) else {
            return;
        };
        status.active_block_id = block_id;
        if matches!(
            status.phase,
            ScriptExecutionPhase::Armed | ScriptExecutionPhase::Running
        ) {
            status.phase = if block_id.is_some() {
                ScriptExecutionPhase::Running
            } else {
                ScriptExecutionPhase::Armed
            };
        }
        status.updated_at_unix_ms = ScriptDocument::current_unix_timestamp_ms();
        self.is_snapshot_dirty = true;
    }

    pub(crate) fn record_delay_sample(&mut self, script_id: &str, delay_ms: u64) {
        if let Some(status) = self.statuses_by_id.get_mut(script_id) {
            status.last_sampled_delay_ms = Some(delay_ms);
            status.updated_at_unix_ms = ScriptDocument::current_unix_timestamp_ms();
        }
        if self.recent_delay_samples_ms.len() >= MAX_DELAY_HISTORY_SAMPLES {
            self.recent_delay_samples_ms.pop_front();
        }
        self.recent_delay_samples_ms.push_back(delay_ms);
        self.is_snapshot_dirty = true;
    }

    pub(crate) fn record_action_executed(&mut self, script_id: &str) {
        if let Some(status) = self.statuses_by_id.get_mut(script_id) {
            status.total_actions_emitted = status.total_actions_emitted.saturating_add(1);
            status.updated_at_unix_ms = ScriptDocument::current_unix_timestamp_ms();
            self.is_snapshot_dirty = true;
        }
    }

    pub(crate) fn record_compile_result(&mut self, script_id: &str, result: CompileResultSnapshot) {
        if result.is_success {
            self.record_log(
                TelemetryLevel::Info,
                Some(script_id.to_string()),
                format!("Compiled Rust script in {} ms.", result.duration_ms),
            );
        } else {
            let error_summary = result
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.clone())
                .unwrap_or_else(|| "Rust script compilation failed.".to_string());
            self.set_script_error(script_id, error_summary.clone());
            self.record_log(
                TelemetryLevel::Error,
                Some(script_id.to_string()),
                format!("Rust compile failed: {error_summary}"),
            );
        }
        if let Ok(mut guard) = self.shared_compile_results.write() {
            guard.insert(script_id.to_string(), result.clone());
        }
        self.last_compile_result = Some(result);
        self.is_snapshot_dirty = true;
    }

    pub(crate) fn mark_emergency_stopped(&mut self, enabled_script_ids: &[String], reason: &str) {
        self.is_emergency_stopped = true;
        self.emergency_stop_count = self.emergency_stop_count.saturating_add(1);
        for script_id in enabled_script_ids {
            self.set_status_fields(
                script_id,
                ScriptExecutionPhase::StoppedByKillSwitch,
                false,
                false,
                None,
            );
        }
        self.record_log(
            TelemetryLevel::Warning,
            None,
            format!("EMERGENCY STOP: {reason}"),
        );
    }

    pub(crate) fn clear_emergency_stop(&mut self) {
        self.is_emergency_stopped = false;
        self.is_snapshot_dirty = true;
    }

    pub(crate) fn update_intercept_flags(
        &mut self,
        is_grab_active: bool,
        owner_script_id: Option<&str>,
    ) {
        if self.input_status.is_trigger_intercept_active != is_grab_active {
            self.input_status.is_trigger_intercept_active = is_grab_active;
            self.is_snapshot_dirty = true;
        }
        for (script_id, status) in &mut self.statuses_by_id {
            let owns_grab = is_grab_active && owner_script_id == Some(script_id.as_str());
            if status.is_intercept_grab_active != owns_grab {
                status.is_intercept_grab_active = owns_grab;
                self.is_snapshot_dirty = true;
            }
        }
    }

    pub(crate) fn record_log(
        &mut self,
        level: TelemetryLevel,
        script_id: Option<String>,
        message: String,
    ) {
        if self.telemetry_logs.len() >= MAX_TELEMETRY_ENTRIES {
            self.telemetry_logs.pop_front();
        }
        self.telemetry_logs.push_back(TelemetryLogEntry {
            sequence_number: self.next_log_sequence,
            timestamp_unix_ms: ScriptDocument::current_unix_timestamp_ms(),
            level,
            script_id,
            message,
        });
        self.next_log_sequence = self.next_log_sequence.saturating_add(1);
        self.is_snapshot_dirty = true;
    }

    pub(crate) fn publish_if_dirty(&mut self, scripts: &[ScriptDocument]) {
        if !self.is_snapshot_dirty {
            return;
        }
        self.is_snapshot_dirty = false;

        let script_statuses: Vec<ScriptRuntimeStatus> = scripts
            .iter()
            .map(|script| {
                self.statuses_by_id
                    .get(&script.id)
                    .cloned()
                    .unwrap_or_else(|| ScriptRuntimeStatus::disabled(&script.id))
            })
            .collect();
        let tray_state = build_tray_runtime_state(
            self.is_emergency_stopped,
            scripts,
            &script_statuses,
            self.focused_window.as_ref(),
        );
        let snapshot = CoordinatorSnapshot {
            focused_window: self.focused_window.clone(),
            open_windows: self.open_windows.clone(),
            running_processes: self.running_processes.clone(),
            script_statuses,
            recent_delay_samples_ms: self.recent_delay_samples_ms.iter().copied().collect(),
            telemetry_logs: self.telemetry_logs.iter().cloned().collect(),
            input_status: self.input_status.clone(),
            last_compile_result: self.last_compile_result.clone(),
            emergency_stop_count: self.emergency_stop_count,
            is_emergency_stopped: self.is_emergency_stopped,
        };
        if let Ok(mut guard) = self.shared_snapshot.write() {
            *guard = snapshot;
        }
        if self.last_tray_state.as_ref() != Some(&tray_state) {
            self.last_tray_state = Some(tray_state.clone());
            if let Some(callback) = self.on_tray_state_changed.as_ref() {
                callback(tray_state);
            }
        }
        self.notify_ui_repaint();
    }

    pub(crate) fn notify_ui_repaint(&self) {
        if let Some(callback) = self.on_ui_repaint_needed.as_ref() {
            callback();
        }
    }
}
